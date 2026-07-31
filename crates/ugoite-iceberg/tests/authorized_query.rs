use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
};
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{FieldType, FormDefinition, FormField, FormVersion};
use ugoite_domain::id::{EntryId, FieldId, FormId, SpaceId};
use ugoite_iceberg::{publication_context, query_context::AuthorizedQueryError, IcebergWorkspace};
use ugoite_storage::operator_from_uri;
use uuid::Uuid;

fn form(id: u128, name: &str) -> FormDefinition {
    FormDefinition {
        id: FormId::from(Uuid::from_u128(id)),
        version: FormVersion::new(1).unwrap(),
        name: name.into(),
        description: None,
        fields: vec![FormField {
            id: FieldId::new(100).unwrap(),
            name: "title".into(),
            field_type: FieldType::String,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }],
        allow_extra_attributes: false,
        extension_metadata: BTreeMap::new(),
    }
}

async fn create_form(workspace: &IcebergWorkspace, form: &FormDefinition) -> anyhow::Result<()> {
    workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.form",
            form,
        )?)?
        .create_form(form)
        .await
}

async fn append(
    workspace: &IcebergWorkspace,
    form: &FormDefinition,
    entry: u128,
    revision_id: u128,
    title: &str,
) -> anyhow::Result<i64> {
    let revision = revision(form, entry, revision_id, title);
    Ok(workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.entry",
            &revision,
        )?)?
        .append_revisions(form.id, vec![revision])
        .await?
        .snapshot_id)
}

fn revision(form: &FormDefinition, entry: u128, revision: u128, title: &str) -> EntryRevision {
    let mut values = BTreeMap::new();
    values.insert(FieldId::new(100).unwrap(), FieldValue::String(title.into()));
    EntryRevision {
        form_id: form.id,
        entry_id: EntryId::from(Uuid::from_u128(entry)),
        revision_id: Uuid::from_u128(revision).into(),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "test".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata::default(),
        values,
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
}

async fn append_duplicate(
    workspace: &IcebergWorkspace,
    form: &FormDefinition,
    entry: u128,
    revision_id: u128,
    title: &str,
) -> anyhow::Result<()> {
    let revision = revision(form, entry, revision_id, title);
    workspace
        .append_revisions_for_testing_allowing_duplicate_versions(form.id, vec![revision])
        .await?;
    Ok(())
}

fn policy(form: &FormDefinition, readable: &[u128]) -> AuthorizedQueryPolicy {
    AuthorizedQueryPolicy {
        forms: [(
            form.id,
            AuthorizedQueryForm {
                relation: "tasks".into(),
                entry_scope: EntryScope::Only(
                    readable
                        .iter()
                        .map(|id| EntryId::from(Uuid::from_u128(*id)))
                        .collect(),
                ),
                columns: ["title".into()].into_iter().collect(),
                system_columns: BTreeSet::new(),
            },
        )]
        .into_iter()
        .collect(),
        checkpoint: None,
        limits: QueryLimits {
            max_memory_bytes: 8 * 1024 * 1024,
            max_rows: 10,
            timeout: Duration::from_secs(5),
            max_concurrency: 1,
            allowed_functions: BTreeSet::new(),
        },
    }
}

#[tokio::test]
async fn context_makes_unapproved_forms_entries_columns_and_system_objects_unresolvable(
) -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(1)),
        "memory://authorized-query-boundary",
    )
    .await?;
    let tasks = form(2, "Tasks");
    let secrets = form(3, "Secrets");
    create_form(&workspace, &tasks).await?;
    create_form(&workspace, &secrets).await?;
    append(&workspace, &tasks, 10, 11, "allowed").await?;
    append(&workspace, &tasks, 12, 13, "denied").await?;
    append(&workspace, &secrets, 14, 15, "secret").await?;

    let context = workspace
        .authorized_query_context(policy(&tasks, &[10]))
        .await?;
    let batches = context
        .execute("SELECT * FROM tasks WHERE title IS NOT NULL")
        .await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1
    );

    for sql in [
        "SELECT entry_id FROM tasks",
        "SELECT t.entry_id FROM tasks AS t",
        "SELECT * FROM secrets",
        "SELECT * FROM information_schema.tables",
        "EXPLAIN SELECT * FROM tasks",
        "SELECT count(*) FROM tasks",
        "SELECT current_schema()",
        "SELECT current_catalog()",
        "SELECT unregistered_udf(title) FROM tasks",
        "SELECT UNNEST([1, 2])",
        "SELECT array_map([1, 2], x -> x + 1)",
        "SELECT * FROM generate_series(1, 2)",
    ] {
        assert!(
            context.execute(sql).await.is_err(),
            "{sql} must fail closed"
        );
    }
    for sql in [
        "SELECT * FROM tasks t JOIN tasks other ON t.title = other.title",
        "SELECT * FROM (SELECT * FROM tasks) nested",
        "SELECT t.title FROM tasks AS t",
    ] {
        let batches = context.execute(sql).await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            1
        );
    }
    assert_eq!(
        context
            .execute("SELECT title AS entry_id FROM tasks")
            .await?
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "a user-selected output alias must not resolve the hidden source column"
    );
    let empty = workspace
        .authorized_query_context(policy(&tasks, &[]))
        .await?;
    assert!(empty.execute("SELECT * FROM tasks").await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn context_binds_native_datafusion_parameters_without_sql_substitution() -> anyhow::Result<()>
{
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(101)),
        "memory://authorized-query-parameters",
    )
    .await?;
    let tasks = form(102, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 103, 104, "allowed").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[103]))
        .await?;

    let rows = context
        .execute_with_parameters(
            "SELECT title FROM tasks WHERE title = $title",
            HashMap::from([(
                "title".to_string(),
                datafusion::scalar::ScalarValue::Utf8(Some("allowed".into())),
            )]),
        )
        .await?;
    assert_eq!(rows.iter().map(|batch| batch.num_rows()).sum::<usize>(), 1);

    let injection = context
        .execute_with_parameters(
            "SELECT title FROM tasks WHERE title = $title",
            HashMap::from([(
                "title".to_string(),
                datafusion::scalar::ScalarValue::Utf8(Some("allowed' OR 1=1 --".into())),
            )]),
        )
        .await?;
    assert_eq!(
        injection
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        0,
        "parameter must remain a scalar value"
    );
    assert!(context
        .execute_with_parameters(
            "SELECT title FROM tasks WHERE title = $title",
            HashMap::new()
        )
        .await
        .is_err());
    assert!(context
        .execute_with_parameters(
            "SELECT title FROM tasks",
            HashMap::from([(
                "title".to_string(),
                datafusion::scalar::ScalarValue::Utf8(Some("allowed".into())),
            )]),
        )
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
async fn duplicate_maximum_versions_fail_every_sql_shape() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(600)),
        "memory://authorized-query-duplicate-maximum",
    )
    .await?;
    let tasks = form(601, "Tasks");
    create_form(&workspace, &tasks).await?;
    append_duplicate(&workspace, &tasks, 602, 603, "left").await?;
    append_duplicate(&workspace, &tasks, 602, 604, "right").await?;

    let mut duplicate_policy = policy(&tasks, &[602]);
    duplicate_policy
        .limits
        .allowed_functions
        .insert("count".into());
    let context = workspace.authorized_query_context(duplicate_policy).await?;
    for sql in [
        "SELECT * FROM tasks",
        "SELECT count(*) FROM tasks",
        "SELECT title FROM tasks LIMIT 1",
    ] {
        let error = context.execute(sql).await.expect_err(sql);
        assert!(matches!(
            error.downcast_ref::<AuthorizedQueryError>(),
            Some(AuthorizedQueryError::RevisionInvariantViolation)
        ));
    }
    Ok(())
}

#[tokio::test]
async fn physical_plan_keeps_iceberg_projection_filter_and_limit_pushdown() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(110)),
        "memory://authorized-query-pushdown",
    )
    .await?;
    let tasks = form(111, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 112, 113, "allowed").await?;

    let context = workspace
        .authorized_query_context(policy(&tasks, &[112]))
        .await?;
    let plan = context
        .physical_plan_for_testing("SELECT title FROM tasks WHERE title = 'allowed' LIMIT 1")
        .await?;
    let normalized = plan.to_ascii_lowercase();
    assert!(normalized.contains("iceberg"), "{plan}");
    assert!(normalized.contains("projection"), "{plan}");
    assert!(normalized.contains("filter"), "{plan}");
    assert!(normalized.contains("limit"), "{plan}");
    Ok(())
}

#[tokio::test]
async fn function_variants_and_storage_failures_are_closed_errors() -> anyhow::Result<()> {
    let warehouse = "memory://authorized-query-closed-errors";
    let workspace =
        IcebergWorkspace::memory_for_tests(SpaceId::from(Uuid::from_u128(70)), warehouse).await?;
    let tasks = form(71, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 72, 73, "one").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[72]))
        .await?;

    let error = context
        .execute("SELECT UNNEST([1, 2])")
        .await
        .expect_err("UNNEST must be controlled by the function allowlist");
    assert!(matches!(
        error.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::UnauthorizedQueryFeature { .. })
    ));

    let checkpoint = workspace.capture_checkpoint().await?;
    let mut checkpoint_policy = policy(&tasks, &[72]);
    checkpoint_policy.checkpoint = Some(checkpoint.clone());
    let metadata_path = checkpoint.tables[0]
        .metadata_location
        .strip_prefix("memory:///")
        .or_else(|| {
            checkpoint.tables[0]
                .metadata_location
                .strip_prefix("memory:/")
        })
        .expect("memory metadata location");
    operator_from_uri(warehouse)?.delete(metadata_path).await?;
    let error = match workspace.authorized_query_context(checkpoint_policy).await {
        Ok(_) => anyhow::bail!("missing checkpoint metadata must not create a query context"),
        Err(error) => error,
    };
    assert!(matches!(
        error.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryExecutionFailed { .. })
    ));
    let rendered = error.to_string();
    assert!(!rendered.contains("memory:"));
    assert!(!rendered.contains("metadata"));
    Ok(())
}

#[tokio::test]
async fn context_requires_complete_checkpoint_and_reads_the_requested_snapshot(
) -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(20)),
        "memory://authorized-query-checkpoint",
    )
    .await?;
    let tasks = form(21, "Tasks");
    let secrets = form(26, "Secrets");
    create_form(&workspace, &tasks).await?;
    create_form(&workspace, &secrets).await?;
    append(&workspace, &tasks, 22, 23, "first").await?;
    append(&workspace, &secrets, 27, 28, "secret").await?;
    let checkpoint = workspace.capture_checkpoint().await?;
    assert_eq!(checkpoint.tables.len(), 2, "checkpoint is Space-wide");
    append(&workspace, &tasks, 24, 25, "later").await?;

    let mut snapshot_policy = policy(&tasks, &[22, 24]);
    snapshot_policy.checkpoint = Some(checkpoint.clone());
    let context = workspace.authorized_query_context(snapshot_policy).await?;
    let batches = context.execute("SELECT * FROM tasks").await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1
    );
    assert!(context.execute("SELECT * FROM secrets").await.is_err());

    let mut incomplete = policy(&tasks, &[22]);
    let mut incomplete_checkpoint = checkpoint;
    incomplete_checkpoint.tables.clear();
    incomplete_checkpoint.coordinate_checksum =
        incomplete_checkpoint.computed_coordinate_checksum();
    incomplete.checkpoint = Some(incomplete_checkpoint);
    assert!(workspace
        .authorized_query_context(incomplete)
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
async fn context_enforces_the_row_limit() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(30)),
        "memory://authorized-query-row-limit",
    )
    .await?;
    let tasks = form(31, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 32, 33, "one").await?;
    append(&workspace, &tasks, 34, 35, "two").await?;
    let mut limited = policy(&tasks, &[32, 34]);
    limited.limits.max_rows = 1;
    let context = workspace.authorized_query_context(limited).await?;
    assert!(context.execute("SELECT * FROM tasks").await.is_err());
    Ok(())
}

#[tokio::test]
async fn system_columns_are_queryable_only_when_explicitly_allowlisted() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(40)),
        "memory://authorized-query-system-columns",
    )
    .await?;
    let tasks = form(41, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 42, 43, "one").await?;

    for column in [
        QuerySystemColumn::EntryId,
        QuerySystemColumn::EntryVersion,
        QuerySystemColumn::CommittedAt,
    ] {
        let mut allowed = policy(&tasks, &[42]);
        allowed
            .forms
            .get_mut(&tasks.id)
            .unwrap()
            .system_columns
            .insert(column);
        let context = workspace.authorized_query_context(allowed).await?;
        assert_eq!(
            context
                .execute(&format!("SELECT {} FROM tasks", column.as_str()))
                .await?
                .len(),
            1
        );
    }

    let mut wildcard = policy(&tasks, &[42]);
    wildcard
        .forms
        .get_mut(&tasks.id)
        .unwrap()
        .system_columns
        .insert(QuerySystemColumn::EntryId);
    let context = workspace.authorized_query_context(wildcard).await?;
    let batches = context.execute("SELECT * FROM tasks").await?;
    assert_eq!(batches[0].schema().field(0).name(), "title");
    assert_eq!(
        batches[0].schema().field(1).name(),
        QuerySystemColumn::EntryId.as_str()
    );

    let context = workspace
        .authorized_query_context(policy(&tasks, &[42]))
        .await?;
    assert!(context
        .execute("SELECT _ugoite_entry_id FROM tasks")
        .await
        .is_err());

    let mut colliding = form(44, "Colliding");
    colliding.fields[0].name = "entry_id".into();
    assert!(create_form(&workspace, &colliding).await.is_err());
    Ok(())
}

#[tokio::test]
async fn context_releases_concurrency_permits_after_errors() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(50)),
        "memory://authorized-query-concurrency",
    )
    .await?;
    let tasks = form(51, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 52, 53, "one").await?;
    let context = std::sync::Arc::new(
        workspace
            .authorized_query_context(policy(&tasks, &[52]))
            .await?,
    );
    assert!(context.execute("SELECT count(*) FROM tasks").await.is_err());
    assert!(context.execute("SELECT * FROM tasks").await.is_ok());

    for entry in 54..63 {
        append(&workspace, &tasks, entry, entry + 100, "many").await?;
    }
    let mut busy_policy = policy(&tasks, &(52..63).collect::<Vec<_>>());
    busy_policy.limits.timeout = Duration::from_millis(50);
    busy_policy.limits.allowed_functions.insert("count".into());
    let busy = std::sync::Arc::new(workspace.authorized_query_context(busy_policy).await?);
    let running = busy.clone();
    let task = tokio::spawn(async move {
        running
            .execute(
                "SELECT count(*) FROM tasks a CROSS JOIN tasks b CROSS JOIN tasks c CROSS JOIN tasks d CROSS JOIN tasks e CROSS JOIN tasks f CROSS JOIN tasks g CROSS JOIN tasks h",
            )
            .await
    });
    tokio::task::yield_now().await;
    let second = busy.execute("SELECT * FROM tasks").await;
    let _ = task.await?;
    assert!(
        second.is_err(),
        "the second concurrent query must fail closed"
    );
    Ok(())
}

#[tokio::test]
async fn context_enforces_memory_and_timeout_limits() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(60)),
        "memory://authorized-query-resource-limits",
    )
    .await?;
    let tasks = form(61, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 62, 63, "one").await?;

    let mut memory_limited = policy(&tasks, &[62]);
    memory_limited.limits.max_memory_bytes = 1;
    let context = workspace.authorized_query_context(memory_limited).await?;
    let error = context
        .execute("SELECT * FROM tasks")
        .await
        .expect_err("current-state execution must honor the memory limit");
    assert!(matches!(
        error.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::ResourceLimitExceeded { .. })
    ));

    let mut timed_out = policy(&tasks, &[62]);
    timed_out.limits.timeout = Duration::from_nanos(1);
    let context = workspace.authorized_query_context(timed_out).await?;
    assert!(context.execute("SELECT * FROM tasks").await.is_err());
    Ok(())
}
