use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use arrow_array::{Int64Array, StringArray};
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
    SqlFunctionPolicy,
};
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{FieldType, FormDefinition, FormField, FormVersion};
use ugoite_domain::id::{EntryId, FieldId, FormId, SpaceId};
use ugoite_iceberg::{publication_context, query_context::AuthorizedQueryError, IcebergWorkspace};
use ugoite_storage::{operator_from_uri, SpaceUri};
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

fn aggregate_form(id: u128) -> FormDefinition {
    let field = |id, name: &str, field_type| FormField {
        id: FieldId::new(id).unwrap(),
        name: name.into(),
        field_type,
        required: false,
        label: None,
        description: None,
        semantic_role: None,
        reference_form: None,
        list_item: None,
        validation: None,
        enum_values: Vec::new(),
        deprecated: false,
    };

    FormDefinition {
        id: FormId::from(Uuid::from_u128(id)),
        version: FormVersion::new(1).unwrap(),
        name: "Aggregate observations".into(),
        description: None,
        fields: vec![
            field(100, "category", FieldType::String),
            field(101, "amount", FieldType::Long),
            field(102, "optional amount", FieldType::Double),
            field(103, "empty amount", FieldType::Double),
        ],
        allow_extra_attributes: false,
        extension_metadata: BTreeMap::new(),
    }
}

fn aggregate_revision(
    form: &FormDefinition,
    entry: u128,
    revision: u128,
    category: &str,
    amount: i64,
    optional_amount: FieldValue,
) -> EntryRevision {
    EntryRevision {
        form_id: form.id,
        entry_id: EntryId::from(Uuid::from_u128(entry)),
        revision_id: Uuid::from_u128(revision).into(),
        change_id: format!("aggregate-change-{revision}"),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "test".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            updated_by: "test".into(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([
            (
                FieldId::new(100).unwrap(),
                FieldValue::String(category.into()),
            ),
            (FieldId::new(101).unwrap(), FieldValue::Integer(amount)),
            (FieldId::new(102).unwrap(), optional_amount),
            (FieldId::new(103).unwrap(), FieldValue::Null),
        ]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
}

async fn append_aggregate(
    workspace: &IcebergWorkspace,
    revision: EntryRevision,
) -> anyhow::Result<i64> {
    Ok(workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.aggregate-entry",
            &revision,
        )?)?
        .append_revisions(revision.form_id, vec![revision])
        .await?
        .snapshot_id)
}

async fn append_aggregate_batch(
    workspace: &IcebergWorkspace,
    revisions: Vec<EntryRevision>,
) -> anyhow::Result<i64> {
    let first = revisions
        .first()
        .ok_or_else(|| anyhow::anyhow!("aggregate batch must contain at least one Entry"))?;
    let form_id = first.form_id;
    Ok(workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.aggregate-entry-batch",
            first,
        )?)?
        .append_revisions(form_id, revisions)
        .await?
        .snapshot_id)
}

fn aggregate_policy(form: &FormDefinition, readable: &[u128]) -> AuthorizedQueryPolicy {
    let mut authorized = policy(form, readable);
    let form_policy = authorized.forms.get_mut(&form.id).unwrap();
    form_policy.columns = ["field_100", "field_101", "field_102", "field_103"]
        .into_iter()
        .map(str::to_string)
        .collect();
    authorized.limits.function_policy = SqlFunctionPolicy::ReadOnlyAggregateV1;
    authorized
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

fn revision(form: &FormDefinition, entry: u128, revision: u128, title: &str) -> EntryRevision {
    EntryRevision {
        form_id: form.id,
        entry_id: EntryId::from(Uuid::from_u128(entry)),
        revision_id: Uuid::from_u128(revision).into(),
        change_id: format!("change-{revision}"),
        parent_revision_id: None,
        entry_version: 1,
        expected_version: None,
        operation: EntryOperation::Upsert,
        committed_at_micros: 1,
        author_id: "test".into(),
        form_version: form.version,
        source_kind: "test".into(),
        source_id: None,
        entry: EntryMetadata {
            updated_by: "test".into(),
            ..EntryMetadata::default()
        },
        values: BTreeMap::from([(FieldId::new(100).unwrap(), FieldValue::String(title.into()))]),
        extra_attributes: BTreeMap::new(),
        extension_metadata: BTreeMap::new(),
    }
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

fn policy(form: &FormDefinition, readable: &[u128]) -> AuthorizedQueryPolicy {
    AuthorizedQueryPolicy {
        forms: [(
            form.id,
            AuthorizedQueryForm {
                relation: "tasks".into(),
                sql_aliases: BTreeSet::new(),
                entry_scope: EntryScope::Only(
                    readable
                        .iter()
                        .map(|id| EntryId::from(Uuid::from_u128(*id)))
                        .collect(),
                ),
                columns: ["field_100".into()].into_iter().collect(),
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
            function_policy: SqlFunctionPolicy::DenyAll,
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
        .execute("SELECT * FROM tasks WHERE field_100 IS NOT NULL")
        .await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1
    );

    // DataFusion accepts the statement syntactically, but the authorized
    // execution context must reject DDL without mutating the Space.
    assert!(context.execute("DROP TABLE tasks").await.is_err());
    let batches = context
        .execute("SELECT * FROM tasks WHERE field_100 IS NOT NULL")
        .await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "rejected DDL must not mutate the authorized query source"
    );

    // Syntax lint accepts parser-valid DML, but the authorized execution
    // boundary must reject each statement before it can mutate the Space.
    // #2314: assert the seeded Entry field VALUE plus history and row count,
    // not just the row count, so a silent in-place mutation cannot pass.
    for sql in [
        "INSERT INTO tasks SELECT * FROM tasks",
        "UPDATE tasks SET field_100 = 'changed'",
        "DELETE FROM tasks",
    ] {
        assert!(
            context.execute(sql).await.is_err(),
            "non-read-only DML must be rejected: {sql}"
        );
        let batches = context
            .execute("SELECT * FROM tasks WHERE field_100 IS NOT NULL")
            .await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            1,
            "rejected DML must not mutate the authorized query source: {sql}"
        );
        let value_batches = context.execute("SELECT field_100 FROM tasks").await?;
        let row_count: usize = value_batches.iter().map(|batch| batch.num_rows()).sum();
        assert_eq!(row_count, 1, "rejected DML must preserve row count: {sql}");
        let value = value_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("field_100 returns UTF-8")
            .value(0);
        assert_eq!(
            value, "allowed",
            "rejected DML must preserve the seeded Entry field value: {sql}"
        );
        let revisions = workspace.read_revisions(tasks.id).await?;
        assert_eq!(
            revisions.len(),
            2,
            "rejected DML must preserve Entry history: {sql}"
        );
        let seeded = revisions
            .iter()
            .find(|revision| revision.entry_id == EntryId::from(Uuid::from_u128(10)))
            .expect("seeded Entry revision");
        let field_id = FieldId::new(100).unwrap();
        assert_eq!(
            seeded.values.get(&field_id),
            Some(&FieldValue::String("allowed".to_string())),
            "rejected DML must preserve the seeded revision payload: {sql}"
        );
    }

    for sql in [
        "SELECT entry_id FROM tasks",
        "SELECT t.entry_id FROM tasks AS t",
        "SELECT * FROM secrets",
        "SELECT * FROM information_schema.tables",
        "EXPLAIN SELECT * FROM tasks",
        "SELECT count(*) FROM tasks",
        "SELECT current_schema()",
        "SELECT current_catalog()",
        "SELECT unregistered_udf(field_100) FROM tasks",
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
        "SELECT * FROM tasks t JOIN tasks other ON t.field_100 = other.field_100",
        "SELECT * FROM (SELECT * FROM tasks) nested",
        "SELECT t.field_100 FROM tasks AS t",
    ] {
        let batches = context.execute(sql).await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            1
        );
    }
    assert_eq!(
        context
            .execute("SELECT field_100 AS entry_id FROM tasks")
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
async fn count_aggregates_only_authorized_current_entries() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(4)),
        "memory://authorized-query-count",
    )
    .await?;
    let tasks = form(5, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 6, 7, "allowed").await?;
    append(&workspace, &tasks, 8, 9, "hidden").await?;

    let mut authorized_policy = policy(&tasks, &[6]);
    authorized_policy.limits.function_policy = SqlFunctionPolicy::new(&[], &["count"], &[]);
    let context = workspace
        .authorized_query_context(authorized_policy)
        .await?;
    let batches = context
        .execute("SELECT COUNT(*) AS visible_count FROM tasks")
        .await?;
    let count = batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("COUNT(*) returns an Int64")
        .value(0);
    assert_eq!(count, 1, "hidden Entries must not contribute to COUNT(*)");
    Ok(())
}

#[tokio::test]
async fn sql_aggregates_preserve_nulls_grouping_authorization_and_type_contracts(
) -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(200)),
        "memory://authorized-query-aggregate-spike",
    )
    .await?;
    let tasks = aggregate_form(201);
    create_form(&workspace, &tasks).await?;
    for (entry, revision_id, category, amount, optional_amount) in [
        (206, 207, "food", 100, FieldValue::Number(1.5)),
        (208, 209, "food", 200, FieldValue::Null),
        (210, 211, "travel", -50, FieldValue::Number(-2.0)),
        (214, 215, "zero", 0, FieldValue::Null),
        (212, 213, "private", 10_000, FieldValue::Number(99.0)),
    ] {
        append_aggregate(
            &workspace,
            aggregate_revision(
                &tasks,
                entry,
                revision_id,
                category,
                amount,
                optional_amount,
            ),
        )
        .await?;
    }

    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[206, 208, 210, 214]))
        .await?;
    let batches = context
        .execute(
            "SELECT COUNT(*) AS row_count, COUNT(field_102) AS optional_count, \
             COUNT(field_101, field_102) AS pair_count, \
             SUM(field_101) AS total, AVG(field_101) AS average, \
             MIN(field_101) AS minimum, MAX(field_101) AS maximum, \
             COUNT(field_103) AS empty_count, SUM(field_103) AS empty_sum, \
             AVG(field_103) AS empty_average, MIN(field_103) AS empty_minimum, \
             MAX(field_103) AS empty_maximum, SUM(field_102) AS float_total, \
             AVG(field_102) AS float_average, MIN(field_102) AS float_minimum, \
             MAX(field_102) AS float_maximum FROM tasks",
        )
        .await?;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);
    let batch = &batches[0];
    let int = |index| {
        batch
            .column(index)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("COUNT and integer SUM/MIN/MAX use Int64 in this fixture")
            .value(0)
    };
    assert_eq!(int(0), 4, "the denied Entry must not affect COUNT(*)");
    assert_eq!(int(1), 2, "COUNT(field) ignores NULL values");
    assert_eq!(int(2), 2, "COUNT(a, b) ignores rows with any NULL input");
    assert_eq!(int(3), 250);
    assert_eq!(int(5), -50);
    assert_eq!(int(6), 200);
    assert_eq!(
        batch.schema().field(3).data_type(),
        &arrow_schema::DataType::Int64
    );
    assert_eq!(
        batch.schema().field(4).data_type(),
        &arrow_schema::DataType::Float64
    );
    let pair_count = context
        .execute(
            "SELECT COUNT(field_101, field_102) AS pair_count FROM tasks \
             WHERE field_100 = 'food'",
        )
        .await?;
    assert_eq!(
        pair_count[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("multi-argument COUNT returns Int64")
            .value(0),
        1,
        "COUNT(a, b) requires every argument to be non-NULL"
    );
    let average = batch
        .column(4)
        .as_any()
        .downcast_ref::<arrow_array::Float64Array>()
        .expect("AVG of Long returns Float64")
        .value(0);
    assert!((average - 62.5).abs() < 1e-12);
    let integer_average_precision_boundary = context
        .execute(
            "SELECT AVG(CASE WHEN field_101 >= 100 THEN \
             CAST(9007199254740992 AS BIGINT) ELSE \
             CAST(9007199254740993 AS BIGINT) END) AS average FROM tasks",
        )
        .await?;
    assert_eq!(
        integer_average_precision_boundary[0]
            .schema()
            .field(0)
            .data_type(),
        &arrow_schema::DataType::Float64,
        "AVG over integer values has an explicit Float64 result type"
    );
    assert_eq!(
        integer_average_precision_boundary[0]
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Float64Array>()
            .expect("integer AVG returns Float64")
            .value(0),
        9_007_199_254_740_992.0,
        "the exact midpoint at 2^53 rounds to the even Float64 value"
    );
    let mean = context
        .execute("SELECT MEAN(field_101) AS mean FROM tasks")
        .await?;
    let mean = mean[0]
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::Float64Array>()
        .expect("MEAN retains DataFusion's AVG alias")
        .value(0);
    assert!((mean - average).abs() < 1e-12);
    assert_eq!(int(7), 0);
    for index in 8..=11 {
        assert!(
            batch.column(index).is_null(0),
            "column {index} must remain NULL"
        );
    }
    let decimal = context
        .execute("SELECT SUM(CAST(field_101 AS DECIMAL(10, 2))) AS total FROM tasks")
        .await?;
    assert_eq!(
        decimal[0].schema().field(0).data_type(),
        &arrow_schema::DataType::Decimal128(20, 2)
    );
    assert_eq!(
        decimal[0]
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Decimal128Array>()
            .expect("Decimal SUM stays Decimal128")
            .value(0),
        25_000
    );
    let decimal_average = context
        .execute("SELECT AVG(CAST(field_101 AS DECIMAL(10, 2))) AS average FROM tasks")
        .await?;
    assert_eq!(
        decimal_average[0].schema().field(0).data_type(),
        &arrow_schema::DataType::Decimal128(14, 6)
    );
    assert_eq!(
        decimal_average[0]
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Decimal128Array>()
            .expect("decimal AVG stays Decimal128")
            .value(0),
        62_500_000,
        "decimal AVG preserves exact base-10 output scale"
    );
    let float_value = |index| {
        batch
            .column(index)
            .as_any()
            .downcast_ref::<arrow_array::Float64Array>()
            .expect("Double aggregate returns Float64")
            .value(0)
    };
    assert_eq!(float_value(12), -0.5);
    assert_eq!(float_value(13), -0.25);
    assert_eq!(float_value(14), -2.0);
    assert_eq!(float_value(15), 1.5);
    let window_aggregate = context
        .execute(
            "SELECT SUM(field_101) OVER (ORDER BY field_100 \
             ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) AS rolling_total FROM tasks",
        )
        .await
        .expect_err("windowed aggregates are outside the read-only aggregate policy");
    assert!(matches!(
        window_aggregate.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::UnauthorizedQueryFeature { .. })
    ));
    let special_float_sum = context
        .execute(
            "SELECT SUM(CAST('NaN' AS DOUBLE)) AS nan_total, \
             SUM(CAST('Infinity' AS DOUBLE)) AS infinity_total FROM tasks",
        )
        .await
        .expect_err("SUM must reject non-finite inputs");
    assert!(matches!(
        special_float_sum.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryExecutionFailed { .. })
    ));
    let non_finite_min_max_sql = "SELECT MIN(CAST('NaN' AS DOUBLE)) AS nan_min, \
         MAX(CAST('Infinity' AS DOUBLE)) AS infinity_max FROM tasks";
    let non_finite_min_max = context
        .execute(non_finite_min_max_sql)
        .await
        .expect_err("MIN/MAX must reject non-finite aggregate results");
    assert!(matches!(
        non_finite_min_max.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryExecutionFailed { .. })
    ));
    let non_finite_page = match context
        .execute_stateless_page(non_finite_min_max_sql, HashMap::new(), 0, 10)
        .await
    {
        Err(error) => error,
        Ok(_) => panic!("stateless pages must reject non-finite aggregate results"),
    };
    assert!(matches!(
        non_finite_page.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryExecutionFailed { .. })
    ));
    let decimal_256 = context
        .execute("SELECT SUM(CAST(field_101 AS DECIMAL(40, 2))) AS total FROM tasks")
        .await?;
    assert_eq!(
        decimal_256[0].schema().field(0).data_type(),
        &arrow_schema::DataType::Decimal256(50, 2)
    );
    assert_eq!(
        decimal_256[0]
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Decimal256Array>()
            .expect("Decimal256 SUM stays Decimal256")
            .value(0)
            .to_string(),
        "25000"
    );
    let decimal_overflow = context
        .execute(
            "SELECT SUM(CAST('99999999999999999999999999999999999999' \
             AS DECIMAL(38, 0))) AS total FROM tasks",
        )
        .await
        .expect_err("Decimal128 SUM must fail instead of wrapping its result");
    assert!(matches!(
        decimal_overflow.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryExecutionFailed { .. })
    ));
    let precision_boundary = format!("9{}", "0".repeat(75));
    let average_output_overflow = context
        .execute(&format!(
            "SELECT AVG(CAST('{precision_boundary}' AS DECIMAL(76, 0))) AS average FROM tasks"
        ))
        .await
        .expect_err("Decimal256 AVG must fail when its scaled result exceeds output precision");
    assert!(matches!(
        average_output_overflow.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryExecutionFailed { .. })
    ));
    let distinct = context
        .execute(
            "SELECT COUNT(DISTINCT CASE WHEN field_101 = 200 THEN 100 \
             ELSE field_101 END) AS distinct_count, \
             SUM(DISTINCT CASE WHEN field_101 = 200 THEN 100 \
             ELSE field_101 END) AS total, \
             AVG(DISTINCT CASE WHEN field_101 = 200 THEN 100 \
             ELSE field_101 END) AS average FROM tasks",
        )
        .await?;
    assert_eq!(
        distinct[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("distinct integer count remains Int64")
            .value(0),
        3
    );
    assert_eq!(
        distinct[0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("distinct integer sum remains Int64")
            .value(0),
        50
    );
    let distinct_average = distinct[0]
        .column(2)
        .as_any()
        .downcast_ref::<arrow_array::Float64Array>()
        .expect("distinct integer average returns Float64")
        .value(0);
    assert!((distinct_average - (50.0 / 3.0)).abs() < 1e-12);

    let grouped = context
        .execute(
            "SELECT field_100, COUNT(*) AS item_count, SUM(field_101) AS total \
             FROM tasks GROUP BY field_100 HAVING COUNT(*) > 0 ORDER BY total DESC",
        )
        .await?;
    assert_eq!(
        grouped.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        3
    );
    let group_batch = &grouped[0];
    let categories = group_batch
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("category is a string");
    let totals = group_batch
        .column(2)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("grouped integer SUM uses Int64");
    assert_eq!(categories.value(0), "food");
    assert_eq!(totals.value(0), 300);
    assert_eq!(categories.value(1), "zero");
    assert_eq!(totals.value(1), 0);
    assert_eq!(categories.value(2), "travel");
    assert_eq!(totals.value(2), -50);

    let plan = context
        .physical_plan_for_testing(
            "SELECT field_100, SUM(field_101) AS total FROM tasks GROUP BY field_100",
        )
        .await?;
    let normalized_plan = plan.to_ascii_lowercase();
    let aggregate = normalized_plan
        .find("aggregateexec")
        .expect("physical plan contains a DataFusion aggregate node");
    let scan = normalized_plan
        .find("icebergtablescan")
        .expect("physical plan scans the authorized Iceberg provider");
    assert!(
        aggregate < scan,
        "authorization source must be below aggregate: {plan}"
    );
    assert!(
        normalized_plan.contains("entry_id"),
        "scope filter must be in the plan: {plan}"
    );
    let scope_filter = normalized_plan
        .find("filterexec")
        .expect("EntryScope is enforced by the authorized relation before aggregation");
    assert!(aggregate < scope_filter && scope_filter < scan, "{plan}");

    let empty_context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[]))
        .await?;
    let empty = empty_context
        .execute(
            "SELECT COUNT(*) AS row_count, COUNT(field_101) AS value_count, \
             SUM(field_101) AS total, \
             AVG(field_101) AS average, MIN(field_101) AS minimum, \
             MAX(field_101) AS maximum FROM tasks",
        )
        .await?;
    assert_eq!(empty[0].num_rows(), 1);
    assert_eq!(
        empty[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        0
    );
    assert_eq!(
        empty[0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        0
    );
    for index in 2..=5 {
        assert!(
            empty[0].column(index).is_null(0),
            "empty aggregate {index} must be NULL"
        );
    }
    Ok(())
}

#[tokio::test]
async fn sql_aggregate_rejects_int64_overflow_and_preserves_resource_limits() -> anyhow::Result<()>
{
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(220)),
        "memory://authorized-query-aggregate-limits",
    )
    .await?;
    let tasks = aggregate_form(221);
    create_form(&workspace, &tasks).await?;
    for (entry, revision_id, amount) in [(226, 227, i64::MAX), (228, 229, 1), (230, 231, 100)] {
        append_aggregate(
            &workspace,
            aggregate_revision(
                &tasks,
                entry,
                revision_id,
                "values",
                amount,
                FieldValue::Null,
            ),
        )
        .await?;
    }

    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[226, 228, 230]))
        .await?;
    let overflow = context
        .execute("SELECT SUM(field_101) AS total FROM tasks")
        .await
        .expect_err("SUM must not wrap when the Int64 result range is exceeded");
    assert!(matches!(
        overflow.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryExecutionFailed { .. })
    ));
    let unsigned_overflow = context
        .execute("SELECT SUM(18446744073709551615) AS total FROM tasks")
        .await
        .expect_err("UInt64 SUM must fail instead of wrapping its result");
    assert!(
        matches!(
            unsigned_overflow.downcast_ref::<AuthorizedQueryError>(),
            Some(AuthorizedQueryError::QueryExecutionFailed { .. })
        ),
        "unexpected UInt64 SUM failure: {unsigned_overflow:#}"
    );

    let mut memory_limited = aggregate_policy(&tasks, &[226, 228, 230]);
    memory_limited.limits.max_memory_bytes = 1;
    let memory_context = workspace.authorized_query_context(memory_limited).await?;
    let error = memory_context
        .execute("SELECT field_100, SUM(field_101) AS total FROM tasks GROUP BY field_100")
        .await
        .expect_err("grouped aggregation must fail when the DataFusion memory budget is exceeded");
    assert!(matches!(
        error.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::ResourceLimitExceeded { .. })
    ));

    let mut timed_out = aggregate_policy(&tasks, &[226, 228, 230]);
    timed_out.limits.timeout = Duration::from_nanos(1);
    let timeout_context = workspace.authorized_query_context(timed_out).await?;
    let error = timeout_context
        .execute("SELECT SUM(field_101) AS total FROM tasks")
        .await
        .expect_err("timed out aggregate must not return a partial value");
    assert!(matches!(
        error.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::QueryTimedOut)
    ));
    Ok(())
}

#[tokio::test]
async fn high_cardinality_grouping_and_distinct_aggregates_are_bounded() -> anyhow::Result<()> {
    const ENTRY_COUNT: usize = 512;
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(240)),
        "memory://authorized-query-aggregate-cardinality",
    )
    .await?;
    let tasks = aggregate_form(241);
    create_form(&workspace, &tasks).await?;
    let entry_ids = (0..ENTRY_COUNT)
        .map(|index| 10_000 + index as u128)
        .collect::<Vec<_>>();
    let revisions = entry_ids
        .iter()
        .enumerate()
        .map(|(index, entry_id)| {
            aggregate_revision(
                &tasks,
                *entry_id,
                20_000 + index as u128,
                &format!("category-{index:04}"),
                (index % 256) as i64,
                FieldValue::Null,
            )
        })
        .collect::<Vec<_>>();
    append_aggregate_batch(&workspace, revisions).await?;

    let mut parallel = aggregate_policy(&tasks, &entry_ids);
    parallel.limits.max_rows = ENTRY_COUNT + 1;
    parallel.limits.max_concurrency = 4;
    parallel.limits.timeout = Duration::from_secs(30);
    let parallel_context = workspace.authorized_query_context(parallel.clone()).await?;
    let distinct_sql = "SELECT COUNT(DISTINCT amount) AS distinct_count, \
         SUM(DISTINCT amount) AS total, AVG(DISTINCT amount) AS average \
         FROM (SELECT field_101 AS amount FROM tasks \
         UNION ALL SELECT field_101 AS amount FROM tasks) AS duplicate_inputs";
    let distinct_plan = parallel_context
        .physical_plan_for_testing(distinct_sql)
        .await?;
    assert!(
        distinct_plan.matches("AggregateExec").count() >= 2,
        "the distinct aggregate should use partial and final aggregate stages: {distinct_plan}"
    );
    assert!(
        distinct_plan.contains("UnionExec"),
        "the aggregate inputs should reach distinct states through multiple partitions: {distinct_plan}"
    );
    let distinct = parallel_context.execute(distinct_sql).await?;
    assert_eq!(
        distinct[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("distinct count stays Int64")
            .value(0),
        256
    );
    assert_eq!(
        distinct[0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("distinct SUM stays Int64")
            .value(0),
        (0..256).sum::<i64>()
    );
    let average = distinct[0]
        .column(2)
        .as_any()
        .downcast_ref::<arrow_array::Float64Array>()
        .expect("distinct AVG returns Float64")
        .value(0);
    assert!((average - 127.5).abs() < 1e-12);

    let mut group_memory_limited = aggregate_policy(&tasks, &entry_ids);
    group_memory_limited.limits.max_rows = ENTRY_COUNT + 1;
    group_memory_limited.limits.max_memory_bytes = 8 * 1024;
    group_memory_limited.limits.timeout = Duration::from_secs(30);
    let group_context = workspace
        .authorized_query_context(group_memory_limited)
        .await?;
    let groups = group_context
        .execute("SELECT field_100, COUNT(*) FROM tasks GROUP BY field_100")
        .await
        .expect_err("512 output groups must stay within the configured memory budget");
    assert!(matches!(
        groups.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::ResourceLimitExceeded { .. })
    ));

    let mut distinct_memory_limited = aggregate_policy(&tasks, &entry_ids);
    distinct_memory_limited.limits.max_rows = ENTRY_COUNT + 1;
    distinct_memory_limited.limits.max_memory_bytes = 8 * 1024;
    distinct_memory_limited.limits.timeout = Duration::from_secs(30);
    let distinct_context = workspace
        .authorized_query_context(distinct_memory_limited)
        .await?;
    let distinct = distinct_context
        .execute("SELECT SUM(DISTINCT field_101) FROM tasks")
        .await
        .expect_err("a large DISTINCT state must stay within the configured memory budget");
    assert!(matches!(
        distinct.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::ResourceLimitExceeded { .. })
    ));

    let mut distinct_count_memory_limited = aggregate_policy(&tasks, &entry_ids);
    distinct_count_memory_limited.limits.max_rows = ENTRY_COUNT + 1;
    distinct_count_memory_limited.limits.max_memory_bytes = 8 * 1024;
    distinct_count_memory_limited.limits.timeout = Duration::from_secs(30);
    let distinct_count_context = workspace
        .authorized_query_context(distinct_count_memory_limited)
        .await?;
    let distinct_count = distinct_count_context
        .execute("SELECT COUNT(DISTINCT field_101) FROM tasks")
        .await
        .expect_err("a high-cardinality COUNT(DISTINCT) state must stay within the memory budget");
    assert!(matches!(
        distinct_count.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::ResourceLimitExceeded { .. })
    ));

    let mut row_limited = aggregate_policy(&tasks, &entry_ids);
    row_limited.limits.max_rows = 32;
    row_limited.limits.max_memory_bytes = 8 * 1024 * 1024;
    row_limited.limits.timeout = Duration::from_secs(30);
    let row_context = workspace.authorized_query_context(row_limited).await?;
    let rows = row_context
        .execute("SELECT field_100, COUNT(*) FROM tasks GROUP BY field_100")
        .await
        .expect_err("an oversized aggregate result must not be returned as a prefix");
    assert!(matches!(
        rows.downcast_ref::<AuthorizedQueryError>(),
        Some(AuthorizedQueryError::ResourceLimitExceeded { .. })
    ));
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
            "SELECT field_100 FROM tasks WHERE field_100 = $title",
            HashMap::from([(
                "title".to_string(),
                datafusion::scalar::ScalarValue::Utf8(Some("allowed".into())),
            )]),
        )
        .await?;
    assert_eq!(rows.iter().map(|batch| batch.num_rows()).sum::<usize>(), 1);

    let injection = context
        .execute_with_parameters(
            "SELECT field_100 FROM tasks WHERE field_100 = $title",
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
            "SELECT field_100 FROM tasks WHERE field_100 = $title",
            HashMap::new()
        )
        .await
        .is_err());
    assert!(context
        .execute_with_parameters(
            "SELECT field_100 FROM tasks",
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
        .physical_plan_for_testing(
            "SELECT field_100 FROM tasks WHERE field_100 = 'allowed' LIMIT 1",
        )
        .await?;
    let normalized = plan.to_ascii_lowercase();
    assert!(normalized.contains("iceberg"), "{plan}");
    assert!(normalized.contains("projection"), "{plan}");
    assert!(normalized.contains("filter"), "{plan}");
    assert!(normalized.contains("limit"), "{plan}");
    let scan = normalized
        .find("icebergtablescan")
        .map(|index| &normalized[index..])
        .expect("the physical plan must contain the upstream Iceberg scan");
    let predicates = scan
        .split_once("predicates: some(")
        .and_then(|(_, rest)| rest.split_once(", limit:"))
        .map(|(predicate, _)| predicate)
        .expect("the upstream scan must expose its pushed predicate");
    assert!(predicates.contains("operation"), "{plan}");
    assert!(
        !predicates.contains("title"),
        "payload predicates must remain above latest-state selection: {plan}"
    );
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

    let mut no_from_aggregate_policy = policy(&tasks, &[]);
    no_from_aggregate_policy.limits.function_policy = SqlFunctionPolicy::ReadOnlyAggregateV1;
    let aggregate_context = workspace
        .authorized_query_context(no_from_aggregate_policy)
        .await?;
    let scalar_count = aggregate_context
        .execute("SELECT COUNT(*) AS row_count")
        .await?;
    assert_eq!(scalar_count[0].num_rows(), 1);
    assert_eq!(
        scalar_count[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("COUNT returns Int64")
            .value(0),
        1,
        "the synthetic scalar input has one row"
    );

    let checkpoint = workspace.capture_checkpoint().await?;
    let mut checkpoint_policy = policy(&tasks, &[72]);
    checkpoint_policy.checkpoint = Some(checkpoint.clone());
    let metadata_uri = SpaceUri::parse(&checkpoint.tables[0].metadata_location)?;
    let metadata_path = format!("test/space_{:032x}/{}", 70_u128, metadata_uri.key());
    operator_from_uri(warehouse)?.delete(&metadata_path).await?;
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
    assert_eq!(batches[0].schema().field(0).name(), "field_100");
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
    busy_policy.limits.function_policy = SqlFunctionPolicy::new(&[], &["count"], &[]);
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
