use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use arrow_array::{BooleanArray, Int64Array, ListArray, StringArray};
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
        "memory://authorized-query-edited-join",
    )
    .await?;
    let tasks = form(36, "Tasks");
    let related = form(43, "Related tasks");
    create_form(&workspace, &tasks).await?;
    create_form(&workspace, &related).await?;
    append(&workspace, &tasks, 37, 38, "matching").await?;
    append(&workspace, &tasks, 39, 40, "matching").await?;
    append(&workspace, &related, 44, 45, "matching").await?;
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
    let denied = workspace
        .authorized_query_context(policy(&tasks, &[37]))
        .await?;
    assert!(
        denied.execute(added_join.document.to_sql()).await.is_err(),
        "typed relation edits cannot expose a Form outside the authorized snapshot"
    );
    Ok(())
}

#[tokio::test]
async fn parameterized_cast_target_edit_executes_exact_decimal_result() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentCastTarget,
        SqlDocumentCastType, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
        SqlDocumentSemanticEditTransaction,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(950)),
        "memory://authorized-parameterized-cast-target",
    )
    .await?;
    let tasks = form(951, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 952, 953, "row").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[952]))
        .await?;

    let source = "SELECT CAST(7 AS DECIMAL) FROM tasks";
    let document = parse_sql_document(source);
    let cast = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
        .expect("the SQL Document exposes the directly written CAST");
    let edited = apply_sql_document_semantic_edit_transaction(
        source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetParameterizedCastTarget {
                node_id: cast.id,
                expected_kind: cast.kind,
                target: SqlDocumentCastTarget {
                    kind: SqlDocumentCastType::Decimal,
                    precision: Some("4".to_owned()),
                    scale: Some("2".to_owned()),
                    length: None,
                },
            }],
        },
    )?;
    assert_eq!(
        edited.document.to_sql(),
        "SELECT CAST(7 AS DECIMAL(4, 2)) FROM tasks"
    );

    let batches = context.execute(edited.document.to_sql()).await?;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);
    assert_eq!(
        batches[0].column(0).data_type(),
        &arrow_schema::DataType::Decimal128(4, 2)
    );
    assert_eq!(
        batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Decimal128Array>()
            .expect("DECIMAL(4, 2) returns Decimal128")
            .value(0),
        700,
        "7 is returned as 7.00 with precision 4 and scale 2"
    );
    Ok(())
}

#[tokio::test]
async fn timestamp_cast_precision_edit_executes_exact_millisecond_result() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentCastTarget,
        SqlDocumentCastType, SqlDocumentNodeKind, SqlDocumentSemanticEdit,
        SqlDocumentSemanticEditTransaction,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(960)),
        "memory://authorized-timestamp-cast-precision",
    )
    .await?;
    let tasks = form(961, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 962, 963, "row").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[962]))
        .await?;

    let source = "SELECT CAST('2026-01-02 03:04:05.123000' AS TIMESTAMP) FROM tasks";
    let document = parse_sql_document(source);
    let cast = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::CastExpression)
        .expect("the SQL Document exposes the directly written CAST");
    let edited = apply_sql_document_semantic_edit_transaction(
        source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetParameterizedCastTarget {
                node_id: cast.id,
                expected_kind: cast.kind,
                target: SqlDocumentCastTarget {
                    kind: SqlDocumentCastType::Timestamp,
                    precision: Some("3".to_owned()),
                    scale: None,
                    length: None,
                },
            }],
        },
    )?;
    assert_eq!(
        edited.document.to_sql(),
        "SELECT CAST('2026-01-02 03:04:05.123000' AS TIMESTAMP(3)) FROM tasks"
    );

    let batches = context.execute(edited.document.to_sql()).await?;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);
    assert_eq!(
        batches[0].column(0).data_type(),
        &arrow_schema::DataType::Timestamp(arrow_schema::TimeUnit::Millisecond, None)
    );
    assert_eq!(
        batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::TimestampMillisecondArray>()
            .expect("TIMESTAMP(3) returns millisecond timestamps")
            .value(0),
        1_767_323_045_123,
        "the timestamp is preserved to exactly 123 milliseconds"
    );
    Ok(())
}

#[tokio::test]
async fn typed_sql_document_nested_structure_edits_execute_authorized_queries() -> anyhow::Result<()>
{
    use ugoite_domain::sql_document::{
        SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
        SqlDocumentSubqueryPredicateTemplate,
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
    let filter_scope = ugoite_domain::sql_document::parse_sql_document(filter_seed.to_sql())
        .conditions
        .into_iter()
        .find(|scope| {
            scope.kind
                == ugoite_domain::sql_document::SqlDocumentConditionScopeKind::AggregateFilter
        })
        .expect("aggregate FILTER has a typed condition scope");
    let seed_id = match filter_scope.expression.expect("FILTER expression") {
        ugoite_domain::sql_document::SqlDocumentConditionNode::Advanced { node_id, .. } => node_id,
        _ => panic!("FILTER starts at TRUE"),
    };
    let predicate_transaction = |source: &str, node_id: u32, value: &str| {
        let document = ugoite_domain::sql_document::parse_sql_document(source);
        ugoite_domain::sql_document::SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![
                ugoite_domain::sql_document::SqlDocumentSemanticEdit::UpdateFilterCondition {
                    node_id,
                    expected_kind: document.nodes[node_id as usize].kind,
                    condition:
                        ugoite_domain::sql_document::SqlDocumentFilterCondition::Comparison {
                            column: ugoite_domain::sql_document::SqlDocumentColumnReference {
                                qualifier: None,
                                column: ugoite_domain::sql_document::SqlDocumentIdentifier {
                                    value: "field_100".to_owned(),
                                    quote_style: None,
                                },
                            },
                            operator: ugoite_domain::sql_document::SqlDocumentFilterOperator::Eq,
                            value: Some(
                                ugoite_domain::sql_document::SqlDocumentSemanticLiteral::String {
                                    value: value.to_owned(),
                                },
                            ),
                        },
                },
            ],
        }
    };
    let food_predicate = ugoite_domain::sql_document::apply_sql_document_semantic_edit_transaction(
        filter_seed.to_sql(),
        &predicate_transaction(filter_seed.to_sql(), seed_id, "food"),
    )?
    .document;
    assert_eq!(
        food_predicate.to_sql(),
        "SELECT COUNT(field_101) FILTER (WHERE field_100 = 'food') FROM tasks"
    );
    let food_result = context.execute(food_predicate.to_sql()).await?;
    assert_eq!(
        food_result[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap()
            .value(0),
        2
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
async fn typed_not_bitwise_and_shift_edits_execute_authorized_results() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document,
        SqlDocumentBinaryOperator, SqlDocumentNodeData, SqlDocumentNodeKind,
        SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction, SqlDocumentUnarySign,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(9_910)),
        "memory://authorized-query-typed-operators",
    )
    .await?;
    let tasks = aggregate_form(9_911);
    create_form(&workspace, &tasks).await?;
    append_aggregate(
        &workspace,
        aggregate_revision(&tasks, 9_912, 9_913, "alpha", 10, FieldValue::Null),
    )
    .await?;
    let context = workspace
        .authorized_query_context(aggregate_policy(&tasks, &[9_912]))
        .await?;

    let apply = |source: &str, edit: SqlDocumentSemanticEdit| {
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

    let source = "SELECT - TRUE FROM tasks";
    let document = parse_sql_document(source);
    let unary = document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::UnaryExpression
                && matches!(
                    node.semantic,
                    Some(SqlDocumentNodeData::UnaryOperator { .. })
                )
        })
        .expect("typed unary operation is present");
    let edited = apply(
        source,
        SqlDocumentSemanticEdit::ReplaceUnarySign {
            node_id: unary.id,
            expected_kind: SqlDocumentNodeKind::UnaryExpression,
            sign: SqlDocumentUnarySign::Not,
        },
    )?;
    assert_eq!(edited.document.to_sql(), "SELECT NOT TRUE FROM tasks");
    let result = context.execute(edited.document.to_sql()).await?;
    assert!(!result[0]
        .column(0)
        .as_any()
        .downcast_ref::<BooleanArray>()
        .expect("boolean NOT returns a Boolean")
        .value(0));

    for (operator, expected_sql, expected) in [
        (
            SqlDocumentBinaryOperator::BitwiseAnd,
            "SELECT 5 & 1 FROM tasks",
            1,
        ),
        (
            SqlDocumentBinaryOperator::BitwiseOr,
            "SELECT 5 | 1 FROM tasks",
            5,
        ),
        (
            SqlDocumentBinaryOperator::ShiftLeft,
            "SELECT 5 << 1 FROM tasks",
            10,
        ),
        (
            SqlDocumentBinaryOperator::ShiftRight,
            "SELECT 5 >> 1 FROM tasks",
            2,
        ),
    ] {
        let source = "SELECT 5 + 1 FROM tasks";
        let document = parse_sql_document(source);
        let binary = document
            .nodes
            .iter()
            .find(|node| node.kind == SqlDocumentNodeKind::BinaryExpression)
            .expect("typed binary operation is present");
        let edited = apply(
            source,
            SqlDocumentSemanticEdit::ReplaceBinaryOperator {
                node_id: binary.id,
                expected_kind: SqlDocumentNodeKind::BinaryExpression,
                operator,
            },
        )?;
        assert_eq!(edited.document.to_sql(), expected_sql);
        let result = context.execute(edited.document.to_sql()).await?;
        assert_eq!(
            result[0]
                .column(0)
                .as_any()
                .downcast_ref::<Int64Array>()
                .expect("bitwise and shift operators return Int64")
                .value(0),
            expected,
            "{} has the authorized result",
            expected_sql,
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
async fn cte_output_column_alias_rename_executes_with_authored_result_name() -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentIdentifier,
        SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(970)),
        "memory://authorized-cte-output-column-alias",
    )
    .await?;
    let tasks = form(971, "Tasks");
    create_form(&workspace, &tasks).await?;
    append(&workspace, &tasks, 972, 973, "row").await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[972]))
        .await?;

    let source = "WITH c (old_name) AS (SELECT 1) SELECT c.old_name FROM c";
    let document = parse_sql_document(source);
    let cte = document
        .nodes
        .iter()
        .find(|node| node.kind == SqlDocumentNodeKind::CommonTableExpression)
        .expect("the SQL Document exposes the CTE");
    let renamed = apply_sql_document_semantic_edit_transaction(
        source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetRelationColumnAliases {
                node_id: cte.id,
                expected_kind: cte.kind,
                aliases: vec![SqlDocumentIdentifier {
                    value: "new_name".to_owned(),
                    quote_style: None,
                }],
            }],
        },
    )?;
    assert_eq!(
        renamed.document.to_sql(),
        "WITH c (new_name) AS (SELECT 1) SELECT c.new_name FROM c"
    );

    let batches = context.execute(renamed.document.to_sql()).await?;
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].num_rows(), 1);
    assert_eq!(batches[0].schema().field(0).name(), "new_name");
    let result = batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("the CTE output value remains Int64");
    assert_eq!(result.values(), &[1]);
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
        "WITH nested AS (SELECT field_100 AS gross FROM tasks) SELECT gross AS TOTAL FROM nested ORDER BY \"total\" /* trailing */",
    ] {
        let document = parse_sql_document(source);
        let item = document
            .nodes
            .iter()
            .find(|node| {
                node.kind == SqlDocumentNodeKind::SelectItem
                    && node
                        .range
                        .and_then(|range| document.source_slice(range))
                        .is_some_and(|projection| {
                            projection.ends_with("AS TOTAL")
                                || projection.ends_with("AS \"total\"")
                        })
            })
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
        assert!(edited.document.to_sql().contains("AS \"task title\""));
        assert!(edited.document.to_sql().contains("ORDER BY \"task title\""));
        assert!(edited.document.to_sql().ends_with("/* trailing */") || !source.contains("trailing"));
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
async fn typed_sql_document_aliased_array_removal_executes_authorized_query() -> anyhow::Result<()>
{
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentNodeKind,
        SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
    };

    let workspace = IcebergWorkspace::memory_for_tests(
        SpaceId::from(Uuid::from_u128(939)),
        "memory://authorized-aliased-array-removal",
    )
    .await?;
    let tasks = form(940, "Tasks");
    create_form(&workspace, &tasks).await?;
    let revision = revision(&tasks, 941, 942, "one row");
    workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.aliased-array-removal-entry",
            &revision,
        )?)?
        .append_revisions(tasks.id, vec![revision])
        .await?;
    let context = workspace
        .authorized_query_context(policy(&tasks, &[941]))
        .await?;

    let source = "SELECT ARRAY[4] AS values_out, 7 AS untouched FROM tasks";
    let document = parse_sql_document(source);
    let element = document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::LiteralExpression
                && node.range.and_then(|range| document.source_slice(range)) == Some("4")
        })
        .expect("the direct aliased ARRAY element is represented");
    let removed = apply_sql_document_semantic_edit_transaction(
        source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: document.contract_version,
            expected_source: source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::RemoveArrayLiteralElement {
                node_id: element.id,
                expected_kind: element.kind,
            }],
        },
    )?;
    assert_eq!(
        removed.document.to_sql(),
        "SELECT ARRAY[] AS values_out, 7 AS untouched FROM tasks"
    );
    let rows = context.execute(removed.document.to_sql()).await?;
    let batch = rows.first().expect("the removed ARRAY query returns a row");
    assert_eq!(batch.schema().field(0).name(), "values_out");
    let values = batch
        .column(0)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("the aliased ARRAY returns a list")
        .value(0);
    assert_eq!(values.len(), 0);
    Ok(())
}

#[tokio::test]
async fn typed_sql_document_array_slice_bounds_and_aliased_reorder_execute_authorized_query(
) -> anyhow::Result<()> {
    use ugoite_domain::sql_document::{
        apply_sql_document_semantic_edit_transaction, parse_sql_document, SqlDocumentNodeData,
        SqlDocumentNodeKind, SqlDocumentSemanticEdit, SqlDocumentSemanticEditTransaction,
        SqlDocumentSemanticLiteral, SqlDocumentValueAccess,
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
        FieldValue::List(vec![
            FieldValue::Object(BTreeMap::from([
                ("type".into(), FieldValue::String("person".into())),
                ("name".into(), FieldValue::String("Ada".into())),
                (
                    "description".into(),
                    FieldValue::String("researcher".into()),
                ),
            ])),
            FieldValue::Object(BTreeMap::from([
                ("type".into(), FieldValue::String("person".into())),
                ("name".into(), FieldValue::String("Grace".into())),
                (
                    "description".into(),
                    FieldValue::String("mathematician".into()),
                ),
            ])),
        ]),
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

    let field_source = "SELECT field_101[1] FROM tasks";
    let field_document = parse_sql_document(field_source);
    let struct_value = field_document
        .nodes
        .iter()
        .find(|node| {
            node.range
                .and_then(|range| field_document.source_slice(range))
                == Some("field_101[1]")
        })
        .expect("the indexed Struct is an editable source expression");
    let named_field = apply_sql_document_semantic_edit_transaction(
        field_source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: field_document.contract_version,
            expected_source: field_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                node_id: struct_value.id,
                expected_kind: struct_value.kind,
                access: SqlDocumentValueAccess::StructField {
                    name: "name".to_owned(),
                },
            }],
        },
    )?;
    assert_eq!(
        named_field.document.to_sql(),
        "SELECT (field_101[1])['name'] FROM tasks"
    );
    let named_rows = context.execute(named_field.document.to_sql()).await?;
    let named_values = named_rows[0]
        .column(0)
        .as_any()
        .downcast_ref::<StringArray>()
        .expect("named Struct field access returns its string value");
    assert_eq!(named_values.value(0), "Ada");

    let slice_source = "SELECT field_101 FROM tasks";
    let slice_document = parse_sql_document(slice_source);
    let list_value = slice_document
        .nodes
        .iter()
        .find(|node| {
            node.kind == SqlDocumentNodeKind::IdentifierExpression
                && node
                    .range
                    .and_then(|range| slice_document.source_slice(range))
                    == Some("field_101")
        })
        .expect("the authorized Form list value is an editable source expression");
    let sliced = apply_sql_document_semantic_edit_transaction(
        slice_source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: slice_document.contract_version,
            expected_source: slice_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::WrapExpressionInValueAccess {
                node_id: list_value.id,
                expected_kind: list_value.kind,
                access: SqlDocumentValueAccess::ArraySlice {
                    one_based_start: "1".to_owned(),
                    one_based_end: "2".to_owned(),
                },
            }],
        },
    )?;
    assert_eq!(
        sliced.document.to_sql(),
        "SELECT (field_101)[1:2] FROM tasks"
    );
    let sliced_rows = context.execute(sliced.document.to_sql()).await?;
    let sliced_list = sliced_rows[0]
        .column(0)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("authorized fixed-bound array slicing returns a list");
    let sliced_item = sliced_list.value(0);
    assert_eq!(sliced_item.len(), 2);
    let sliced_values = sliced_item
        .as_any()
        .downcast_ref::<arrow_array::StructArray>()
        .expect("the slice retains the list's struct values");
    let sliced_names = sliced_values
        .column_by_name("name")
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .expect("the slice retains its named field");
    assert_eq!(sliced_names.value(0), "Ada");
    assert_eq!(sliced_names.value(1), "Grace");

    let existing_slice_source = sliced.document.to_sql();
    let existing_slice_document = parse_sql_document(existing_slice_source);
    let existing_slice = existing_slice_document
        .nodes
        .iter()
        .find(|node| {
            node.semantic.as_ref().is_some_and(|semantic| {
                matches!(
                    semantic,
                    ugoite_domain::sql_document::SqlDocumentNodeData::ArraySlice { .. }
                )
            })
        })
        .expect("the parsed SQL exposes typed existing slice bounds");
    let changed_slice = apply_sql_document_semantic_edit_transaction(
        existing_slice_source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: existing_slice_document.contract_version,
            expected_source: existing_slice_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::SetArraySliceBounds {
                node_id: existing_slice.id,
                expected_kind: existing_slice.kind,
                one_based_start: "2".to_owned(),
                one_based_end: "2".to_owned(),
            }],
        },
    )?;
    assert_eq!(
        changed_slice.document.to_sql(),
        "SELECT (field_101)[2:2] FROM tasks"
    );
    let changed_rows = context.execute(changed_slice.document.to_sql()).await?;
    let changed_list = changed_rows[0]
        .column(0)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("the edited authorized slice returns a list");
    let changed_item = changed_list.value(0);
    assert_eq!(changed_item.len(), 1);
    let changed_values = changed_item
        .as_any()
        .downcast_ref::<arrow_array::StructArray>()
        .expect("the selected slice element retains its struct");
    let changed_names = changed_values
        .column_by_name("name")
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .expect("the selected slice element retains its named field");
    assert_eq!(changed_names.value(0), "Grace");

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
    let array_document = parse_sql_document(&array_query.document.source);
    let last_array_element = array_document
        .nodes
        .iter()
        .find(|node| {
            node.parent
                .and_then(|parent_id| array_document.nodes.get(parent_id as usize))
                .is_some_and(|parent| {
                    matches!(parent.semantic, Some(SqlDocumentNodeData::ArrayLiteral))
                })
                && node
                    .range
                    .and_then(|range| array_document.source_slice(range))
                    == Some("9")
        })
        .expect("the final array element can be selected for reordering");
    let array_query = apply_sql_document_semantic_edit_transaction(
        &array_query.document.source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: array_document.contract_version,
            expected_source: array_query.document.source.clone(),
            edits: vec![SqlDocumentSemanticEdit::MoveArrayLiteralElement {
                node_id: last_array_element.id,
                expected_kind: last_array_element.kind,
                move_up: true,
            }],
        },
    )?;
    assert_eq!(
        array_query.document.to_sql(),
        "SELECT ARRAY[9, 4] FROM tasks"
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
    assert_eq!(values.value(0), 9);
    assert_eq!(values.value(1), 4);

    let aliased_source = "SELECT ARRAY[4, 9] AS ordered_values FROM tasks";
    let aliased_document = parse_sql_document(aliased_source);
    let aliased_array = aliased_document
        .nodes
        .iter()
        .find(|node| matches!(node.semantic, Some(SqlDocumentNodeData::ArrayLiteral)))
        .expect("the aliased ARRAY expression is represented");
    let aliased_element = aliased_document
        .nodes
        .iter()
        .find(|node| {
            node.parent == Some(aliased_array.id)
                && node
                    .range
                    .and_then(|range| aliased_document.source_slice(range))
                    == Some("9")
        })
        .expect("the aliased ARRAY's second direct element is represented");
    let aliased_reorder = apply_sql_document_semantic_edit_transaction(
        aliased_source,
        &SqlDocumentSemanticEditTransaction {
            contract_version: aliased_document.contract_version,
            expected_source: aliased_source.to_owned(),
            edits: vec![SqlDocumentSemanticEdit::MoveArrayLiteralElement {
                node_id: aliased_element.id,
                expected_kind: aliased_element.kind,
                move_up: true,
            }],
        },
    )?;
    assert_eq!(
        aliased_reorder.document.to_sql(),
        "SELECT ARRAY[9, 4] AS ordered_values FROM tasks"
    );
    let aliased_batches = context.execute(aliased_reorder.document.to_sql()).await?;
    assert_eq!(
        aliased_batches[0].schema().field(0).name(),
        "ordered_values"
    );
    let aliased_array = aliased_batches[0]
        .column(0)
        .as_any()
        .downcast_ref::<ListArray>()
        .expect("the alias-preserving edit returns an integer list");
    let aliased_list_values = aliased_array.value(0);
    let aliased_values = aliased_list_values
        .as_any()
        .downcast_ref::<Int64Array>()
        .expect("the aliased list contains Int64 values");
    assert_eq!(aliased_values.len(), 2);
    assert_eq!(aliased_values.value(0), 9);
    assert_eq!(aliased_values.value(1), 4);

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
