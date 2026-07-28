use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, QueryCheckpoint, QueryLimits, QuerySystemColumn,
};
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{FieldType, FormDefinition, FormField, FormVersion};
use ugoite_domain::id::{EntryId, FieldId, FormId, SpaceId};
use ugoite_iceberg::{publication_context, IcebergWorkspace};
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
    revision: u128,
    title: &str,
) -> anyhow::Result<i64> {
    let mut values = BTreeMap::new();
    values.insert(FieldId::new(100).unwrap(), FieldValue::String(title.into()));
    let revision = EntryRevision {
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
    };
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

fn policy(form: &FormDefinition, readable: &[u128]) -> AuthorizedQueryPolicy {
    AuthorizedQueryPolicy {
        forms: [(
            form.id,
            AuthorizedQueryForm {
                relation: "tasks".into(),
                columns: ["title".into()].into_iter().collect(),
                system_columns: BTreeSet::new(),
            },
        )]
        .into_iter()
        .collect(),
        readable_entry_ids: readable
            .iter()
            .map(|id| EntryId::from(Uuid::from_u128(*id)))
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
        "SELECT * FROM secrets",
        "SELECT * FROM information_schema.tables",
        "EXPLAIN SELECT * FROM tasks",
        "SELECT count(*) FROM tasks",
    ] {
        assert!(
            context.execute(sql).await.is_err(),
            "{sql} must fail closed"
        );
    }
    for sql in [
        "SELECT * FROM tasks t JOIN tasks other ON t.title = other.title",
        "SELECT * FROM (SELECT * FROM tasks) nested",
    ] {
        let batches = context.execute(sql).await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            1
        );
    }
    let empty = workspace
        .authorized_query_context(policy(&tasks, &[]))
        .await?;
    assert!(empty.execute("SELECT * FROM tasks").await?.is_empty());
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
    create_form(&workspace, &tasks).await?;
    let first_snapshot = append(&workspace, &tasks, 22, 23, "first").await?;
    append(&workspace, &tasks, 24, 25, "later").await?;

    let mut snapshot_policy = policy(&tasks, &[22, 24]);
    snapshot_policy.checkpoint = Some(QueryCheckpoint {
        form_snapshots: [(tasks.id, first_snapshot)].into_iter().collect(),
    });
    let context = workspace.authorized_query_context(snapshot_policy).await?;
    let batches = context.execute("SELECT * FROM tasks").await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1
    );

    let mut incomplete = policy(&tasks, &[22]);
    incomplete.checkpoint = Some(QueryCheckpoint {
        form_snapshots: BTreeMap::new(),
    });
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

#[test]
fn system_column_allowlist_is_explicit() {
    assert_eq!(QuerySystemColumn::EntryId.as_str(), "entry_id");
}
