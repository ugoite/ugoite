use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use arrow_array::{BooleanArray, Float64Array, Int64Array, ListArray, StringArray};
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
        "SELECT * FROM tasks AS \"_ugoite_left\" INNER JOIN tasks AS \"_ugoite_right\" ON \"_ugoite_left\".\"field_100\" = \"_ugoite_right\".\"field_100\"",
        "SELECT * FROM (SELECT * FROM tasks) nested",
        "SELECT t.field_100 FROM tasks AS t",
    ] {
        let batches = context.execute(sql).await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            1
        );
    }
    let joined_filter_order = context
        .execute(
            "SELECT * FROM tasks AS left_rows INNER JOIN tasks AS right_rows ON left_rows.field_100 = right_rows.field_100 WHERE right_rows.field_100 IS NOT NULL ORDER BY left_rows.field_100 ASC LIMIT 8",
        )
        .await?;
    assert_eq!(
        joined_filter_order
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "the planner admits a joined query with qualified WHERE and ORDER BY"
    );
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
    // This denied Entry shares the visible "food" group. Its values must not
    // influence that group's count or aggregates.
    append_aggregate(
        &workspace,
        aggregate_revision(&tasks, 216, 217, "food", 10_000, FieldValue::Number(99.0)),
    )
    .await?;

    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[206, 208, 210, 214]))
        .await?;
    let aggregate_source = "SELECT field_101 + 0 FROM tasks";
    let aggregate_document = ugoite_domain::sql_document::parse_sql_document(aggregate_source);
    let amount = aggregate_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::BinaryExpression
                && node
                    .range
                    .and_then(|range| aggregate_document.source_slice(range))
                    == Some("field_101 + 0")
        })
        .expect("the SQL document exposes the composed numeric expression");
    let wrapped_aggregate =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            aggregate_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: aggregate_document.contract_version,
                expected_source: aggregate_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::WrapExpressionInAggregate {
                        node_id: amount.id,
                        expected_kind: amount.kind,
                        function:
                            ugoite_domain::sql_document::SqlDocumentAggregateFunction::Sum,
                        distinct: true,
                    },
                ],
            },
        )?;
    assert_eq!(
        wrapped_aggregate.document.to_sql(),
        "SELECT SUM(DISTINCT field_101 + 0) FROM tasks"
    );
    let wrapped_aggregate_batches = context.execute(wrapped_aggregate.document.to_sql()).await?;
    assert_eq!(wrapped_aggregate_batches.len(), 1);
    assert_eq!(wrapped_aggregate_batches[0].num_rows(), 1);
    assert_eq!(
        wrapped_aggregate_batches[0].schema().field(0).data_type(),
        &arrow_schema::DataType::Int64,
        "SUM(DISTINCT expression) retains the Int64 result schema"
    );
    assert_eq!(
        wrapped_aggregate_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("the authorized SUM(DISTINCT expression) result is Int64")
            .value(0),
        250,
        "the aggregate observes only distinct, authorized expression results"
    );
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
    let distinct_wildcard = context
        .execute("SELECT COUNT(DISTINCT *) FROM tasks")
        .await?;
    assert_eq!(distinct_wildcard[0].num_rows(), 1);
    assert_eq!(
        distinct_wildcard[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("COUNT(DISTINCT *) returns Int64")
            .value(0),
        1,
        "COUNT(DISTINCT *) counts the one distinct wildcard row value admitted by DataFusion"
    );
    let nullary_count = context.execute("SELECT COUNT() FROM tasks").await?;
    assert_eq!(
        nullary_count[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("COUNT() returns Int64")
            .value(0),
        4,
        "COUNT() counts every authorized input row"
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
             CAST(9007199254740994 AS BIGINT) END) AS average FROM tasks",
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
        "the exact midpoint at 2^53 + 1 rounds to the even Float64 value"
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
            "SELECT field_100, COUNT(*) AS item_count, SUM(field_101) AS total, \
             AVG(field_101) AS average, MIN(field_101) AS minimum, \
             MAX(field_101) AS maximum FROM tasks GROUP BY field_100 \
             HAVING COUNT(*) > 0 ORDER BY total DESC",
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
    let counts = group_batch
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("grouped COUNT uses Int64");
    let totals = group_batch
        .column(2)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("grouped integer SUM uses Int64");
    let averages = group_batch
        .column(3)
        .as_any()
        .downcast_ref::<arrow_array::Float64Array>()
        .expect("grouped integer AVG uses Float64");
    let minimums = group_batch
        .column(4)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("grouped integer MIN uses Int64");
    let maximums = group_batch
        .column(5)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("grouped integer MAX uses Int64");
    assert_eq!(categories.value(0), "food");
    assert_eq!(counts.value(0), 2);
    assert_eq!(totals.value(0), 300);
    assert_eq!(averages.value(0), 150.0);
    assert_eq!(minimums.value(0), 100);
    assert_eq!(maximums.value(0), 200);
    assert_eq!(categories.value(1), "zero");
    assert_eq!(counts.value(1), 1);
    assert_eq!(totals.value(1), 0);
    assert_eq!(averages.value(1), 0.0);
    assert_eq!(minimums.value(1), 0);
    assert_eq!(maximums.value(1), 0);
    assert_eq!(categories.value(2), "travel");
    assert_eq!(counts.value(2), 1);
    assert_eq!(totals.value(2), -50);
    assert_eq!(averages.value(2), -50.0);
    assert_eq!(minimums.value(2), -50);
    assert_eq!(maximums.value(2), -50);

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
    let permitted_groups = parallel_context
        .execute("SELECT field_100, COUNT(*) FROM tasks GROUP BY field_100")
        .await?;
    assert_eq!(
        permitted_groups
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        ENTRY_COUNT,
        "the same 512-group query succeeds within the configured memory budget"
    );
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

    let mut group_memory_limited = parallel.clone();
    group_memory_limited.limits.max_memory_bytes = 8 * 1024;
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

    let mut distinct_memory_limited = parallel.clone();
    distinct_memory_limited.limits.max_memory_bytes = 8 * 1024;
    let distinct_context = workspace
        .authorized_query_context(distinct_memory_limited)
        .await?;
    for (sql, aggregate) in [
        ("SELECT COUNT(DISTINCT field_101) FROM tasks", "COUNT"),
        ("SELECT SUM(DISTINCT field_101) FROM tasks", "SUM"),
        ("SELECT AVG(DISTINCT field_101) FROM tasks", "AVG"),
    ] {
        let error = distinct_context
            .execute(sql)
            .await
            .expect_err("a large DISTINCT state must stay within the configured memory budget");
        assert!(
            matches!(
                error.downcast_ref::<AuthorizedQueryError>(),
                Some(AuthorizedQueryError::ResourceLimitExceeded { .. })
            ),
            "{aggregate} DISTINCT should fail under the configured memory limit: {error:#}"
        );
    }

    let mut row_limited = parallel.clone();
    row_limited.limits.max_rows = 32;
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
async fn typed_set_operation_operand_swap_reaches_authorized_execution() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(100)),
        "memory://authorized-query-set-operation-swap",
    )
    .await?;
    let tasks = form(101, "Tasks");
    create_form(&workspace, &tasks).await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[]))
        .await?;

    let source = "SELECT 1 UNION ALL SELECT 2";
    let document = ugoite_domain::sql_document::parse_sql_document(source);
    let operation = document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SetOperation)
        .expect("the parser exposes the set operation node");
    let edited = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::SwapSetOperationOperands {
                    node_id: operation.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SetOperation,
                },
            ],
        },
    )?;
    assert_eq!(edited.document.to_sql(), "SELECT 2 UNION ALL SELECT 1");

    let rows = context.execute(edited.document.to_sql()).await?;
    assert_eq!(rows.iter().map(|batch| batch.num_rows()).sum::<usize>(), 2);
    let mut values = rows
        .iter()
        .flat_map(|batch| {
            batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("the SELECT literals are Int64")
                .values()
                .iter()
                .copied()
        })
        .collect::<Vec<_>>();
    values.sort_unstable();
    assert_eq!(values, [1, 2]);
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
async fn typed_sql_document_edits_execute_through_authorized_query_context() -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(35)),
        "memory://authorized-query-schema-only",
    )
    .await?;
    let tasks = form(36, "Tasks");
    let related = form(43, "Related tasks");
    create_form(&workspace, &tasks).await?;
    create_form(&workspace, &related).await?;
    append(&workspace, &tasks, 37, 38, "not-a-number").await?;
    append(&workspace, &tasks, 39, 40, "alpha").await?;
    append(&workspace, &related, 44, 45, "not-a-number").await?;
    let mut null_title = revision(&tasks, 41, 42, "placeholder");
    null_title
        .values
        .insert(FieldId::new(100).unwrap(), FieldValue::Null);
    workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.entry",
            &null_title,
        )?)?
        .append_revisions(tasks.id, vec![null_title])
        .await?;
    let mut authorized_policy = policy(&tasks, &[37]);
    authorized_policy.forms.insert(
        related.id,
        AuthorizedQueryForm {
            relation: "related_tasks".to_owned(),
            sql_aliases: BTreeSet::new(),
            entry_scope: EntryScope::Only(
                [EntryId::from(Uuid::from_u128(44))].into_iter().collect(),
            ),
            columns: ["field_100".to_owned()].into_iter().collect(),
            system_columns: BTreeSet::new(),
        },
    );
    let context = workspace
        .authorized_query_context(authorized_policy)
        .await?;
    let join_source = "SELECT left_rows.field_100 FROM tasks AS left_rows";
    let join_document = ugoite_domain::sql_document::parse_sql_document(join_source);
    let relation_tree = join_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::RelationTree)
        .expect("the typed SQL document exposes the FROM relation tree");
    let added_join = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        join_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: join_document.contract_version,
            expected_source: join_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddJoin {
                    node_id: relation_tree.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::RelationTree,
                    join: ugoite_domain::sql_document::SqlDocumentJoin {
                        operator: ugoite_domain::sql_document::SqlDocumentJoinOperator::Inner,
                        relation: ugoite_domain::sql_document::SqlDocumentRelationFactor::Table {
                            name: vec![ugoite_domain::sql_document::SqlDocumentIdentifier {
                                value: "related_tasks".to_owned(),
                                quote_style: None,
                            }],
                            alias: Some(ugoite_domain::sql_document::SqlDocumentIdentifier {
                                value: "right_rows".to_owned(),
                                quote_style: None,
                            }),
                        },
                        constraint: ugoite_domain::sql_document::SqlDocumentJoinConstraint::Using {
                            columns: vec![ugoite_domain::sql_document::SqlDocumentIdentifier {
                                value: "field_100".to_owned(),
                                quote_style: None,
                            }],
                        },
                    },
                },
            ],
        },
    )?;
    assert_eq!(
        added_join.document.to_sql(),
        "SELECT left_rows.field_100 FROM tasks AS left_rows INNER JOIN related_tasks AS right_rows USING (field_100)"
    );
    let joined = context.execute(added_join.document.to_sql()).await?;
    assert_eq!(
        joined.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "the typed multi-Form JOIN executes only within both authorized Form scopes"
    );
    let unauthorized_join_source = "SELECT left_rows.field_100 FROM tasks AS left_rows";
    let unauthorized_join_document =
        ugoite_domain::sql_document::parse_sql_document(unauthorized_join_source);
    let unauthorized_relation_tree = unauthorized_join_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::RelationTree)
        .expect("the typed SQL document exposes the source relation tree");
    let unauthorized_join =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            unauthorized_join_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: unauthorized_join_document.contract_version,
                expected_source: unauthorized_join_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddJoin {
                        node_id: unauthorized_relation_tree.id,
                        expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::RelationTree,
                        join: ugoite_domain::sql_document::SqlDocumentJoin {
                            operator: ugoite_domain::sql_document::SqlDocumentJoinOperator::Inner,
                            relation:
                                ugoite_domain::sql_document::SqlDocumentRelationFactor::Table {
                                    name: vec![
                                        ugoite_domain::sql_document::SqlDocumentIdentifier {
                                            value: "secrets".to_owned(),
                                            quote_style: None,
                                        },
                                    ],
                                    alias: Some(
                                        ugoite_domain::sql_document::SqlDocumentIdentifier {
                                            value: "secret_rows".to_owned(),
                                            quote_style: None,
                                        },
                                    ),
                                },
                            constraint:
                                ugoite_domain::sql_document::SqlDocumentJoinConstraint::Using {
                                    columns: vec![
                                        ugoite_domain::sql_document::SqlDocumentIdentifier {
                                            value: "field_100".to_owned(),
                                            quote_style: None,
                                        },
                                    ],
                                },
                        },
                    },
                ],
            },
        )?;
    assert!(
        context
            .execute(unauthorized_join.document.to_sql())
            .await
            .is_err(),
        "typed relation edits cannot expose a Form outside the authorized snapshot"
    );
    let comparison_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
            name: "field_100".to_owned(),
            column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
        }],
        variables: Vec::new(),
        stable_key: Vec::new(),
    };
    let comparison = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT field_100 FROM tasks",
        &comparison_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetFilterExpression {
            expression: Some(
                ugoite_domain::sql_structure::SqlStructureFilterExpression::ColumnComparison {
                    left: ugoite_domain::sql_structure::SqlStructureColumnReference {
                        column: "field_100".to_owned(),
                        qualifier: None,
                    },
                    operator: ugoite_domain::sql_structure::SqlStructureFilterOperator::Eq,
                    right: ugoite_domain::sql_structure::SqlStructureColumnReference {
                        column: "field_100".to_owned(),
                        qualifier: None,
                    },
                },
            ),
        },
    );
    assert_eq!(
        comparison.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported,
        "the Rust filter editor constructs a typed column comparison"
    );
    let comparison_sql = comparison
        .proposed_sql
        .expect("the column comparison adds a WHERE clause");
    let comparison_result = context.execute(&comparison_sql).await?;
    assert_eq!(
        comparison_result
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "the edited predicate reaches authorized planning and filters the Form rows"
    );
    let projection_source = "SELECT 1 FROM tasks WHERE field_100 = 'not-a-number'";
    let projection_document = ugoite_domain::sql_document::parse_sql_document(projection_source);
    let projection_item = projection_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem)
        .expect("the SQL tree exposes the selected projection item");
    let where_expression = projection_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
                && matches!(
                    &node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Identifier {
                        value,
                        ..
                    }) if value == "field_100"
                )
        })
        .expect("the SQL tree exposes the authorized Form-column expression");
    let projection_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            projection_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: projection_document.contract_version,
                expected_source: projection_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::SetProjectionExpression {
                        node_id: projection_item.id,
                        expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem,
                        expression_node_id: where_expression.id,
                        expression_expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression,
                    },
                ],
            },
        )?;
    let projected_result = context.execute(projection_edit.document.to_sql()).await?;
    assert_eq!(
        projected_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("the copied Form-column expression keeps its string type")
            .value(0),
        "not-a-number",
        "a typed projection expression replacement executes through the authorized query context"
    );
    let binary_source = "SELECT field_100, 'x' FROM tasks";
    let binary_document = ugoite_domain::sql_document::parse_sql_document(binary_source);
    let mut binary_items = binary_document
        .nodes
        .iter()
        .filter(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem)
        .collect::<Vec<_>>();
    binary_items.sort_by_key(|node| node.range.map(|range| range.start));
    let binary_left = binary_document
        .nodes
        .iter()
        .find(|node| {
            node.parent == Some(binary_items[0].id)
                && node.kind
                    == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
        })
        .expect("the authorized Form column is a typed expression");
    let binary_right = binary_document
        .nodes
        .iter()
        .find(|node| {
            matches!(
                &node.semantic,
                Some(ugoite_domain::sql_document::SqlDocumentNodeData::Literal {
                    raw: Some(raw),
                    ..
                }) if raw == "'x'"
            )
        })
        .expect("the projection literal is a typed expression");
    let binary_edit = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        binary_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: binary_document.contract_version,
            expected_source: binary_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::ComposeBinaryExpression {
                    node_id: binary_items[0].id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem,
                    left_expression_node_id: binary_left.id,
                    left_expression_expected_kind: binary_left.kind,
                    operator: ugoite_domain::sql_document::SqlDocumentBinaryOperator::Concat,
                    right_expression_node_id: binary_right.id,
                    right_expression_expected_kind: binary_right.kind,
                },
            ],
        },
    )?;
    let binary_result = context.execute(binary_edit.document.to_sql()).await?;
    assert_eq!(
        binary_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("the planner accepts the typed string concatenation")
            .value(0),
        "not-a-numberx",
        "a constructed expression plans and runs inside the authorized Form context"
    );
    let expression_copy_source = "SELECT field_100 || 'x' AS text_value, field_100 FROM tasks";
    let expression_copy_document =
        ugoite_domain::sql_document::parse_sql_document(expression_copy_source);
    let expression_copy_target = expression_copy_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::BinaryExpression
                && node
                    .range
                    .and_then(|range| expression_copy_document.source_slice(range))
                    == Some("field_100 || 'x'")
        })
        .expect("the first projection exposes its concatenation expression");
    let expression_copy_source_node = expression_copy_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
                && node
                    .range
                    .is_some_and(|range| range.start > expression_copy_target.range.unwrap().end)
        })
        .expect("the second projection exposes a same-query identifier");
    let expression_copy_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            expression_copy_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: expression_copy_document.contract_version,
                expected_source: expression_copy_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::ReplaceExpressionFromNode {
                        node_id: expression_copy_target.id,
                        expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::BinaryExpression,
                        expression_node_id: expression_copy_source_node.id,
                        expression_expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression,
                    },
                ],
            },
        )?;
    let expression_copy_result = context
        .execute(expression_copy_edit.document.to_sql())
        .await?;
    assert_eq!(
        expression_copy_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("the planner accepts the copied string expression")
            .value(0),
        "not-a-number",
        "a copied typed expression reaches authorized SQL planning and execution"
    );
    let projection_add_source = "SELECT 1 FROM tasks WHERE field_100 = 'not-a-number'";
    let projection_add_document =
        ugoite_domain::sql_document::parse_sql_document(projection_add_source);
    let projection_list = projection_add_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectList)
        .expect("the SQL tree exposes the projection list");
    let filter_expression = projection_add_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::BinaryExpression
        })
        .expect("the SQL tree exposes the typed comparison expression");
    let projection_add_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            projection_add_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: projection_add_document.contract_version,
                expected_source: projection_add_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddProjectionExpression {
                        node_id: projection_list.id,
                        expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SelectList,
                        expression_node_id: filter_expression.id,
                        expression_expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::BinaryExpression,
                    },
                ],
            },
        )?;
    let projection_add_result = context
        .execute(projection_add_edit.document.to_sql())
        .await?;
    assert_eq!(projection_add_result[0].num_columns(), 2);
    assert!(projection_add_result[0]
        .column(1)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("the copied comparison is a typed boolean projection")
        .value(0));
    assert_eq!(
        projection_add_result[0].num_rows(),
        1,
        "a typed projection expression insertion executes through the authorized query context"
    );
    let scalar_subquery_source = "SELECT field_100 FROM tasks WHERE field_100 = 'not-a-number'";
    let scalar_subquery_document =
        ugoite_domain::sql_document::parse_sql_document(scalar_subquery_source);
    let scalar_subquery_list = scalar_subquery_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectList)
        .expect("the SQL tree exposes the projection list");
    let scalar_subquery_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            scalar_subquery_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: scalar_subquery_document.contract_version,
                expected_source: scalar_subquery_source.to_owned(),
                edits: vec![ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
                    node_id: scalar_subquery_list.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SelectList,
                }],
            },
        )?;
    let scalar_subquery_result = context
        .execute(scalar_subquery_edit.document.to_sql())
        .await?;
    assert_eq!(scalar_subquery_result[0].num_columns(), 2);
    assert_eq!(
        scalar_subquery_result[0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("the nested relation-free SELECT returns a typed integer")
            .value(0),
        1,
        "a constructed scalar subquery reaches authorized planning and execution"
    );
    let projection_move_source = projection_add_edit.document.to_sql();
    let projection_move_document =
        ugoite_domain::sql_document::parse_sql_document(projection_move_source);
    let mut projection_items = projection_move_document
        .nodes
        .iter()
        .filter(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem)
        .collect::<Vec<_>>();
    projection_items.sort_by_key(|node| node.range.map(|range| range.start));
    let projection_move_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            projection_move_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: projection_move_document.contract_version,
                expected_source: projection_move_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::MoveProjectionItem {
                        node_id: projection_items[1].id,
                        expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem,
                        to_index: 0,
                    },
                ],
            },
        )?;
    let projection_move_result = context
        .execute(projection_move_edit.document.to_sql())
        .await?;
    assert_eq!(projection_move_result[0].num_columns(), 2);
    assert!(projection_move_result[0]
        .column(0)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("the moved comparison remains a typed boolean projection")
        .value(0));
    assert_eq!(
        projection_move_result[0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("the other moved projection remains integer typed")
            .value(0),
        1,
        "a typed projection reorder executes through the authorized query context"
    );

    let projection_remove_source = projection_move_edit.document.to_sql();
    let projection_remove_document =
        ugoite_domain::sql_document::parse_sql_document(projection_remove_source);
    let mut projection_remove_items = projection_remove_document
        .nodes
        .iter()
        .filter(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem)
        .collect::<Vec<_>>();
    projection_remove_items.sort_by_key(|node| node.range.map(|range| range.start));
    let projection_remove_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            projection_remove_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: projection_remove_document.contract_version,
                expected_source: projection_remove_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::RemoveProjectionItem {
                        node_id: projection_remove_items[1].id,
                        expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem,
                    },
                ],
            },
        )?;
    let projection_remove_result = context
        .execute(projection_remove_edit.document.to_sql())
        .await?;
    assert_eq!(projection_remove_result[0].num_columns(), 1);
    assert!(projection_remove_result[0]
        .column(0)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("the remaining comparison stays typed")
        .value(0));
    assert_eq!(
        projection_remove_result[0].num_rows(),
        1,
        "a typed projection removal executes through the authorized query context"
    );
    let filter_structure_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
            name: "field_100".to_owned(),
            column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
        }],
        variables: vec![
            ugoite_domain::sql_structure::SqlStructureVariable {
                name: "status".to_owned(),
                variable_type: ugoite_domain::sql_structure::SqlStructureVariableType::String,
            },
            ugoite_domain::sql_structure::SqlStructureVariable {
                name: "status_upper".to_owned(),
                variable_type: ugoite_domain::sql_structure::SqlStructureVariableType::String,
            },
            ugoite_domain::sql_structure::SqlStructureVariable {
                name: "pattern".to_owned(),
                variable_type: ugoite_domain::sql_structure::SqlStructureVariableType::String,
            },
        ],
        stable_key: vec![],
    };
    let filter_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT * FROM tasks",
        &filter_structure_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetFilters {
            filters: vec![ugoite_domain::sql_structure::SqlStructureFilter {
                column: "field_100".to_owned(),
                qualifier: None,
                operator: ugoite_domain::sql_structure::SqlStructureFilterOperator::Eq,
                parameter: Some("status".to_owned()),
                second_parameter: None,
            }],
        },
    );
    assert_eq!(
        filter_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported
    );
    let filtered_sql = filter_proposal
        .proposed_sql
        .expect("the typed Filter edit returns executable SQL");
    let filtered = context
        .execute_with_parameters(
            &filtered_sql,
            HashMap::from([(
                "status".to_owned(),
                datafusion::scalar::ScalarValue::Utf8(Some("not-a-number".to_owned())),
            )]),
        )
        .await?;
    assert_eq!(
        filtered.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1
    );

    let range_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT * FROM tasks",
        &filter_structure_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetFilterExpression {
            expression: Some(
                ugoite_domain::sql_structure::SqlStructureFilterExpression::Predicate {
                    column: "field_100".to_owned(),
                    qualifier: None,
                    operator: ugoite_domain::sql_structure::SqlStructureFilterOperator::Between,
                    parameter: Some("status".to_owned()),
                    second_parameter: Some("status_upper".to_owned()),
                },
            ),
        },
    );
    assert_eq!(
        range_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported
    );
    let range_sql = range_proposal
        .proposed_sql
        .expect("the typed range edit returns executable SQL");
    assert!(range_sql.contains("BETWEEN $status AND $status_upper"));
    let ranged = context
        .execute_with_parameters(
            &range_sql,
            HashMap::from([
                (
                    "status".to_owned(),
                    datafusion::scalar::ScalarValue::Utf8(Some("a".to_owned())),
                ),
                (
                    "status_upper".to_owned(),
                    datafusion::scalar::ScalarValue::Utf8(Some("z".to_owned())),
                ),
            ]),
        )
        .await?;
    assert_eq!(
        ranged.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "both range bounds are bound through the authorized query context"
    );

    for (operator, parameter_name, parameter_value, expected_rows) in [
        (
            ugoite_domain::sql_structure::SqlStructureFilterOperator::In,
            "status",
            "not-a-number",
            1,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureFilterOperator::NotIn,
            "status",
            "not-a-number",
            0,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureFilterOperator::Like,
            "pattern",
            "%number%",
            1,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureFilterOperator::NotLike,
            "pattern",
            "%number%",
            0,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureFilterOperator::Ilike,
            "pattern",
            "%NUMBER%",
            1,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureFilterOperator::NotIlike,
            "pattern",
            "%NUMBER%",
            0,
        ),
    ] {
        let proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
            "SELECT * FROM tasks",
            &filter_structure_context,
            ugoite_domain::sql_structure::SqlStructureOperation::SetFilterExpression {
                expression: Some(
                    ugoite_domain::sql_structure::SqlStructureFilterExpression::Predicate {
                        column: "field_100".to_owned(),
                        qualifier: None,
                        operator,
                        parameter: Some(parameter_name.to_owned()),
                        second_parameter: None,
                    },
                ),
            },
        );
        assert_eq!(
            proposal.status,
            ugoite_domain::sql_structure::SqlStructureStatus::Supported,
            "{operator:?}"
        );
        let sql = proposal.proposed_sql.expect("typed predicate SQL");
        let result = context
            .execute_with_parameters(
                &sql,
                HashMap::from([(
                    parameter_name.to_owned(),
                    datafusion::scalar::ScalarValue::Utf8(Some(parameter_value.to_owned())),
                )]),
            )
            .await?;
        assert_eq!(
            result.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            expected_rows,
            "{operator:?} executes through the authorized query context: {sql}"
        );
    }

    let mut grouping_policy = policy(&tasks, &[37]);
    grouping_policy.limits.function_policy = SqlFunctionPolicy::ReadOnlyAggregateV1;
    let grouping_context = workspace.authorized_query_context(grouping_policy).await?;
    let count_source = "SELECT COUNT(field_100) AS item_count FROM tasks";
    let count_document = ugoite_domain::sql_document::parse_sql_document(count_source);
    let count_function = count_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::FunctionExpression
                && matches!(
                    &node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Function {
                        name,
                        ..
                    }) if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("COUNT"))
                )
        })
        .expect("the SQL tree exposes the COUNT aggregate");
    let count_argument = count_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
                && node.parent == Some(count_function.id)
        })
        .expect("the SQL tree exposes COUNT's typed expression argument");
    let count_argument_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            count_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: count_document.contract_version,
                expected_source: count_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::SetFunctionArgument {
                        node_id: count_function.id,
                        expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::FunctionExpression,
                        argument_index: 1,
                        expression_node_id: count_argument.id,
                        expression_expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression,
                    },
                ],
            },
        )?;
    let count_argument_result = grouping_context
        .execute(count_argument_edit.document.to_sql())
        .await?;
    assert_eq!(
        count_argument_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("COUNT of typed arguments returns Int64")
            .value(0),
        1,
        "a typed expression copied into COUNT executes through the authorized query context"
    );
    let case_source =
        "SELECT SUM(CASE WHEN field_100 = 'not-a-number' THEN 10 ELSE 1 END) AS total FROM tasks";
    let case_document = ugoite_domain::sql_document::parse_sql_document(case_source);
    let then_literal = case_document
        .nodes
        .iter()
        .find(|node| {
            node.range
                .and_then(|range| case_document.source_slice(range))
                == Some("10")
                && matches!(
                    node.semantic.as_ref(),
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Literal {
                        literal_kind: ugoite_domain::sql_document::SqlDocumentLiteralKind::Number,
                        ..
                    })
                )
        })
        .expect("the SQL tree exposes the CASE THEN numeric literal");
    let edited_case = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        case_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: case_document.contract_version,
            expected_source: case_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::ReplaceLiteral {
                    node_id: then_literal.id,
                    expected_kind: then_literal.kind,
                    literal: ugoite_domain::sql_document::SqlDocumentSemanticLiteral::Number {
                        value: "20".to_owned(),
                    },
                },
            ],
        },
    )?;
    let case_result = grouping_context
        .execute(edited_case.document.to_sql())
        .await?;
    assert_eq!(
        case_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("SUM(CASE ...) remains an Int64 result")
            .value(0),
        20,
        "the exact CASE tree literal edit changes the authorized aggregate result"
    );
    let case_node = case_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::CaseExpression)
        .expect("the SQL tree exposes the CASE expression");
    let case_branch_edit =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            case_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: case_document.contract_version,
                expected_source: case_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddCaseBranch {
                        node_id: case_node.id,
                        expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::CaseExpression,
                    },
                ],
            },
        )?;
    let case_branch_result = grouping_context
        .execute(case_branch_edit.document.to_sql())
        .await?;
    assert_eq!(
        case_branch_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("CASE branch editing keeps the aggregate type")
            .value(0),
        10,
        "the added typed CASE branch executes through the authorized query context"
    );

    let aggregate_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT * FROM tasks",
        &ugoite_domain::sql_structure::SqlStructureContext {
            relation: "tasks".to_owned(),
            columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
                name: "field_100".to_owned(),
                column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
            }],
            variables: vec![],
            stable_key: vec![],
        },
        ugoite_domain::sql_structure::SqlStructureOperation::SetAggregates {
            aggregates: vec![ugoite_domain::sql_structure::SqlStructureAggregate {
                function: ugoite_domain::sql_structure::SqlStructureAggregateFunction::Count,
                column: None,
                arguments: vec![],
                distinct: false,
                alias: Some(ugoite_domain::sql_structure::SqlStructureAlias {
                    name: "row_count".to_owned(),
                    quoted: true,
                }),
            }],
        },
    );
    assert_eq!(
        aggregate_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported
    );
    let aggregate_sql = aggregate_proposal
        .proposed_sql
        .expect("the typed aggregate edit returns executable SQL");
    let aggregate_result = grouping_context.execute(&aggregate_sql).await?;
    assert_eq!(aggregate_result.len(), 1);
    assert_eq!(
        aggregate_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("COUNT(*) result stays Int64")
            .value(0),
        1
    );
    let having_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT COUNT(*) AS item_count FROM tasks",
        &ugoite_domain::sql_structure::SqlStructureContext {
            relation: "tasks".to_owned(),
            columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
                name: "field_100".to_owned(),
                column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
            }],
            variables: vec![],
            stable_key: vec![],
        },
        ugoite_domain::sql_structure::SqlStructureOperation::SetHaving {
            expression: Some(
                ugoite_domain::sql_structure::SqlStructureHavingExpression::Predicate {
                    aggregate: ugoite_domain::sql_structure::SqlStructureAggregate {
                        function:
                            ugoite_domain::sql_structure::SqlStructureAggregateFunction::Count,
                        column: None,
                        arguments: vec![],
                        distinct: false,
                        alias: None,
                    },
                    operator: ugoite_domain::sql_structure::SqlStructureHavingOperator::Gt,
                    value: ugoite_domain::sql_structure::SqlStructureHavingValue::Integer(
                        "0".to_owned(),
                    ),
                },
            ),
        },
    );
    assert_eq!(
        having_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported
    );
    let having_sql = having_proposal
        .proposed_sql
        .expect("the typed HAVING edit returns executable SQL");
    let having_result = grouping_context.execute(&having_sql).await?;
    assert_eq!(
        having_result
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1
    );
    let grouping_structure_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
            name: "field_100".to_owned(),
            column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
        }],
        variables: vec![],
        stable_key: vec![],
    };
    let grouping_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT field_100 FROM tasks",
        &grouping_structure_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetGroupBy {
            columns: vec!["field_100".to_owned()],
        },
    );
    assert_eq!(
        grouping_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported,
        "GROUP BY proposal was rejected: {grouping_proposal:?}"
    );
    let grouped_sql = grouping_proposal
        .proposed_sql
        .expect("the typed GROUP BY edit returns executable SQL");
    let grouped = grouping_context.execute(&grouped_sql).await?;
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].num_rows(), 1);
    assert_eq!(grouped[0].num_columns(), 1);

    let relation_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
            name: "field_100".to_owned(),
            column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
        }],
        variables: vec![],
        stable_key: vec![],
    };
    let relation_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT 1",
        &relation_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetRelation {
            relation: "tasks".to_owned(),
        },
    );
    assert_eq!(
        relation_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported
    );
    let selected_form_sql = relation_proposal
        .proposed_sql
        .expect("the trusted Form picker returns executable SQL");
    let selected_form = context.execute(&selected_form_sql).await?;
    assert_eq!(
        selected_form
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1
    );

    let projection_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
            name: "field_100".to_owned(),
            column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
        }],
        variables: vec![],
        stable_key: vec![],
    };
    let projection_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT * FROM tasks",
        &projection_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetColumns {
            columns: vec!["field_100".to_owned()],
            aliases: vec![],
        },
    );
    assert_eq!(
        projection_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported
    );
    let projected_sql = projection_proposal
        .proposed_sql
        .expect("the typed Columns edit returns executable SQL");
    let projected = context.execute(&projected_sql).await?;
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].num_rows(), 1);
    assert_eq!(projected[0].num_columns(), 1);

    let row_limit_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
            name: "field_100".to_owned(),
            column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
        }],
        variables: vec![],
        stable_key: vec![],
    };
    let row_limit_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        "SELECT * FROM tasks",
        &row_limit_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetRowLimit {
            limit: Some("1".to_owned()),
            offset: None,
        },
    );
    assert_eq!(
        row_limit_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported
    );
    let row_limited_sql = row_limit_proposal
        .proposed_sql
        .expect("the typed row-limit edit returns executable SQL");
    let row_limited = context.execute(&row_limited_sql).await?;
    assert_eq!(
        row_limited
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "authorized execution applies the exact GUI-proposed LIMIT"
    );

    let mut sort_policy = policy(&tasks, &[37, 39, 41]);
    sort_policy
        .forms
        .get_mut(&tasks.id)
        .unwrap()
        .system_columns
        .insert(QuerySystemColumn::ExternalId);
    let sort_query_context = workspace.authorized_query_context(sort_policy).await?;
    let base_relation = sort_query_context.execute("SELECT * FROM tasks").await?;
    let base_relation_columns = base_relation
        .first()
        .map(|batch| batch.num_columns())
        .unwrap_or_default();
    assert!(base_relation_columns > 0);

    let join_structure_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![ugoite_domain::sql_structure::SqlStructureColumn {
            name: "field_100".to_owned(),
            column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
        }],
        variables: vec![],
        stable_key: vec![],
    };
    for (join_kind, keyword, expected_rows) in [
        (
            ugoite_domain::sql_structure::SqlStructureJoinKind::Inner,
            "INNER JOIN",
            2,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureJoinKind::Left,
            "LEFT JOIN",
            3,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureJoinKind::Right,
            "RIGHT JOIN",
            3,
        ),
        (
            ugoite_domain::sql_structure::SqlStructureJoinKind::Full,
            "FULL JOIN",
            4,
        ),
    ] {
        let join_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
            "SELECT * FROM tasks",
            &join_structure_context,
            ugoite_domain::sql_structure::SqlStructureOperation::SetJoin {
                join_kind,
                left_column: "field_100".to_owned(),
                right_column: "field_100".to_owned(),
            },
        );
        assert_eq!(
            join_proposal.status,
            ugoite_domain::sql_structure::SqlStructureStatus::Supported,
            "{keyword} proposal was rejected: {join_proposal:?}"
        );
        let joined_sql = join_proposal
            .proposed_sql
            .expect("the typed JOIN edit returns executable SQL");
        assert!(joined_sql.contains(keyword), "{joined_sql}");
        let joined = sort_query_context.execute(&joined_sql).await?;
        assert!(
            joined
                .iter()
                .all(|batch| batch.num_columns() == base_relation_columns * 2),
            "authorized {keyword} must preserve both relation outputs"
        );
        assert_eq!(
            joined.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            expected_rows,
            "authorized {keyword} must preserve outer rows without scanning policy-hidden entries"
        );
    }

    let sort_source = "SELECT field_100 FROM tasks ORDER BY field_100 ASC NULLS FIRST";
    let sort_context = ugoite_domain::sql_structure::SqlStructureContext {
        relation: "tasks".to_owned(),
        columns: vec![
            ugoite_domain::sql_structure::SqlStructureColumn {
                name: "field_100".to_owned(),
                column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
            },
            ugoite_domain::sql_structure::SqlStructureColumn {
                name: "_ugoite_id".to_owned(),
                column_type: ugoite_domain::sql_structure::SqlStructureColumnType::String,
            },
        ],
        variables: vec![],
        stable_key: vec!["_ugoite_id".to_owned()],
    };
    let sort_proposal = ugoite_domain::sql_structure::propose_sql_structure_operation(
        sort_source,
        &sort_context,
        ugoite_domain::sql_structure::SqlStructureOperation::SetSort {
            order_by: vec![ugoite_domain::sql_structure::SqlStructureSort {
                column: "field_100".to_owned(),
                qualifier: None,
                projection_alias: None,
                direction: ugoite_domain::sql_structure::SqlStructureSortDirection::Desc,
                nulls_first: Some(false),
            }],
        },
    );
    assert_eq!(
        sort_proposal.status,
        ugoite_domain::sql_structure::SqlStructureStatus::Supported,
        "Sort edit proposal was rejected: {sort_proposal:?}"
    );
    let edited_sort_sql = sort_proposal
        .proposed_sql
        .expect("the typed Sort edit returns executable SQL");
    assert!(
        edited_sort_sql.contains("ORDER BY \"field_100\" DESC NULLS LAST"),
        "unexpected typed Sort output: {edited_sort_sql}"
    );
    let sorted = sort_query_context.execute(&edited_sort_sql).await?;
    let sorted_values = sorted
        .iter()
        .flat_map(|batch| {
            batch
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .expect("the authorized Sort projection stays string typed")
                .iter()
                .map(|value| value.map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sorted_values,
        vec![
            Some("not-a-number".to_owned()),
            Some("alpha".to_owned()),
            None
        ],
        "the typed Sort edit executes with explicit NULLS LAST semantics"
    );

    let sql = "SELECT CAST(field_100 AS BIGINT) AS numeric_title FROM tasks";

    let schema = context.stateless_result_schema(sql, HashMap::new()).await?;
    assert_eq!(schema.len(), 1);
    assert_eq!(schema[0].name, "numeric_title");
    assert_eq!(
        schema[0].column_type,
        ugoite_core::sql_query::SqlResultColumnType::Integer
    );
    assert!(context
        .execute_stateless_page(sql, HashMap::new(), 0, 1)
        .await
        .is_err());

    let document = ugoite_domain::sql_document::parse_sql_document(sql);
    let cast = document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::CastExpression)
        .expect("the typed SQL document exposes the CAST node");
    let edited = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        sql,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: sql.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::ReplaceCastTargetType {
                    node_id: cast.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::CastExpression,
                    target_type: ugoite_domain::sql_document::SqlDocumentCastType::Varchar,
                },
            ],
        },
    )?;
    let batches = context.execute(edited.document.to_sql()).await?;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);
    assert!(
        format!("{:?}", batches[0].column(0)).contains("not-a-number"),
        "the authorized planner returns the edited VARCHAR cast"
    );

    let wrap_source = "SELECT field_100 FROM tasks WHERE field_100 = 'not-a-number'";
    let wrap_document = ugoite_domain::sql_document::parse_sql_document(wrap_source);
    let select_item = wrap_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem)
        .expect("the SQL document exposes the selected Form column");
    let expression = wrap_document
        .nodes
        .iter()
        .find(|node| {
            node.parent == Some(select_item.id)
                && node.kind
                    == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
        })
        .expect("the selected Form column is a typed expression node");
    let wrapped = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        wrap_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: wrap_document.contract_version,
            expected_source: wrap_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::WrapExpressionInCast {
                    node_id: expression.id,
                    expected_kind: expression.kind,
                    target_type: ugoite_domain::sql_document::SqlDocumentCastType::Varchar,
                },
            ],
        },
    )?;
    assert_eq!(
        wrapped.document.to_sql(),
        "SELECT CAST(field_100 AS VARCHAR) FROM tasks WHERE field_100 = 'not-a-number'"
    );
    let wrapped_batches = context.execute(wrapped.document.to_sql()).await?;
    assert_eq!(
        wrapped_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "the generated CAST SQL executes through the authorized Form query path"
    );
    assert!(matches!(
        wrapped_batches[0].column(0).data_type(),
        arrow_schema::DataType::Utf8
            | arrow_schema::DataType::LargeUtf8
            | arrow_schema::DataType::Utf8View
    ));
    assert!(
        format!("{:?}", wrapped_batches[0].column(0)).contains("not-a-number"),
        "the generated VARCHAR CAST preserves the authorized Form value"
    );

    let in_list_source =
        "SELECT field_100 FROM tasks WHERE field_100 IN ('not-a-number', 'unmatched')";
    let in_list_document = ugoite_domain::sql_document::parse_sql_document(in_list_source);
    let in_predicate = in_list_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::PredicateExpression
                && matches!(
                    node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Predicate {
                        ref operator,
                        ..
                    }) if operator == "in"
                )
        })
        .expect("the typed SQL document exposes the IN predicate");
    let in_list_edit = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        in_list_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: in_list_document.contract_version,
            expected_source: in_list_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::AppendInListValue {
                    node_id: in_predicate.id,
                    expected_kind: in_predicate.kind,
                },
            ],
        },
    )?;
    assert_eq!(
        in_list_edit.document.to_sql(),
        "SELECT field_100 FROM tasks WHERE field_100 IN ('not-a-number', 'unmatched', NULL)"
    );
    let in_list_batches = context.execute(in_list_edit.document.to_sql()).await?;
    assert_eq!(
        in_list_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "the typed IN-list edit keeps execution within the authorized Form path"
    );

    let set_source = "SELECT field_100 FROM tasks UNION ALL SELECT field_100 FROM tasks";
    let set_document = ugoite_domain::sql_document::parse_sql_document(set_source);
    let set_operation = set_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SetOperation)
        .expect("the typed SQL document exposes the set operation node");
    let edited_set = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        set_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: set_document.contract_version,
            expected_source: set_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::ReplaceSetOperation {
                    node_id: set_operation.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SetOperation,
                    operator: ugoite_domain::sql_document::SqlDocumentSetOperator::Except,
                    quantifier: ugoite_domain::sql_document::SqlDocumentSetQuantifier::Distinct,
                },
            ],
        },
    )?;
    let batches = context.execute(edited_set.document.to_sql()).await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        0,
        "the exact SQL produced by a typed set-operation edit is planned and executed"
    );

    let set_add_source = "SELECT 1";
    let set_add_document = ugoite_domain::sql_document::parse_sql_document(set_add_source);
    let set_add_query = set_add_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::Query
                && node
                    .range
                    .and_then(|range| set_add_document.source_slice(range))
                    .is_some_and(|text| text.trim() == set_add_source)
        })
        .expect("the typed SQL document exposes the query node");
    let appended_set = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        set_add_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: set_add_document.contract_version,
            expected_source: set_add_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddSetOperationOperand {
                    node_id: set_add_query.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::Query,
                    operator: ugoite_domain::sql_document::SqlDocumentSetOperator::Union,
                    quantifier: ugoite_domain::sql_document::SqlDocumentSetQuantifier::All,
                },
            ],
        },
    )?;
    let batches = context.execute(appended_set.document.to_sql()).await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        2,
        "the appended typed set-operation seed reaches the authorized executor"
    );
    let appended_set_source = appended_set.document.to_sql();
    let appended_set_document =
        ugoite_domain::sql_document::parse_sql_document(appended_set_source);
    let appended_operation = appended_set_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::SetOperation)
        .expect("the appended set operation has a typed node");
    let removed_set = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        appended_set_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: appended_set_document.contract_version,
            expected_source: appended_set_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::RemoveSetOperationOperand {
                    node_id: appended_operation.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::SetOperation,
                    keep: ugoite_domain::sql_document::SqlDocumentSetOperand::Left,
                },
            ],
        },
    )?;
    assert_eq!(removed_set.document.to_sql(), set_add_source);
    let batches = context.execute(removed_set.document.to_sql()).await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "removing the appended operand keeps the SQL authorized and executable"
    );

    let cte_source = "WITH c AS (SELECT field_100 FROM tasks) SELECT field_100 FROM c";
    let cte_document = ugoite_domain::sql_document::parse_sql_document(cte_source);
    let cte = cte_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::CommonTableExpression
        })
        .expect("the typed SQL document exposes the CTE node");
    let edited_cte = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        cte_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: cte_document.contract_version,
            expected_source: cte_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::RenameCommonTableExpression {
                    node_id: cte.id,
                    expected_kind:
                        ugoite_domain::sql_document::SqlDocumentNodeKind::CommonTableExpression,
                    name: ugoite_domain::sql_document::SqlDocumentIdentifier {
                        value: "rows".to_owned(),
                        quote_style: None,
                    },
                },
            ],
        },
    )?;
    let batches = context.execute(edited_cte.document.to_sql()).await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "the exact SQL produced by a typed CTE edit is planned and executed"
    );

    let scoped_cte_source = "WITH c AS (SELECT field_100 FROM tasks WHERE field_100 IS NOT NULL), d AS (WITH c AS (SELECT field_100 FROM tasks WHERE field_100 IS NULL) SELECT field_100 FROM c), e AS (SELECT field_100 FROM c) SELECT field_100 FROM c";
    let scoped_cte_document = ugoite_domain::sql_document::parse_sql_document(scoped_cte_source);
    let outer_cte = scoped_cte_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::CommonTableExpression
                && matches!(node.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
        })
        .expect("the outer CTE declaration is exposed by the typed document");
    let renamed_scoped_cte =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            scoped_cte_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: scoped_cte_document.contract_version,
                expected_source: scoped_cte_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::RenameCommonTableExpression {
                        node_id: outer_cte.id,
                        expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::CommonTableExpression,
                        name: ugoite_domain::sql_document::SqlDocumentIdentifier {
                            value: "rows".to_owned(),
                            quote_style: None,
                        },
                    },
                ],
            },
        )?;
    assert_eq!(
        renamed_scoped_cte.document.to_sql(),
        "WITH rows AS (SELECT field_100 FROM tasks WHERE field_100 IS NOT NULL), d AS (WITH c AS (SELECT field_100 FROM tasks WHERE field_100 IS NULL) SELECT field_100 FROM c), e AS (SELECT field_100 FROM rows) SELECT field_100 FROM rows"
    );
    let batches = context
        .execute(renamed_scoped_cte.document.to_sql())
        .await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "scope-aware CTE rename preserves nested shadowing and executes in the authorized context"
    );

    let add_cte_source = "SELECT field_100 FROM tasks";
    let add_cte_document = ugoite_domain::sql_document::parse_sql_document(add_cte_source);
    let query = add_cte_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::Query
                && node
                    .range
                    .and_then(|range| add_cte_document.source_slice(range))
                    .is_some_and(|text| text.trim() == add_cte_source)
        })
        .expect("the typed SQL document exposes the query node");
    let added_cte = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        add_cte_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: add_cte_document.contract_version,
            expected_source: add_cte_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddCommonTableExpression {
                    node_id: query.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::Query,
                    name: ugoite_domain::sql_document::SqlDocumentIdentifier {
                        value: "unused_rows".to_owned(),
                        quote_style: None,
                    },
                },
            ],
        },
    )?;
    let batches = context.execute(added_cte.document.to_sql()).await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "the exact SQL produced by a typed CTE addition is authorized and executable"
    );
    let added_cte_source = added_cte.document.to_sql();
    let added_cte_document = ugoite_domain::sql_document::parse_sql_document(added_cte_source);
    let unused_cte = added_cte_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::CommonTableExpression
        })
        .expect("the added CTE has a typed declaration");
    let removed_cte = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        added_cte_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: added_cte_document.contract_version,
            expected_source: added_cte_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::RemoveCommonTableExpression {
                    node_id: unused_cte.id,
                    expected_kind:
                        ugoite_domain::sql_document::SqlDocumentNodeKind::CommonTableExpression,
                },
            ],
        },
    )?;
    assert_eq!(removed_cte.document.to_sql(), add_cte_source);
    let batches = context.execute(removed_cte.document.to_sql()).await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "removing an unreferenced CTE keeps the authorized query executable"
    );

    let predicate_source = "SELECT field_100 FROM tasks WHERE field_100 IS NULL";
    let predicate_document = ugoite_domain::sql_document::parse_sql_document(predicate_source);
    let predicate = predicate_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::PredicateExpression
        })
        .expect("the typed SQL document exposes the NULL predicate");
    let edited_predicate =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            predicate_source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: predicate_document.contract_version,
                expected_source: predicate_source.to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::ReplacePredicateNegation {
                        node_id: predicate.id,
                        expected_kind:
                            ugoite_domain::sql_document::SqlDocumentNodeKind::PredicateExpression,
                        negated: true,
                    },
                ],
            },
        )?;
    let batches = context.execute(edited_predicate.document.to_sql()).await?;
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "the exact SQL produced by a typed predicate edit is planned and authorized"
    );

    let predicate_seed_source = "SELECT field_100 FROM tasks";
    let predicate_seed_document =
        ugoite_domain::sql_document::parse_sql_document(predicate_seed_source);
    let predicate_seed_expression = predicate_seed_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
        })
        .expect("the SQL document exposes the typed numeric expression");
    let predicate_seed = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        predicate_seed_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: predicate_seed_document.contract_version,
            expected_source: predicate_seed_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::WrapExpressionInPredicate {
                    node_id: predicate_seed_expression.id,
                    expected_kind: predicate_seed_expression.kind,
                    template: ugoite_domain::sql_document::SqlDocumentPredicateTemplate::Between,
                },
            ],
        },
    )?;
    assert_eq!(
        predicate_seed.document.to_sql(),
        "SELECT (field_100) BETWEEN NULL AND NULL FROM tasks"
    );
    let predicate_seed_batches = context.execute(predicate_seed.document.to_sql()).await?;
    assert_eq!(predicate_seed_batches.len(), 1);
    assert_eq!(
        predicate_seed_batches[0].schema().field(0).data_type(),
        &arrow_schema::DataType::Boolean,
        "the generated BETWEEN predicate is typed and authorized for execution"
    );

    let where_seed_source = "SELECT field_100 FROM tasks ORDER BY field_100 LIMIT 2";
    let where_seed_document = ugoite_domain::sql_document::parse_sql_document(where_seed_source);
    let select = where_seed_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::Select)
        .expect("the query document exposes its SELECT node");
    let where_seed = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        where_seed_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: where_seed_document.contract_version,
            expected_source: where_seed_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddWhereClause {
                    node_id: select.id,
                    expected_kind: select.kind,
                },
            ],
        },
    )?;
    assert_eq!(
        where_seed.document.to_sql(),
        "SELECT field_100 FROM tasks WHERE TRUE ORDER BY field_100 LIMIT 2"
    );
    let where_seed_batches = context.execute(where_seed.document.to_sql()).await?;
    assert_eq!(
        where_seed_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "the generated WHERE seed remains executable through the authorized query context"
    );

    let aggregate_tasks = aggregate_form(39);
    create_form(&workspace, &aggregate_tasks).await?;
    append_aggregate(
        &workspace,
        aggregate_revision(&aggregate_tasks, 40, 41, "one", 1, FieldValue::Number(1.0)),
    )
    .await?;
    let aggregate_context = workspace
        .authorized_query_context(aggregate_policy(&aggregate_tasks, &[40]))
        .await?;
    let case_wrap_source = "SELECT field_101 FROM tasks";
    let case_wrap_document = ugoite_domain::sql_document::parse_sql_document(case_wrap_source);
    let amount = case_wrap_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
                && matches!(
                    &node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Identifier {
                        value,
                        ..
                    }) if value == "field_101"
                )
        })
        .expect("the SQL document exposes the numeric Form column");
    let wrapped_case = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        case_wrap_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: case_wrap_document.contract_version,
            expected_source: case_wrap_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::WrapExpressionInCase {
                    node_id: amount.id,
                    expected_kind: amount.kind,
                },
            ],
        },
    )?;
    assert_eq!(
        wrapped_case.document.to_sql(),
        "SELECT CASE WHEN TRUE THEN field_101 ELSE NULL END FROM tasks"
    );
    let wrapped_case_batches = aggregate_context
        .execute(wrapped_case.document.to_sql())
        .await?;
    assert_eq!(wrapped_case_batches.len(), 1);
    assert_eq!(wrapped_case_batches[0].num_rows(), 1);
    assert_eq!(
        wrapped_case_batches[0].schema().field(0).data_type(),
        &arrow_schema::DataType::Int64,
        "the CASE expression preserves the numeric Form column's result type"
    );
    assert_eq!(
        wrapped_case_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("the authorized CASE result remains Int64")
            .value(0),
        1,
        "the generated CASE preserves the selected Form value"
    );
    let unary_source = "SELECT -COUNT(*) AS row_count FROM tasks";
    let unary_document = ugoite_domain::sql_document::parse_sql_document(unary_source);
    let unary = unary_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::UnaryExpression
                && matches!(
                    node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::UnaryOperator {
                        ref operator
                    }) if operator == "-"
                )
        })
        .expect("the typed SQL document exposes the unary aggregate sign");
    let edited_unary = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        unary_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: unary_document.contract_version,
            expected_source: unary_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::ReplaceUnarySign {
                    node_id: unary.id,
                    expected_kind:
                        ugoite_domain::sql_document::SqlDocumentNodeKind::UnaryExpression,
                    sign: ugoite_domain::sql_document::SqlDocumentUnarySign::Positive,
                },
            ],
        },
    )?;
    let batches = aggregate_context
        .execute(edited_unary.document.to_sql())
        .await?;
    let values = batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("the authorized aggregate expression returns an integer");
    assert_eq!(values.value(0), 1);
    let group_by_source = "SELECT field_100 FROM tasks ORDER BY field_100";
    let group_by_document = ugoite_domain::sql_document::parse_sql_document(group_by_source);
    let select = group_by_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::Select)
        .expect("the SQL tree exposes the selected SELECT body");
    let group_expression = group_by_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
                && node.parent.is_some_and(|parent| {
                    group_by_document.nodes[parent as usize].kind
                        == ugoite_domain::sql_document::SqlDocumentNodeKind::SelectItem
                })
        })
        .expect("the authorized Form column is a typed grouping expression");
    let group_by_edit = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        group_by_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: group_by_document.contract_version,
            expected_source: group_by_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::AddGroupByExpression {
                    node_id: select.id,
                    expected_kind: ugoite_domain::sql_document::SqlDocumentNodeKind::Select,
                    expression_node_id: group_expression.id,
                    expression_expected_kind:
                        ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression,
                },
            ],
        },
    )?;
    assert_eq!(
        group_by_edit.document.to_sql(),
        "SELECT field_100 FROM tasks GROUP BY field_100 ORDER BY field_100"
    );
    let grouped = context.execute(group_by_edit.document.to_sql()).await?;
    assert_eq!(
        grouped.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1,
        "the typed grouping expression reaches authorized SQL planning and execution"
    );
    Ok(())
}

#[tokio::test]
async fn typed_sql_document_nested_structure_edits_execute_authorized_queries() -> anyhow::Result<()>
{
    use ugoite_domain::sql_document::{
        SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentRelationFactor,
        SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction, SqlDocumentSetOperator,
        SqlDocumentSetQuantifier, SqlDocumentSubqueryPredicateTemplate,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(76)),
        "memory://authorized-query-nested-structure",
    )
    .await?;
    let tasks = form(77, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 78, 79, "alpha").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[78]))
        .await?;

    let apply = |source: &str, edits: Vec<SqlDocumentSemanticEdit>| {
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: document.contract_version,
                expected_source: source.to_owned(),
                edits,
            },
        )
    };

    let scalar_source = "SELECT 1";
    let scalar_document = ugoite_domain::sql_document::parse_sql_document(scalar_source);
    let scalar_list = scalar_document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::SelectList)
        .expect("the scalar SELECT has a typed projection list");
    let scalar = apply(
        scalar_source,
        vec![SqlDocumentSemanticEdit::AddScalarSubqueryProjection {
            node_id: scalar_list.id,
            expected_kind: SqlDocumentNodeKind::SelectList,
        }],
    )?;
    let scalar_select = scalar
        .document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::Select
                && node
                    .range
                    .and_then(|range| scalar.document.source_slice(range))
                    == Some("SELECT 1")
                && node.parent.is_some_and(|parent| {
                    scalar
                        .document
                        .nodes
                        .get(parent as usize)
                        .is_some_and(|query| {
                            query.parent.is_some_and(|grandparent| {
                                scalar.document.nodes.get(grandparent as usize).is_some_and(
                                    |subquery| {
                                        subquery.kind == SqlDocumentNodeKind::SubqueryExpression
                                    },
                                )
                            })
                        })
                })
        })
        .expect("the scalar subquery has its own typed SELECT scope");
    let scalar_with_from = apply(
        &scalar.document.source,
        vec![SqlDocumentSemanticEdit::AddFromRelation {
            node_id: scalar_select.id,
            expected_kind: SqlDocumentNodeKind::Select,
            factor: SqlDocumentRelationFactor::Table {
                name: vec![SqlDocumentIdentifier {
                    value: "tasks".to_owned(),
                    quote_style: None,
                }],
                alias: None,
            },
        }],
    )?;
    assert_eq!(
        scalar_with_from.document.to_sql(),
        "SELECT 1, (SELECT 1 FROM tasks) AS __ugoite_scalar_subquery"
    );
    let scalar_batches = context.execute(scalar_with_from.document.to_sql()).await?;
    assert_eq!(scalar_batches.len(), 1);
    assert_eq!(scalar_batches[0].num_columns(), 2);
    assert_eq!(
        scalar_batches[0]
            .column(1)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("nested scalar SELECT has an integer result")
            .value(0),
        1
    );

    let in_source = "SELECT 1";
    let in_document = ugoite_domain::sql_document::parse_sql_document(in_source);
    let operand = in_document
        .nodes
        .iter()
        .find(|node| {
            node.range.and_then(|range| in_document.source_slice(range)) == Some("1")
                && node.parent.is_some_and(|parent| {
                    in_document
                        .nodes
                        .get(parent as usize)
                        .is_some_and(|item| item.kind == SqlDocumentNodeKind::SelectItem)
                })
        })
        .expect("the projection expression is a typed node");
    let in_query = apply(
        in_source,
        vec![SqlDocumentSemanticEdit::WrapExpressionInSubqueryPredicate {
            node_id: operand.id,
            expected_kind: operand.kind,
            template: SqlDocumentSubqueryPredicateTemplate::In,
        }],
    )?;
    assert_eq!(
        in_query.document.status,
        ugoite_domain::sql_document::SqlDocumentStatus::Parsed
    );
    let in_error = context
        .execute(in_query.document.to_sql())
        .await
        .expect_err("the current physical executor rejects IN subqueries");
    let in_error = format!("{in_error:#}");
    assert!(
        in_error.contains("Physical plan does not support logical expression InSubquery"),
        "unexpected IN-subquery executor diagnostic: {in_error}"
    );

    let exists_source = "SELECT field_100 FROM tasks WHERE TRUE";
    let exists_document = ugoite_domain::sql_document::parse_sql_document(exists_source);
    let truth = exists_document
        .nodes
        .iter()
        .find(|node| {
            node.range
                .and_then(|range| exists_document.source_slice(range))
                == Some("TRUE")
        })
        .expect("the WHERE seed is typed");
    let exists_query = apply(
        exists_source,
        vec![
            SqlDocumentSemanticEdit::ReplaceExpressionWithExistsSubquery {
                node_id: truth.id,
                expected_kind: truth.kind,
            },
        ],
    )?;
    let exists_batches = context.execute(exists_query.document.to_sql()).await?;
    assert_eq!(
        exists_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1
    );
    assert_eq!(
        exists_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("authorized Form column remains a string")
            .value(0),
        "alpha"
    );

    let cte_source = "WITH c AS (SELECT 1 AS n), d AS (SELECT 2 AS n) SELECT n FROM c";
    let cte_document = ugoite_domain::sql_document::parse_sql_document(cte_source);
    let cte_c = cte_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::CommonTableExpression
                && matches!(node.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "c")
        })
        .expect("the first CTE is typed");
    let query_d = cte_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::Query
                && node.parent.is_some_and(|parent| {
                    cte_document.nodes.get(parent as usize).is_some_and(|cte| {
                        matches!(cte.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "d")
                    })
                })
        })
        .expect("the second CTE body is a typed query");
    let cte_replaced = apply(
        cte_source,
        vec![SqlDocumentSemanticEdit::ReplaceCommonTableExpressionQuery {
            node_id: cte_c.id,
            expected_kind: SqlDocumentNodeKind::CommonTableExpression,
            query_node_id: query_d.id,
        }],
    )?;
    let cte_replaced_batches = context.execute(cte_replaced.document.to_sql()).await?;
    assert_eq!(
        cte_replaced_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("the CTE returns an integer")
            .value(0),
        2
    );
    let cte_d = cte_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::CommonTableExpression
                && matches!(node.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::CommonTableExpression { ref name, .. }) if name.value == "d")
        })
        .expect("the second CTE is typed");
    let cte_moved = apply(
        cte_source,
        vec![SqlDocumentSemanticEdit::MoveCommonTableExpression {
            node_id: cte_d.id,
            expected_kind: SqlDocumentNodeKind::CommonTableExpression,
            to_index: 0,
        }],
    )?;
    let cte_moved_batches = context.execute(cte_moved.document.to_sql()).await?;
    assert_eq!(
        cte_moved_batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("reordered CTEs keep integer output")
            .value(0),
        1
    );

    let set_source = "SELECT 1 AS v UNION ALL SELECT 2 AS v";
    let set_document = ugoite_domain::sql_document::parse_sql_document(set_source);
    let right_operand = set_document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::SetOperationRight)
        .expect("the right set operand is represented");
    let left_select = set_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::Select
                && node
                    .range
                    .and_then(|range| set_document.source_slice(range))
                    == Some("SELECT 1 AS v")
        })
        .expect("the left SELECT is parser-backed");
    let set_replaced = apply(
        set_source,
        vec![SqlDocumentSemanticEdit::ReplaceSetOperationOperand {
            node_id: right_operand.id,
            expected_kind: SqlDocumentNodeKind::SetOperationRight,
            expression_node_id: left_select.id,
            expression_expected_kind: SqlDocumentNodeKind::Select,
        }],
    )?;
    let set_batches = context.execute(set_replaced.document.to_sql()).await?;
    assert_eq!(
        set_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        2
    );
    let values = set_batches
        .iter()
        .flat_map(|batch| {
            batch
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("replaced set operand returns integer values")
                .iter()
                .map(|value| value.expect("set values are non-null"))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(values, vec![1, 1]);

    for quantifier in [
        SqlDocumentSetQuantifier::ByName,
        SqlDocumentSetQuantifier::AllByName,
        SqlDocumentSetQuantifier::DistinctByName,
    ] {
        let source =
            "SELECT 1 AS left_value, 2 AS shared UNION SELECT 3 AS shared, 4 AS right_value";
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        let operation = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SetOperation)
            .expect("the set operation is represented");
        let edited = apply(
            source,
            vec![SqlDocumentSemanticEdit::ReplaceSetOperation {
                node_id: operation.id,
                expected_kind: SqlDocumentNodeKind::SetOperation,
                operator: SqlDocumentSetOperator::Union,
                quantifier,
            }],
        )?;
        let by_name = context.execute(edited.document.to_sql()).await?;
        assert_eq!(
            by_name.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            2
        );
        assert_eq!(by_name[0].num_columns(), 3);
    }

    // The typed relation model remains guarded by the same authorization
    // context when adding a second Form relation through FROM.
    let relation_source = "SELECT tasks.field_100 FROM tasks";
    let relation_document = ugoite_domain::sql_document::parse_sql_document(relation_source);
    let select = relation_document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::Select)
        .expect("the outer SELECT is typed");
    let added_relation = apply(
        relation_source,
        vec![SqlDocumentSemanticEdit::AddFromRelation {
            node_id: select.id,
            expected_kind: SqlDocumentNodeKind::Select,
            factor: SqlDocumentRelationFactor::Table {
                name: vec![SqlDocumentIdentifier {
                    value: "tasks".to_owned(),
                    quote_style: None,
                }],
                alias: Some(SqlDocumentIdentifier {
                    value: "other_tasks".to_owned(),
                    quote_style: None,
                }),
            },
        }],
    )?;
    let added_relation_batches = context.execute(added_relation.document.to_sql()).await?;
    assert_eq!(
        added_relation_batches
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1,
        "a generated relation remains within the authorized Form scope"
    );

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
    // The deterministic held-permit test in query_context::query_permit_queue_tests
    // covers bounded waiting, cancellation, and deadline expiry; this service
    // test verifies that an execution error releases its permit.
    let batches = context
        .execute("SELECT * FROM tasks")
        .await
        .expect("a failed query must release its concurrency permit");
    assert_eq!(
        batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1
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

#[tokio::test]
async fn typed_sql_document_filter_conditions_execute_authorized_queries() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        SqlDocumentConditionGroupOperator, SqlDocumentFilterCondition, SqlDocumentFilterOperator,
        SqlDocumentIdentifier, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
        SqlDocumentSemanticEditTransaction, SqlDocumentSemanticLiteral,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(970)),
        "memory://authorized-query-sql-document-condition-groups",
    )
    .await?;
    let tasks = aggregate_form(971);
    create_form(&workspace, &tasks).await?;
    let revisions = [
        aggregate_revision(&tasks, 972, 982, "food", 10, FieldValue::Null),
        aggregate_revision(&tasks, 973, 983, "food", 10, FieldValue::Null),
        aggregate_revision(&tasks, 974, 984, "travel", 20, FieldValue::Null),
    ];
    append_aggregate_batch(&workspace, revisions.to_vec()).await?;
    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[972, 973, 974]))
        .await?;

    let source = "SELECT field_101 FROM tasks";
    let document = ugoite_domain::sql_document::parse_sql_document(source);
    let select = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::Select)
        .expect("the SELECT scope is represented");
    let comparison = |operator, value: &str| SqlDocumentFilterCondition::Comparison {
        column: ugoite_domain::sql_document::SqlDocumentColumnReference {
            qualifier: None,
            column: SqlDocumentIdentifier {
                value: "field_101".to_owned(),
                quote_style: None,
            },
        },
        operator,
        value: Some(SqlDocumentSemanticLiteral::Number {
            value: value.to_owned(),
        }),
    };
    let edit = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::InsertFilterCondition {
                node_id: select.id,
                expected_kind: select.kind,
                condition: SqlDocumentFilterCondition::Group {
                    operator: SqlDocumentConditionGroupOperator::Or,
                    children: vec![
                        comparison(SqlDocumentFilterOperator::Gte, "20"),
                        comparison(SqlDocumentFilterOperator::Lt, "0"),
                    ],
                },
            }],
        },
    )?;
    let matching = context.execute(edit.document.to_sql()).await?;
    assert_eq!(
        matching.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        1
    );
    assert_eq!(
        matching[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .expect("the selected amount is an integer")
            .value(0),
        20
    );

    let grouped = ugoite_domain::sql_document::parse_sql_document(edit.document.to_sql());
    let ugoite_domain::sql_document::SqlDocumentConditionNode::Group { node_id, .. } = grouped
        .conditions[0]
        .expression
        .as_ref()
        .expect("the nested filter group is represented")
    else {
        panic!("the inserted filter is a typed group");
    };
    let changed = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        edit.document.to_sql(),
        &SqlDocumentSemanticEditTransaction {
            contract_version: grouped.contract_version,
            expected_source: edit.document.to_sql().to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetFilterGroupOperator {
                node_id: *node_id,
                expected_kind: SqlDocumentNodeKind::BinaryExpression,
                operator: SqlDocumentConditionGroupOperator::And,
            }],
        },
    )?;
    let nonmatching = context.execute(changed.document.to_sql()).await?;
    assert_eq!(
        nonmatching
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        0
    );
    Ok(())
}

#[tokio::test]
async fn typed_sql_document_filter_predicate_edits_execute_authorized_queries() -> anyhow::Result<()>
{
    use ugoite_domain::sql_document::{
        SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(9870)),
        "memory://authorized-query-sql-document-filter-predicate-edits",
    )
    .await?;
    let tasks = aggregate_form(9871);
    create_form(&workspace, &tasks).await?;
    let revisions = [
        aggregate_revision(&tasks, 9872, 9882, "food", 10, FieldValue::Null),
        aggregate_revision(&tasks, 9873, 9883, "food", 10, FieldValue::Null),
        aggregate_revision(&tasks, 9874, 9884, "travel", 20, FieldValue::Null),
    ];
    append_aggregate_batch(&workspace, revisions.to_vec()).await?;
    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[9872, 9873, 9874]))
        .await?;

    let source = "SELECT field_101 > 0 FROM tasks WHERE field_101 = 10";
    let document = ugoite_domain::sql_document::parse_sql_document(source);
    let where_clause = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::WhereClause)
        .expect("the source predicate is represented");
    let replacement = document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::BinaryExpression
                && document.source_slice(node.range.unwrap()) == Some("field_101 > 0")
        })
        .expect("the SELECT predicate is a reusable typed expression");
    let replaced = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetFilterPredicate {
                node_id: where_clause.id,
                expected_kind: where_clause.kind,
                expression_node_id: replacement.id,
                expression_expected_kind: replacement.kind,
            }],
        },
    )?;
    let filtered = context.execute(replaced.document.to_sql()).await?;
    assert_eq!(
        filtered.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        3
    );
    let predicate = filtered[0]
        .column(0)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("the typed predicate expression returns booleans");
    assert!(predicate.values().iter().all(|value| value));

    let edited = ugoite_domain::sql_document::parse_sql_document(replaced.document.to_sql());
    let where_clause = edited
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::WhereClause)
        .expect("the replaced WHERE clause remains represented");
    let removed = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        replaced.document.to_sql(),
        &SqlDocumentSemanticEditTransaction {
            contract_version: edited.contract_version,
            expected_source: replaced.document.to_sql().to_owned(),
            edits: vec![SqlDocumentSemanticEdit::RemoveFilterClause {
                node_id: where_clause.id,
                expected_kind: where_clause.kind,
            }],
        },
    )?;
    let unfiltered = context.execute(removed.document.to_sql()).await?;
    assert_eq!(
        unfiltered
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        3
    );
    Ok(())
}

#[tokio::test]
async fn typed_sql_document_group_and_aggregate_edits_execute_authorized_queries(
) -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(970)),
        "memory://authorized-query-sql-document-common-edits",
    )
    .await?;
    let tasks = aggregate_form(971);
    create_form(&workspace, &tasks).await?;
    let revisions = [
        aggregate_revision(&tasks, 972, 982, "food", 10, FieldValue::Null),
        aggregate_revision(&tasks, 973, 983, "food", 10, FieldValue::Null),
        aggregate_revision(&tasks, 974, 984, "travel", 20, FieldValue::Null),
    ];
    append_aggregate_batch(&workspace, revisions.to_vec()).await?;
    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[972, 973, 974]))
        .await?;

    let filter_seed_source = "SELECT COUNT(field_101) FROM tasks";
    let filter_seed_document = ugoite_domain::sql_document::parse_sql_document(filter_seed_source);
    let count = filter_seed_document.nodes.iter().find(|node| {
        node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::FunctionExpression
            && matches!(node.semantic, Some(ugoite_domain::sql_document::SqlDocumentNodeData::Function { ref name, .. })
                if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("count")))
    }).expect("COUNT is represented");
    let filter_seed = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        filter_seed_source,
        &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: filter_seed_document.contract_version,
            expected_source: filter_seed_source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::SetAggregateFilter {
                    node_id: count.id,
                    expected_kind: count.kind,
                    enabled: true,
                },
            ],
        },
    )?
    .document;
    let filter_seed_result = context.execute(filter_seed.to_sql()).await?;
    assert_eq!(
        filter_seed_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        3
    );
    let filter_seed_document =
        ugoite_domain::sql_document::parse_sql_document(filter_seed.to_sql());
    let true_literal = filter_seed_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::LiteralExpression
                && node
                    .range
                    .and_then(|range| filter_seed_document.source_slice(range))
                    == Some("TRUE")
        })
        .expect("the FILTER seed is an editable boolean literal");
    let disabled_filter =
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            filter_seed.to_sql(),
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: filter_seed_document.contract_version,
                expected_source: filter_seed.to_sql().to_owned(),
                edits: vec![
                    ugoite_domain::sql_document::SqlDocumentSemanticEdit::ReplaceLiteral {
                        node_id: true_literal.id,
                        expected_kind: true_literal.kind,
                        literal: ugoite_domain::sql_document::SqlDocumentSemanticLiteral::Boolean {
                            value: false,
                        },
                    },
                ],
            },
        )?
        .document;
    let disabled_result = context.execute(disabled_filter.to_sql()).await?;
    assert_eq!(
        disabled_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        0
    );
    let grouping_source = "SELECT field_100, COUNT(*) FROM tasks";
    let grouping_document = ugoite_domain::sql_document::parse_sql_document(grouping_source);
    let grouping_select = grouping_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::Select)
        .unwrap();
    let grouping_key = grouping_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::IdentifierExpression
                && node
                    .range
                    .and_then(|range| grouping_document.source_slice(range))
                    == Some("field_100")
        })
        .unwrap();
    for (mode, groups) in [
        (
            ugoite_domain::sql_document::SqlDocumentGroupByMode::Expressions,
            vec![vec![grouping_key.id]],
        ),
        (
            ugoite_domain::sql_document::SqlDocumentGroupByMode::All,
            vec![],
        ),
        (
            ugoite_domain::sql_document::SqlDocumentGroupByMode::Rollup,
            vec![vec![grouping_key.id]],
        ),
        (
            ugoite_domain::sql_document::SqlDocumentGroupByMode::Cube,
            vec![vec![grouping_key.id]],
        ),
        (
            ugoite_domain::sql_document::SqlDocumentGroupByMode::GroupingSets,
            vec![vec![grouping_key.id], vec![]],
        ),
    ] {
        let grouped_document =
            ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
                grouping_source,
                &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                    contract_version: grouping_document.contract_version,
                    expected_source: grouping_source.to_owned(),
                    edits: vec![
                        ugoite_domain::sql_document::SqlDocumentSemanticEdit::SetGroupByStructure {
                            node_id: grouping_select.id,
                            expected_kind: grouping_select.kind,
                            mode,
                            groups,
                        },
                    ],
                },
            )?
            .document;
        let grouped = context.execute(grouped_document.to_sql()).await?;
        assert!(!grouped.is_empty(), "{}", grouped_document.to_sql());
        assert!(
            grouped.iter().any(|batch| batch.num_rows() > 0),
            "{}",
            grouped_document.to_sql()
        );
    }

    let apply = |source: &str, edit: ugoite_domain::sql_document::SqlDocumentSemanticEdit| {
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
            source,
            &ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
                contract_version: document.contract_version,
                expected_source: source.to_owned(),
                edits: vec![edit],
            },
        )
        .map(|result| result.document.to_sql().to_owned())
        .map_err(anyhow::Error::from)
    };

    let group_source = "SELECT field_100, SUM(field_101) FROM tasks GROUP BY field_100, field_102";
    let group_document = ugoite_domain::sql_document::parse_sql_document(group_source);
    let group_clause = group_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::GroupByClause)
        .expect("GROUP BY is represented");
    let mut group_items = group_document
        .nodes
        .iter()
        .filter(|node| {
            node.parent == Some(group_clause.id)
                && node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::GroupByItem
        })
        .collect::<Vec<_>>();
    group_items.sort_by_key(|item| item.range.map(|range| range.start));
    let moved_group = apply(
        group_source,
        ugoite_domain::sql_document::SqlDocumentSemanticEdit::MoveGroupByItem {
            node_id: group_items[1].id,
            expected_kind: group_items[1].kind,
            to_index: 0,
        },
    )?;
    let grouped = context.execute(&moved_group).await?;
    assert_eq!(
        grouped.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        2
    );
    let moved_document = ugoite_domain::sql_document::parse_sql_document(&moved_group);
    let group_clause = moved_document
        .nodes
        .iter()
        .find(|node| node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::GroupByClause)
        .expect("reordered GROUP BY is represented");
    let amount_group = moved_document
        .nodes
        .iter()
        .find(|node| {
            node.parent == Some(group_clause.id)
                && node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::GroupByItem
                && moved_document.source_slice(node.range.unwrap()) == Some("field_102")
        })
        .expect("the removable grouping expression is represented");
    let reduced_group = apply(
        &moved_group,
        ugoite_domain::sql_document::SqlDocumentSemanticEdit::RemoveGroupByItem {
            node_id: amount_group.id,
            expected_kind: amount_group.kind,
        },
    )?;
    let grouped = context.execute(&reduced_group).await?;
    assert_eq!(
        grouped.iter().map(|batch| batch.num_rows()).sum::<usize>(),
        2
    );

    let aggregate_source = "SELECT SUM(field_101) FROM tasks";
    let aggregate_document = ugoite_domain::sql_document::parse_sql_document(aggregate_source);
    let sum = aggregate_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::FunctionExpression
                && matches!(
                    node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Function { ref name, .. })
                        if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("sum"))
                )
        })
        .expect("SUM is represented as a registered aggregate");
    let average = apply(
        aggregate_source,
        ugoite_domain::sql_document::SqlDocumentSemanticEdit::SetAggregateFunction {
            node_id: sum.id,
            expected_kind: sum.kind,
            function: ugoite_domain::sql_document::SqlDocumentAggregateFunction::Avg,
        },
    )?;
    let average_batches = context.execute(&average).await?;
    assert_eq!(average_batches[0].num_rows(), 1);
    let average_value = average_batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("AVG returns a typed floating result")
        .value(0);
    assert!((average_value - (40.0 / 3.0)).abs() < 1e-9);

    let average_document = ugoite_domain::sql_document::parse_sql_document(&average);
    let avg = average_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::FunctionExpression
                && matches!(
                    node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Function { ref name, .. })
                        if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("avg"))
                )
        })
        .expect("AVG remains represented");
    let distinct = apply(
        &average,
        ugoite_domain::sql_document::SqlDocumentSemanticEdit::SetAggregateDistinct {
            node_id: avg.id,
            expected_kind: avg.kind,
            distinct: true,
        },
    )?;
    let distinct_batches = context.execute(&distinct).await?;
    let distinct_average = distinct_batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Float64Array>()
        .expect("AVG(DISTINCT ...) remains numeric")
        .value(0);
    assert!((distinct_average - 15.0).abs() < 1e-9);

    let distinct_document = ugoite_domain::sql_document::parse_sql_document(&distinct);
    let avg = distinct_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == ugoite_domain::sql_document::SqlDocumentNodeKind::FunctionExpression
                && matches!(
                    node.semantic,
                    Some(ugoite_domain::sql_document::SqlDocumentNodeData::Function { ref name, .. })
                        if name.last().is_some_and(|part| part.value.eq_ignore_ascii_case("avg"))
                )
        })
        .expect("AVG(DISTINCT ...) is represented");
    let without_aggregate = apply(
        &distinct,
        ugoite_domain::sql_document::SqlDocumentSemanticEdit::RemoveAggregate {
            node_id: avg.id,
            expected_kind: avg.kind,
        },
    )?;
    let unaggregated = context.execute(&without_aggregate).await?;
    assert_eq!(
        unaggregated
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        3
    );
    Ok(())
}

#[tokio::test]
async fn predicate_and_typed_literal_constructors_execute_in_authorized_queries(
) -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentNodeData,
        SqlDocumentNodeKind, SqlDocumentPredicateTemplate, SqlDocumentSemanticEdit,
        SqlDocumentSemanticEditTransaction, SqlDocumentSemanticLiteral,
        SqlDocumentTypedLiteralType,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(9900)),
        "memory://authorized-query-sql-document-predicate-literals",
    )
    .await?;
    let tasks = aggregate_form(9901);
    create_form(&workspace, &tasks).await?;
    append_aggregate_batch(
        &workspace,
        vec![
            aggregate_revision(&tasks, 9902, 9912, "alpha", 10, FieldValue::Null),
            aggregate_revision(&tasks, 9903, 9913, "beta", 20, FieldValue::Null),
        ],
    )
    .await?;
    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[9902, 9903]))
        .await?;

    let construct = |source: &str, edit: SqlDocumentSemanticEdit| {
        let document = parse_sql_document(source);
        apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: document.contract_version,
                expected_source: source.to_owned(),
                edits: vec![edit],
            },
        )
    };
    let cases = [
        (
            "SELECT field_101 FROM tasks",
            SqlDocumentPredicateTemplate::In,
        ),
        (
            "SELECT field_101 FROM tasks",
            SqlDocumentPredicateTemplate::NotIn,
        ),
        (
            "SELECT field_101 FROM tasks",
            SqlDocumentPredicateTemplate::Between,
        ),
        (
            "SELECT field_101 FROM tasks",
            SqlDocumentPredicateTemplate::NotBetween,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::Like,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::NotLike,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::ILike,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::NotILike,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::SimilarTo,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::NotSimilarTo,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::IsNull,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::IsNotNull,
        ),
        (
            "SELECT TRUE FROM tasks",
            SqlDocumentPredicateTemplate::IsTrue,
        ),
        (
            "SELECT TRUE FROM tasks",
            SqlDocumentPredicateTemplate::IsFalse,
        ),
        (
            "SELECT TRUE FROM tasks",
            SqlDocumentPredicateTemplate::IsUnknown,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::IsDistinctFrom,
        ),
        (
            "SELECT field_100 FROM tasks",
            SqlDocumentPredicateTemplate::IsNotDistinctFrom,
        ),
    ];
    for (source, template) in cases {
        let document = parse_sql_document(source);
        let expression = document
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.kind,
                    SqlDocumentNodeKind::IdentifierExpression | SqlDocumentNodeKind::Value
                )
            })
            .expect("the selected expression is parsed");
        let edited = construct(
            source,
            SqlDocumentSemanticEdit::WrapExpressionInPredicate {
                node_id: expression.id,
                expected_kind: expression.kind,
                template,
            },
        )?;
        let batches = context.execute(edited.document.to_sql()).await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            2
        );
    }

    let literal_cases = [
        (SqlDocumentSemanticLiteral::TypedString {
            data_type: SqlDocumentTypedLiteralType::Date,
            value: "2026-02-03".into(),
        },),
        (SqlDocumentSemanticLiteral::TypedString {
            data_type: SqlDocumentTypedLiteralType::Timestamp,
            value: "2026-02-03 13:45:00".into(),
        },),
        (SqlDocumentSemanticLiteral::TypedString {
            data_type: SqlDocumentTypedLiteralType::Interval,
            value: "2".into(),
        },),
        (SqlDocumentSemanticLiteral::HexString {
            value: "BEEF".into(),
        },),
    ];
    for (literal,) in literal_cases {
        let source = "SELECT 1 FROM tasks";
        let document = parse_sql_document(source);
        let value = document
            .nodes
            .iter()
            .find(|node| matches!(node.semantic, Some(SqlDocumentNodeData::Literal { .. })))
            .expect("the seed literal is parsed");
        let edited = construct(
            source,
            SqlDocumentSemanticEdit::ReplaceLiteral {
                node_id: value.id,
                expected_kind: value.kind,
                literal,
            },
        )?;
        let batches = context.execute(edited.document.to_sql()).await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            2
        );
    }
    Ok(())
}

#[tokio::test]
async fn semi_and_anti_join_constructors_execute_in_authorized_queries() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentIdentifier,
        SqlDocumentJoin, SqlDocumentJoinConstraint, SqlDocumentJoinOperator, SqlDocumentNodeKind,
        SqlDocumentRelationFactor, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(9910)),
        "memory://authorized-query-sql-document-semi-anti-joins",
    )
    .await?;
    let tasks = aggregate_form(9911);
    create_form(&workspace, &tasks).await?;
    append_aggregate_batch(
        &workspace,
        vec![
            aggregate_revision(&tasks, 9912, 9922, "alpha", 10, FieldValue::Null),
            aggregate_revision(&tasks, 9913, 9923, "beta", 20, FieldValue::Null),
        ],
    )
    .await?;
    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[9912, 9913]))
        .await?;

    for (operator, expected_rows) in [
        (SqlDocumentJoinOperator::LeftSemi, 2),
        (SqlDocumentJoinOperator::RightSemi, 2),
        (SqlDocumentJoinOperator::LeftAnti, 0),
        (SqlDocumentJoinOperator::RightAnti, 0),
    ] {
        let source = "SELECT 1 FROM tasks AS l WHERE 1 = 1";
        let document = parse_sql_document(source);
        let tree = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::RelationTree)
            .expect("the left relation has a typed relation tree");
        let condition = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("the equality condition is a typed expression");
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: document.contract_version,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::AddJoin {
                    node_id: tree.id,
                    expected_kind: tree.kind,
                    join: SqlDocumentJoin {
                        operator,
                        relation: SqlDocumentRelationFactor::Table {
                            name: vec![SqlDocumentIdentifier {
                                value: "tasks".into(),
                                quote_style: None,
                            }],
                            alias: Some(SqlDocumentIdentifier {
                                value: "r".into(),
                                quote_style: None,
                            }),
                        },
                        constraint: SqlDocumentJoinConstraint::On {
                            expression_node_id: condition.id,
                            expression_expected_kind: condition.kind,
                        },
                    },
                }],
            },
        )?;
        let batches = context.execute(edited.document.to_sql()).await?;
        assert_eq!(
            batches.iter().map(|batch| batch.num_rows()).sum::<usize>(),
            expected_rows,
            "{operator:?}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn expression_and_parameter_limit_constructors_execute_in_authorized_queries(
) -> anyhow::Result<()> {
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(9930)),
        "memory://authorized-query-sql-document-limit-expressions",
    )
    .await?;
    let tasks = aggregate_form(9931);
    create_form(&workspace, &tasks).await?;
    append_aggregate_batch(
        &workspace,
        vec![
            aggregate_revision(&tasks, 9932, 9942, "alpha", 10, FieldValue::Null),
            aggregate_revision(&tasks, 9933, 9943, "beta", 20, FieldValue::Null),
        ],
    )
    .await?;
    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[9932, 9933]))
        .await?;

    let expression = context
        .execute("SELECT field_100 FROM tasks LIMIT 1 + 1")
        .await?;
    assert_eq!(
        expression
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        2
    );

    let parameter = context
        .execute_with_parameters(
            "SELECT field_100 FROM tasks LIMIT $rows",
            HashMap::from([(
                "rows".to_owned(),
                datafusion::scalar::ScalarValue::Int64(Some(1)),
            )]),
        )
        .await?;
    assert_eq!(
        parameter
            .iter()
            .map(|batch| batch.num_rows())
            .sum::<usize>(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn projection_reorder_keeps_authorized_grouping_and_sort_meaning() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_edit_transaction, apply_sql_document_semantic_edit_transaction,
        parse_sql_document, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
        SqlDocumentSemanticEditTransaction,
    };
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(501)),
        "memory://authorized-projection-position",
    )
    .await?;
    let tasks = form(502, "Tasks");
    create_form(&workspace, &tasks).await?;
    for (entry, title) in [(503, "zeta"), (505, "alpha")] {
        append(&workspace, &tasks, entry, entry + 1, title).await?;
    }
    let mut authorized_policy = policy(&tasks, &[503, 505]);
    authorized_policy.limits.function_policy = SqlFunctionPolicy::ReadOnlyAggregateV1;
    let context = workspace
        .authorized_query_context(authorized_policy)
        .await?;
    for source in [
        "SELECT field_100 AS title, 1 AS n FROM tasks ORDER BY 1",
        "SELECT field_100 AS title, COUNT(*) AS n FROM tasks GROUP BY (1) ORDER BY 1",
    ] {
        let document = parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: document.contract_version,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::MoveProjectionItem {
                    node_id: item.id,
                    expected_kind: item.kind,
                    to_index: 1,
                }],
            },
        )?;
        assert!(edited.document.source.ends_with("ORDER BY 2"));
        let rows = context.execute(edited.document.to_sql()).await?;
        let titles = rows
            .iter()
            .flat_map(|batch| {
                batch
                    .column(1)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .unwrap()
                    .iter()
                    .map(|value| value.unwrap().to_owned())
            })
            .collect::<Vec<_>>();
        assert_eq!(titles, ["alpha", "zeta"]);
        assert_eq!(rows.iter().map(|batch| batch.num_rows()).sum::<usize>(), 2);
        assert_eq!(
            apply_sql_document_edit_transaction(edited.document.to_sql(), &edited.undo)?
                .document
                .source,
            source
        );
    }
    Ok(())
}

#[tokio::test]
async fn projection_alias_edit_executes_with_the_authored_output_name() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentIdentifier,
        SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
    };
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(511)),
        "memory://authorized-projection-alias",
    )
    .await?;
    let tasks = form(512, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 513, 514, "zeta").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[513]))
        .await?;
    for source in [
        "SELECT field_100 AS TOTAL FROM tasks ORDER BY \"total\"",
        "SELECT field_100 AS \"total\" FROM tasks ORDER BY TOTAL",
    ] {
        let document = parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::SelectItem)
            .unwrap();
        let edited = apply_sql_document_semantic_edit_transaction(
            source,
            &SqlDocumentSemanticEditTransaction {
                contract_version: document.contract_version,
                expected_source: source.to_owned(),
                edits: vec![SqlDocumentSemanticEdit::SetProjectionAlias {
                    node_id: item.id,
                    expected_kind: item.kind,
                    alias: Some(SqlDocumentIdentifier {
                        value: "task title".into(),
                        quote_style: Some('"'),
                    }),
                }],
            },
        )?;
        let rows = context.execute(edited.document.to_sql()).await?;
        assert_eq!(rows[0].schema().field(0).name(), "task title");
        let values = rows[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(values.iter().flatten().collect::<Vec<_>>(), ["zeta"]);
        assert_eq!(
            edited.document.to_sql(),
            "SELECT field_100 AS \"task title\" FROM tasks ORDER BY \"task title\""
        );
    }

    let case_distinct = context
        .execute("SELECT field_100 AS \"TOTAL\" FROM tasks ORDER BY total")
        .await;
    assert!(case_distinct.is_err(), "quoted case remains significant");
    Ok(())
}

#[tokio::test]
async fn simple_case_operand_edit_executes_in_authorized_query_context() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentNodeKind,
        SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
    };
    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(921)),
        "memory://authorized-simple-case-operand",
    )
    .await?;
    let tasks = form(922, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 923, 924, "zeta").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[923]))
        .await?;

    let source = "SELECT field_100, CASE WHEN 'zeta' THEN 10 ELSE 0 END FROM tasks";
    let document = parse_sql_document(source);
    let case_node = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::CaseExpression)
        .expect("the SQL Document exposes the searched CASE");
    let operand = document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::IdentifierExpression
                && node.parent.is_some_and(|parent| {
                    document.nodes[parent as usize].kind == SqlDocumentNodeKind::SelectItem
                })
                && document
                    .source_slice(node.range.unwrap())
                    .is_some_and(|source| source.eq_ignore_ascii_case("field_100"))
        })
        .expect("the sibling projection provides a same-scope CASE operand");
    let edited = apply_sql_document_semantic_edit_transaction(
        source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetCaseOperand {
                node_id: case_node.id,
                expected_kind: case_node.kind,
                expression_node_id: Some(operand.id),
                expression_expected_kind: Some(operand.kind),
            }],
        },
    )?;
    assert!(edited.document.to_sql().contains("CASE field_100 WHEN"));
    let rows = context.execute(edited.document.to_sql()).await?;
    assert_eq!(rows.iter().map(|batch| batch.num_rows()).sum::<usize>(), 1);
    let values = rows[0]
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("the CASE expression returns an integer");
    assert_eq!(values.value(0), 10);
    Ok(())
}

#[tokio::test]
async fn typed_sql_document_array_index_executes_authorized_queries() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentNodeKind,
        SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction, SqlDocumentSemanticLiteral,
        SqlDocumentValueAccess,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(925)),
        "memory://authorized-value-access",
    )
    .await?;
    let mut tasks = form(926, "Tasks");
    tasks.fields.push(FormField {
        id: FieldId::new(101).unwrap(),
        name: "attributes".into(),
        field_type: FieldType::ObjectList,
        required: false,
        label: None,
        description: None,
        semantic_role: None,
        reference_form: None,
        list_item: None,
        validation: None,
        enum_values: Vec::new(),
        deprecated: false,
    });
    create_form(&workspace, &tasks).await?;
    let mut revision = revision(&tasks, 927, 928, "one row");
    revision.values.insert(
        FieldId::new(101).unwrap(),
        FieldValue::List(vec![FieldValue::Object(BTreeMap::from([
            ("type".into(), FieldValue::String("person".into())),
            ("name".into(), FieldValue::String("Ada".into())),
            (
                "description".into(),
                FieldValue::String("researcher".into()),
            ),
        ]))]),
    );
    workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.value-access-entry",
            &revision,
        )?)?
        .append_revisions(tasks.id, vec![revision])
        .await?;
    let mut authorized = policy(&tasks, &[927]);
    authorized
        .forms
        .get_mut(&tasks.id)
        .expect("the Form is authorized")
        .columns
        .insert("field_101".into());
    let context = workspace.authorized_query_context(authorized).await?;

    let array_source = "SELECT field_101 FROM tasks";
    let array_document = parse_sql_document(array_source);
    let array_value = array_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::IdentifierExpression
                && node
                    .range
                    .and_then(|range| array_document.source_slice(range))
                    == Some("field_101")
        })
        .expect("the Form list value is an editable source expression");
    let indexed = apply_sql_document_semantic_edit_transaction(
        array_source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: array_document.contract_version,
            expected_source: array_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                node_id: array_value.id,
                expected_kind: array_value.kind,
                access: SqlDocumentValueAccess::ArrayIndex {
                    one_based_index: "1".to_owned(),
                },
            }],
        },
    )?;
    assert_eq!(
        indexed.document.to_sql(),
        "SELECT (field_101)[1] FROM tasks"
    );
    let rows = context.execute(indexed.document.to_sql()).await?;
    let batch = rows.first().expect("the indexed list returns a row");
    assert_eq!(batch.num_rows(), 1);
    let values = batch
        .column(0)
        .as_any()
        .downcast_ref::<arrow_array::StructArray>()
        .expect("one-based list indexing returns the nested struct");
    assert_eq!(
        values
            .column_by_name("name")
            .and_then(|column| column.as_any().downcast_ref::<StringArray>())
            .expect("the struct retains its named field")
            .value(0),
        "Ada"
    );

    let scalar_source = "SELECT 4 FROM tasks";
    let scalar_document = parse_sql_document(scalar_source);
    let scalar = scalar_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::Value
                && node
                    .range
                    .and_then(|range| scalar_document.source_slice(range))
                    == Some("4")
        })
        .expect("the array element seed is a typed literal");
    let array_query = apply_sql_document_semantic_edit_transaction(
        scalar_source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: scalar_document.contract_version,
            expected_source: scalar_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInArray {
                node_id: scalar.id,
                expected_kind: scalar.kind,
            }],
        },
    )?;
    assert_eq!(array_query.document.to_sql(), "SELECT ARRAY[4] FROM tasks");
    let array_document = parse_sql_document(&array_query.document.source);
    let array_expression = array_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::SelectItem
                && node
                    .range
                    .and_then(|range| array_document.source_slice(range))
                    == Some("ARRAY[4]")
        })
        .expect("the constructed ARRAY remains selectable");
    let array_query = apply_sql_document_semantic_edit_transaction(
        &array_query.document.source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: array_document.contract_version,
            expected_source: array_query.document.source.clone(),
            edits: vec![SqlDocumentSemanticEdit::AppendArrayLiteralElement {
                node_id: array_expression.id,
                expected_kind: array_expression.kind,
                literal: SqlDocumentSemanticLiteral::Number {
                    value: "9".to_owned(),
                },
            }],
        },
    )?;
    assert_eq!(
        array_query.document.to_sql(),
        "SELECT ARRAY[4, 9] FROM tasks"
    );
    let array_batches = context.execute(array_query.document.to_sql()).await?;
    let array = array_batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("the typed ARRAY expression returns an integer list");
    let list_values = array.value(0);
    let values = list_values
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("the list contains Int64 values");
    assert_eq!(values.len(), 2);
    assert_eq!(values.value(0), 4);
    assert_eq!(values.value(1), 9);

    let empty_array_batches = context
        .execute("SELECT CAST(ARRAY[] AS BIGINT[]) AS empty")
        .await?;
    let empty_array = empty_array_batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("a typed empty ARRAY returns an integer list");
    assert_eq!(empty_array.value(0).len(), 0);
    let inferred_empty_array_batches = context.execute("SELECT ARRAY[] AS empty").await?;
    let inferred_empty_array = inferred_empty_array_batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("an untyped empty ARRAY is accepted by the authorized executor");
    assert_eq!(inferred_empty_array.value(0).len(), 0);

    Ok(())
}
