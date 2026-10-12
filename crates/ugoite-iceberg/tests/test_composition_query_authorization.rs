mod common;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use ugoite_core::{
    entry_query::EntryPageRequest,
    sql_query::{SavedSqlRevisionRef, SqlQueryRequest},
};
use ugoite_domain::composition::{
    CompositionAggregateOperation, CompositionComponent, CompositionDocument,
    CompositionFieldSchemaEntry, CompositionFormat, CompositionKind, CompositionMetricDefinition,
    CompositionParameter, CompositionParameterType, CompositionQueryOperator,
    CompositionResultColumn, CompositionResultFieldType, CompositionSortDirection,
    CompositionSource, CompositionSpec, CompositionValue, DashboardFlowLayout,
    EntryQueryFilterTemplate, EntryQueryProjectionTemplate, EntryQuerySortTemplate,
    EntryQueryTemplate, FlowItem, FlowLayoutKind, FlowRow,
};
use ugoite_domain::form::FieldType;
use ugoite_domain::identity::{
    AccessPolicy, Action, Grant, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::composition::CompositionSaveRequest;
use ugoite_iceberg::saved_sql::{SqlKind, SqlPayload};
use ugoite_iceberg::service::UgoiteService;
use uuid::Uuid;

fn resolved_entry_request(
    resolution: &ugoite_iceberg::service::CompositionResolution,
) -> Result<EntryPageRequest> {
    let plan = resolution
        .plan
        .as_ref()
        .context("authorized Composition resolution should return a plan")?;
    let [ugoite_core::composition::ResolvedSourceRequest::EntryQuery { request, .. }] =
        plan.sources.as_slice()
    else {
        anyhow::bail!("test Composition should resolve to exactly one EntryQuery source");
    };
    Ok(request.clone())
}

#[tokio::test]
async fn typed_metric_query_uses_full_matching_set_and_bounded_saved_sql_probe() -> Result<()> {
    use ugoite_core::composition_metric::{
        CompositionMetricQueryRequest, CompositionMetricQueryTarget,
    };

    let op = common::setup_operator()?;
    let service = UgoiteService::from_operator(
        op,
        format!("memory://composition-metric-query-{}", Uuid::now_v7()),
    );
    let owner = Uuid::from_u128(3_433_101);
    let space_id = service
        .create_space_for_principal("composition-metric-query", owner, "Owner")
        .await?
        .to_string();
    let form_name = "metric-query-source";
    let form = service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": form_name,
                "fields": {
                    "amount": {"id": 101, "type": "long", "required": false},
                    "all_null": {"id": 102, "type": "long", "required": false}
                }
            }),
        )
        .await?;
    let amount_field_id = ugoite_domain::id::FieldId::new(101)?;
    let all_null_field_id = ugoite_domain::id::FieldId::new(102)?;
    let amount_column = ugoite_domain::form::sql_column_name(amount_field_id);
    let amount_field = |amount: i64| {
        BTreeMap::from([
            ("amount".to_string(), json!(amount)),
            ("all_null".to_string(), Value::Null),
        ])
    };
    for (id, amount) in [
        ("metric-entry-01", 9_007_199_254_740_993_i64),
        ("metric-entry-02", 1_i64),
    ] {
        service
            .create_structured_entry_with_receipt(
                &space_id,
                id,
                form_name.to_string(),
                Vec::new(),
                amount_field(amount),
                BTreeMap::new(),
                "owner",
            )
            .await?;
    }
    service
        .create_structured_entry_with_receipt(
            &space_id,
            "metric-entry-null",
            form_name.to_string(),
            Vec::new(),
            BTreeMap::from([
                ("amount".to_string(), Value::Null),
                ("all_null".to_string(), Value::Null),
            ]),
            BTreeMap::new(),
            "owner",
        )
        .await?;

    let saved_sql = |name: &str, sql: String| SqlPayload {
        name: Some(name.to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql,
        variables: json!([]),
    };
    let scalar_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric scalar",
                format!(
                    "SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" = 9007199254740993"
                ),
            ),
            "owner",
        )
        .await?;
    let empty_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric empty",
                format!("SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" < 0"),
            ),
            "owner",
        )
        .await?;
    let multiple_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric multiple rows",
                format!("SELECT \"{amount_column}\" AS total FROM \"{form_name}\" ORDER BY \"{amount_column}\""),
            ),
            "owner",
        )
        .await?;
    let parameter_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &SqlPayload {
                name: Some("Metric parameter".to_string()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: format!(
                    "SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" = {{{{minimum}}}}"
                ),
                variables: json!([{
                    "name":"minimum",
                    "type":"integer",
                    "description":"Minimum included amount"
                }]),
            },
            "owner",
        )
        .await?;
    let null_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric null",
                format!(
                    "SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" IS NULL"
                ),
            ),
            "owner",
        )
        .await?;
    let decimal_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric decimal",
                format!(
                    "SELECT CAST(\"{amount_column}\" AS DECIMAL(30, 4)) AS total FROM \"{form_name}\" WHERE \"{amount_column}\" = 9007199254740993"
                ),
            ),
            "owner",
        )
        .await?;

    let sql_source = |id: &str, saved_sql: &Value, result_type: CompositionResultFieldType| {
        CompositionSource::SavedSql {
            id: id.to_string(),
            entry_id: serde_json::from_value(saved_sql["id"].clone()).unwrap(),
            revision_id: serde_json::from_value(saved_sql["revision_id"].clone()).unwrap(),
            expected_result: vec![CompositionResultColumn {
                name: "total".to_string(),
                result_type,
            }],
            variables: BTreeMap::new(),
        }
    };
    let metric = |id: &str, definition| CompositionComponent::Metric {
        id: id.to_string(),
        label: None,
        source: None,
        value_field: None,
        definition: Some(definition),
        presentation: None,
    };
    let entry_source_id = "entries";
    let components = vec![
        metric(
            "entry_sum",
            CompositionMetricDefinition::Aggregate {
                source: entry_source_id.to_string(),
                operation: CompositionAggregateOperation::Sum,
                field_id: Some(amount_field_id),
            },
        ),
        metric(
            "sql_scalar",
            CompositionMetricDefinition::SqlScalar {
                source: "scalar_sql".to_string(),
                column: "total".to_string(),
            },
        ),
        metric(
            "sql_empty",
            CompositionMetricDefinition::SqlScalar {
                source: "empty_sql".to_string(),
                column: "total".to_string(),
            },
        ),
        metric(
            "sql_multiple",
            CompositionMetricDefinition::SqlScalar {
                source: "multiple_sql".to_string(),
                column: "total".to_string(),
            },
        ),
        metric(
            "sql_null",
            CompositionMetricDefinition::SqlScalar {
                source: "null_sql".to_string(),
                column: "total".to_string(),
            },
        ),
        metric(
            "sql_decimal",
            CompositionMetricDefinition::SqlScalar {
                source: "decimal_sql".to_string(),
                column: "total".to_string(),
            },
        ),
        metric(
            "sql_parameterized",
            CompositionMetricDefinition::SqlScalar {
                source: "parameter_sql".to_string(),
                column: "total".to_string(),
            },
        ),
    ];
    let metric_aggregate =
        |id: &str, source: &str, operation: CompositionAggregateOperation, field_id| {
            metric(
                id,
                CompositionMetricDefinition::Aggregate {
                    source: source.to_string(),
                    operation,
                    field_id,
                },
            )
        };
    let mut components = components;
    components.push(metric_aggregate(
        "entry_count_rows",
        entry_source_id,
        CompositionAggregateOperation::CountRows,
        None,
    ));
    components.push(metric_aggregate(
        "entry_count_values",
        entry_source_id,
        CompositionAggregateOperation::CountValues,
        Some(amount_field_id),
    ));
    for (id, operation) in [
        ("all_null_sum", CompositionAggregateOperation::Sum),
        ("all_null_avg", CompositionAggregateOperation::Avg),
        ("all_null_min", CompositionAggregateOperation::Min),
        ("all_null_max", CompositionAggregateOperation::Max),
    ] {
        components.push(metric_aggregate(
            id,
            entry_source_id,
            operation,
            Some(all_null_field_id),
        ));
    }
    components.push(metric_aggregate(
        "all_null_count_values",
        entry_source_id,
        CompositionAggregateOperation::CountValues,
        Some(all_null_field_id),
    ));
    components.push(metric_aggregate(
        "empty_count_rows",
        "empty_entries",
        CompositionAggregateOperation::CountRows,
        None,
    ));
    components.push(metric_aggregate(
        "empty_count_values",
        "empty_entries",
        CompositionAggregateOperation::CountValues,
        Some(amount_field_id),
    ));
    for (id, operation) in [
        ("empty_sum", CompositionAggregateOperation::Sum),
        ("empty_avg", CompositionAggregateOperation::Avg),
        ("empty_min", CompositionAggregateOperation::Min),
        ("empty_max", CompositionAggregateOperation::Max),
    ] {
        components.push(metric_aggregate(
            id,
            "empty_entries",
            operation,
            Some(amount_field_id),
        ));
    }
    let mut sources = vec![CompositionSource::EntryQuery {
        id: entry_source_id.to_string(),
        form_id: form.form_id,
        field_schema: [amount_field_id, all_null_field_id]
            .into_iter()
            .map(|field_id| CompositionFieldSchemaEntry {
                field_id,
                field_type: FieldType::Long,
                reference_form: None,
                list_item: None,
            })
            .collect(),
        query: EntryQueryTemplate {
            text: None,
            filters: Vec::new(),
            sort: Vec::new(),
            page_limit: 1,
            projection: EntryQueryProjectionTemplate::Preview,
            display_system_fields: Vec::new(),
        },
    }];
    sources.push(CompositionSource::EntryQuery {
        id: "empty_entries".to_string(),
        form_id: form.form_id,
        field_schema: [amount_field_id, all_null_field_id]
            .into_iter()
            .map(|field_id| CompositionFieldSchemaEntry {
                field_id,
                field_type: FieldType::Long,
                reference_form: None,
                list_item: None,
            })
            .collect(),
        query: EntryQueryTemplate {
            text: None,
            filters: vec![EntryQueryFilterTemplate {
                field_id: amount_field_id,
                operator: CompositionQueryOperator::Gte,
                value: serde_json::from_value::<CompositionValue>(json!(i64::MAX))?,
            }],
            sort: Vec::new(),
            page_limit: 1,
            projection: EntryQueryProjectionTemplate::Preview,
            display_system_fields: Vec::new(),
        },
    });
    sources.extend([
        sql_source(
            "scalar_sql",
            &scalar_sql,
            CompositionResultFieldType::Integer,
        ),
        sql_source("empty_sql", &empty_sql, CompositionResultFieldType::Integer),
        sql_source(
            "multiple_sql",
            &multiple_sql,
            CompositionResultFieldType::Integer,
        ),
        sql_source("null_sql", &null_sql, CompositionResultFieldType::Integer),
        sql_source(
            "decimal_sql",
            &decimal_sql,
            CompositionResultFieldType::Decimal,
        ),
        CompositionSource::SavedSql {
            id: "parameter_sql".to_string(),
            entry_id: serde_json::from_value(parameter_sql["id"].clone())?,
            revision_id: serde_json::from_value(parameter_sql["revision_id"].clone())?,
            expected_result: vec![CompositionResultColumn {
                name: "total".to_string(),
                result_type: CompositionResultFieldType::Integer,
            }],
            variables: BTreeMap::from([(
                "minimum".to_string(),
                ugoite_domain::composition::CompositionValue::Parameter(
                    ugoite_domain::composition::CompositionParameterReference {
                        parameter: "minimum".to_string(),
                    },
                ),
            )]),
        },
    ]);
    let component_ids = components
        .iter()
        .map(|component| component.id().to_string())
        .collect::<Vec<_>>();
    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Metric query contract".to_string(),
        tags: Vec::new(),
        spec: CompositionSpec {
            parameters: vec![CompositionParameter {
                id: "minimum".to_string(),
                label: None,
                parameter_type: CompositionParameterType::Integer,
                required: true,
                default: None,
                format: None,
            }],
            sources,
            components,
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_string(),
                    items: component_ids
                        .iter()
                        .map(|component| FlowItem::Component {
                            component: component.clone(),
                        })
                        .chain(std::iter::once(FlowItem::Parameter {
                            parameter: "minimum".to_string(),
                        }))
                        .collect(),
                }],
            },
        },
    };
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let query = |component_id: &str| CompositionMetricQueryRequest {
        target: CompositionMetricQueryTarget::SavedRevision {
            composition_id: saved.entry_id.to_string(),
            revision_id: saved.revision_id.to_string(),
        },
        component_id: component_id.to_string(),
        parameters: BTreeMap::from([("minimum".to_string(), json!(0))]),
    };

    let resolution = service
        .resolve_composition_local(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &BTreeMap::from([("minimum".to_string(), json!(0))]),
        )
        .await?;
    assert!(
        resolution.plan.is_some(),
        "typed metric Composition should resolve: {:?}",
        resolution.diagnostics
    );

    let aggregate = service
        .query_composition_metric_local(&space_id, query("entry_sum"))
        .await
        .context("querying the aggregate Entry metric should succeed")?;
    let aggregate_result = aggregate
        .result
        .clone()
        .context(format!("aggregate result should exist: {aggregate:?}"))?;
    assert_eq!(
        aggregate_result.operation,
        ugoite_core::composition_metric::CompositionMetricOperation::Sum
    );
    assert_eq!(aggregate_result.value, json!("9007199254740994"));

    for (component_id, expected_value) in [
        ("entry_count_rows", json!("3")),
        ("entry_count_values", json!("2")),
        ("all_null_sum", Value::Null),
        ("all_null_avg", Value::Null),
        ("all_null_min", Value::Null),
        ("all_null_max", Value::Null),
        ("all_null_count_values", json!("0")),
        ("empty_count_rows", json!("0")),
        ("empty_count_values", json!("0")),
        ("empty_sum", Value::Null),
        ("empty_avg", Value::Null),
        ("empty_min", Value::Null),
        ("empty_max", Value::Null),
    ] {
        let response = service
            .query_composition_metric_local(&space_id, query(component_id))
            .await
            .with_context(|| format!("querying {component_id} should succeed"))?;
        assert!(response.ok, "{component_id} failed: {response:?}");
        let result = response
            .result
            .with_context(|| format!("{component_id} should return a scalar result"))?;
        assert_eq!(result.value, expected_value, "{component_id}");
    }

    let sql_scalar_result = service
        .query_composition_metric_local(&space_id, query("sql_scalar"))
        .await
        .context("querying a one-row Saved SQL scalar should succeed")?
        .result
        .context("one-row Saved SQL scalar should succeed")?;
    assert_eq!(sql_scalar_result.value, json!("9007199254740993"));

    let sql_decimal_result = service
        .query_composition_metric_local(&space_id, query("sql_decimal"))
        .await
        .context("querying a Decimal Saved SQL scalar should succeed")?
        .result
        .context("one-row Decimal Saved SQL scalar should succeed")?;
    assert_eq!(
        sql_decimal_result.result_type,
        CompositionResultFieldType::Decimal
    );
    assert_eq!(sql_decimal_result.value, json!("9007199254740993.0000"));

    let parameter_query = CompositionMetricQueryRequest {
        target: CompositionMetricQueryTarget::SavedRevision {
            composition_id: saved.entry_id.to_string(),
            revision_id: saved.revision_id.to_string(),
        },
        component_id: "sql_parameterized".to_string(),
        parameters: BTreeMap::from([("minimum".to_string(), json!(9_007_199_254_740_993_i64))]),
    };

    let parameterized = service
        .query_composition_metric_local(&space_id, parameter_query)
        .await
        .context("querying a parameterized Saved SQL scalar should succeed")?
        .result
        .context("parameterized Saved SQL metric should succeed")?;
    assert_eq!(parameterized.value, json!("9007199254740993"));

    let empty = service
        .query_composition_metric_local(&space_id, query("sql_empty"))
        .await
        .context("probing an empty Saved SQL scalar should return a one-row diagnostic")?;
    assert!(!empty.ok);
    assert!(empty.result.is_none());
    assert_eq!(
        empty.diagnostics[0].code,
        ugoite_domain::composition::CompositionDiagnosticCode::MetricScalarExpectedOneRow
    );

    let multiple = service
        .query_composition_metric_local(&space_id, query("sql_multiple"))
        .await
        .context("probing multiple Saved SQL rows should return a diagnostic")?;
    assert!(!multiple.ok);
    assert!(
        multiple.result.is_none(),
        "a two-row probe must never return a prefix value"
    );
    assert_eq!(
        multiple.diagnostics[0].code,
        ugoite_domain::composition::CompositionDiagnosticCode::MetricScalarExpectedOneRow
    );

    let raw_null_probe = service
        .probe_sql_scalar_local(
            &space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: BTreeMap::new(),
                include_exact_integer_values: false,
                limit: 2,
                continuation: None,
                saved_sql: Some(SavedSqlRevisionRef {
                    id: null_sql["id"]
                        .as_str()
                        .context("NULL Saved SQL id")?
                        .to_owned(),
                    revision_id: null_sql["revision_id"]
                        .as_str()
                        .context("NULL Saved SQL revision")?
                        .to_owned(),
                }),
            },
        )
        .await?;
    assert_eq!(raw_null_probe.columns, ["total"]);
    assert_eq!(raw_null_probe.rows.len(), 1);
    assert_eq!(
        raw_null_probe.result_schema[0].column_type,
        ugoite_core::sql_query::SqlResultColumnType::Integer
    );

    let null = service
        .query_composition_metric_local(&space_id, query("sql_null"))
        .await
        .context("querying a NULL Saved SQL scalar should succeed")?;
    assert!(null.ok, "SQL NULL metric query should succeed: {null:?}");
    assert_eq!(
        null.result.context("SQL NULL is a scalar result")?.value,
        Value::Null
    );

    service
        .create_structured_entry_with_receipt(
            &space_id,
            "metric-entry-03",
            form_name.to_string(),
            Vec::new(),
            amount_field(2),
            BTreeMap::new(),
            "owner",
        )
        .await?;
    let updated = service
        .query_composition_metric_local(&space_id, query("entry_sum"))
        .await
        .context("requerying the aggregate after an Entry write should succeed")?
        .result
        .context("revisiting the same Composition should see current data")?;
    assert_eq!(updated.value, json!("9007199254740996"));

    for (component_id, expected_value) in [
        ("entry_count_rows", json!("4")),
        ("entry_count_values", json!("3")),
    ] {
        let updated_count = service
            .query_composition_metric_local(&space_id, query(component_id))
            .await
            .with_context(|| {
                format!("requerying {component_id} after an Entry write should succeed")
            })?
            .result
            .with_context(|| format!("revisiting {component_id} should return a scalar"))?;
        assert_eq!(updated_count.value, expected_value, "{component_id}");
    }

    let changed_parameter = CompositionMetricQueryRequest {
        target: CompositionMetricQueryTarget::SavedRevision {
            composition_id: saved.entry_id.to_string(),
            revision_id: saved.revision_id.to_string(),
        },
        component_id: "sql_parameterized".to_string(),
        parameters: BTreeMap::from([("minimum".to_string(), json!(1))]),
    };
    let parameter_changed = service
        .query_composition_metric_local(&space_id, changed_parameter)
        .await
        .context("querying a Saved SQL scalar after a parameter change should succeed")?
        .result
        .context("changing a declared parameter should change the scalar")?;
    assert_eq!(parameter_changed.value, json!("1"));

    Ok(())
}

fn source_fields(title: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([("title".to_string(), Value::String(title.to_string()))])
}

#[tokio::test]
async fn source_resolution_and_continuation_recheck_current_authorization() -> Result<()> {
    let op = common::setup_operator()?;
    let service = UgoiteService::from_operator(
        op.clone(),
        format!("memory://composition-query-auth-{}", Uuid::now_v7()),
    );
    let owner = Uuid::from_u128(3_433_001);
    let viewer = Uuid::from_u128(3_433_002);
    let space_id = service
        .create_space_for_principal("composition-query-auth", owner, "Owner")
        .await?
        .to_string();
    let authorizer = Authorizer::new(op);

    let form_name = "Composition-Query-Auth";
    let form = service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": form_name,
                "fields": {
                    "title": {"id": 100, "type": "string", "required": false}
                }
            }),
        )
        .await?;
    let field_id = ugoite_domain::id::FieldId::new(100)?;
    for index in 0..4 {
        service
            .create_structured_entry_with_receipt(
                &space_id,
                &format!("composition-query-{index:02}"),
                form_name.to_string(),
                Vec::new(),
                source_fields(&format!("row-{index}")),
                BTreeMap::new(),
                "owner",
            )
            .await?;
    }

    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: viewer,
                kind: PrincipalKind::Human,
                display_name: "Viewer".to_string(),
                state: PrincipalState::Active,
                created_at: Utc::now().to_rfc3339(),
            },
            SpaceRole::Viewer,
        )
        .await?;

    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Authorized query pages".to_string(),
        tags: Vec::new(),
        spec: CompositionSpec {
            parameters: Vec::new(),
            sources: vec![CompositionSource::EntryQuery {
                id: "source".to_string(),
                form_id: form.form_id,
                field_schema: vec![CompositionFieldSchemaEntry {
                    field_id,
                    field_type: FieldType::String,
                    reference_form: None,
                    list_item: None,
                }],
                query: EntryQueryTemplate {
                    text: None,
                    filters: Vec::new(),
                    sort: vec![EntryQuerySortTemplate {
                        field_id,
                        direction: CompositionSortDirection::Asc,
                    }],
                    page_limit: 2,
                    projection: EntryQueryProjectionTemplate::Preview,
                    display_system_fields: Vec::new(),
                },
            }],
            components: vec![CompositionComponent::Table {
                id: "table".to_string(),
                label: None,
                source: "source".to_string(),
            }],
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_string(),
                    items: vec![FlowItem::Component {
                        component: "table".to_string(),
                    }],
                }],
            },
        },
    };
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    // Resolve while the viewer can read the source. The compiled query itself
    // carries no authorization grant; execution must check current ACL again.
    let resolution = service
        .resolve_composition_authorized_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &BTreeMap::new(),
            &[viewer],
        )
        .await?;
    assert!(resolution.diagnostics.is_empty());
    let compiled_request = resolved_entry_request(&resolution)?;

    // Revoke Form access after resolve but before its first page. The existing
    // authorized query boundary returns no source rows for the stale plan.
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Form,
                id: form_name.to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![Grant {
                    principal_id: owner,
                    actions: [Action::Read].into_iter().collect(),
                }],
            },
        )
        .await?;
    let first_page_after_revoke = service
        .query_entry_page_authorized_for_principals(&space_id, &[viewer], compiled_request.clone())
        .await
        .context("the existing query path should complete with the current ACL")?;
    assert!(
        first_page_after_revoke.rows.is_empty(),
        "the stale resolved request must return no rows after Form access is revoked"
    );
    assert!(!first_page_after_revoke.has_more);
    assert!(first_page_after_revoke.next.is_none());

    // Restore access, resolve the same exact Composition revision, and issue a
    // continuation. Revoking one later-page Entry must invalidate that cursor.
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Form,
                id: form_name.to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: true,
                grants: Vec::new(),
            },
        )
        .await?;
    let resolution = service
        .resolve_composition_authorized_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &BTreeMap::new(),
            &[viewer],
        )
        .await?;
    assert!(resolution.diagnostics.is_empty());
    let compiled_request = resolved_entry_request(&resolution)?;
    let first_page = service
        .query_entry_page_authorized_for_principals(&space_id, &[viewer], compiled_request.clone())
        .await?;
    assert_eq!(first_page.rows.len(), 2);
    assert!(first_page
        .rows
        .iter()
        .all(|row| row.id != "composition-query-02"));
    let cursor = first_page
        .next
        .context("first query page should provide a continuation")?;
    assert!(first_page.has_more);

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "composition-query-02".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    let continued = service
        .query_entry_page_authorized_for_principals(
            &space_id,
            &[viewer],
            EntryPageRequest {
                after: Some(cursor),
                ..compiled_request
            },
        )
        .await
        .expect_err("revoked source access must invalidate the resolved continuation");
    let error = format!("{continued:#}");
    assert!(
        error.contains("authorization"),
        "continuation should fail because current authorization changed: {error}"
    );

    // The bounded metric SQL probe must use the same current Entry ACL as
    // ordinary SQL reads, while retaining the exact Saved SQL revision.
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "composition-query-00".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![Grant {
                    principal_id: owner,
                    actions: [Action::Read].into_iter().collect(),
                }],
            },
        )
        .await?;
    let saved_sql = service
        .create_saved_sql(
            &space_id,
            Some("metric-scalar-source"),
            &SqlPayload {
                name: Some("Metric scalar source".to_string()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: format!(
                    "SELECT \"_ugoite_id\" AS id FROM \"{form_name}\" ORDER BY \"_ugoite_id\""
                ),
                variables: json!([]),
            },
            "owner",
        )
        .await?;
    let probe_request = || SqlQueryRequest {
        sql: String::new(),
        parameters: Map::new(),
        parameter_types: BTreeMap::new(),
        include_exact_integer_values: false,
        limit: 2,
        continuation: None,
        saved_sql: Some(SavedSqlRevisionRef {
            id: "metric-scalar-source".to_string(),
            revision_id: saved_sql["revision_id"]
                .as_str()
                .expect("saved SQL revision id")
                .to_string(),
        }),
    };
    let viewer_probe = service
        .probe_sql_scalar_authorized_for_principals(&space_id, &[viewer], probe_request())
        .await?;
    let owner_probe = service
        .probe_sql_scalar_authorized_for_principals(&space_id, &[owner], probe_request())
        .await?;
    assert_eq!(viewer_probe.rows.len(), 2);
    assert_eq!(owner_probe.rows.len(), 2);
    assert!(viewer_probe
        .rows
        .iter()
        .all(|row| { row["id"] != "composition-query-00" && row["id"] != "composition-query-02" }));
    assert!(owner_probe
        .rows
        .iter()
        .any(|row| row["id"] == "composition-query-00"));
    Ok(())
}

#[tokio::test]
async fn authorized_metric_scalar_rechecks_current_form_and_saved_sql_access() -> Result<()> {
    use ugoite_core::composition_metric::{
        CompositionMetricQueryRequest, CompositionMetricQueryTarget,
    };

    let op = common::setup_operator()?;
    let service = UgoiteService::from_operator(
        op.clone(),
        format!("memory://authorized-composition-metric-{}", Uuid::now_v7()),
    );
    let owner = Uuid::from_u128(3_433_201);
    let viewer = Uuid::from_u128(3_433_202);
    let space_id = service
        .create_space_for_principal("authorized-composition-metric", owner, "Owner")
        .await?
        .to_string();
    let authorizer = Authorizer::new(op);
    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: viewer,
                kind: PrincipalKind::Human,
                display_name: "Viewer".to_string(),
                state: PrincipalState::Active,
                created_at: Utc::now().to_rfc3339(),
            },
            SpaceRole::Viewer,
        )
        .await?;

    let form_name = "metric-amount";
    service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": form_name,
                "fields": {
                    "amount": {"id": 101, "type": "long", "required": false}
                }
            }),
        )
        .await?;
    let amount_column = ugoite_domain::form::sql_column_name(ugoite_domain::id::FieldId::new(101)?);
    for (id, amount) in [
        ("metric-amount-visible", 7_i64),
        ("metric-amount-other", 8_i64),
    ] {
        service
            .create_structured_entry_with_receipt(
                &space_id,
                id,
                form_name.to_string(),
                Vec::new(),
                BTreeMap::from([("amount".to_string(), json!(amount))]),
                BTreeMap::new(),
                "owner",
            )
            .await?;
    }

    let saved_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &SqlPayload {
                name: Some("Metric amount".to_string()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: format!(
                    "SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" = 7"
                ),
                variables: json!([]),
            },
            "owner",
        )
        .await?;
    let saved_sql_id = saved_sql["id"].as_str().context("Saved SQL id")?;
    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Authorized metric".to_string(),
        tags: Vec::new(),
        spec: CompositionSpec {
            parameters: Vec::new(),
            sources: vec![CompositionSource::SavedSql {
                id: "amount_sql".to_string(),
                entry_id: serde_json::from_value(saved_sql["id"].clone())?,
                revision_id: serde_json::from_value(saved_sql["revision_id"].clone())?,
                expected_result: vec![CompositionResultColumn {
                    name: "total".to_string(),
                    result_type: CompositionResultFieldType::Integer,
                }],
                variables: BTreeMap::new(),
            }],
            components: vec![CompositionComponent::Metric {
                id: "amount_metric".to_string(),
                label: None,
                source: None,
                value_field: None,
                definition: Some(CompositionMetricDefinition::SqlScalar {
                    source: "amount_sql".to_string(),
                    column: "total".to_string(),
                }),
                presentation: None,
            }],
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_string(),
                    items: vec![FlowItem::Component {
                        component: "amount_metric".to_string(),
                    }],
                }],
            },
        },
    };
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let metric_request = CompositionMetricQueryRequest {
        target: CompositionMetricQueryTarget::SavedRevision {
            composition_id: saved.entry_id.to_string(),
            revision_id: saved.revision_id.to_string(),
        },
        component_id: "amount_metric".to_string(),
        parameters: BTreeMap::new(),
    };

    let visible = service
        .query_composition_metric_authorized_for_principals(
            &space_id,
            &[viewer],
            metric_request.clone(),
        )
        .await?;
    assert!(
        visible.ok,
        "viewer should be able to read the metric: {visible:?}"
    );
    assert_eq!(
        visible.result.context("authorized metric value")?.value,
        json!("7")
    );

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: saved.entry_id.to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![Grant {
                    principal_id: owner,
                    actions: [Action::Read].into_iter().collect(),
                }],
            },
        )
        .await?;
    assert!(service
        .query_composition_metric_authorized_for_principals(
            &space_id,
            &[viewer],
            metric_request.clone(),
        )
        .await
        .is_err());
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: saved.entry_id.to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: true,
                grants: Vec::new(),
            },
        )
        .await?;

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "metric-amount-visible".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![Grant {
                    principal_id: owner,
                    actions: [Action::Read].into_iter().collect(),
                }],
            },
        )
        .await?;
    let entry_revoked = service
        .query_composition_metric_authorized_for_principals(
            &space_id,
            &[viewer],
            metric_request.clone(),
        )
        .await?;
    assert!(!entry_revoked.ok);
    assert!(entry_revoked.result.is_none());
    assert_eq!(
        entry_revoked.diagnostics[0].code,
        ugoite_domain::composition::CompositionDiagnosticCode::MetricScalarExpectedOneRow
    );

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "metric-amount-visible".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: true,
                grants: Vec::new(),
            },
        )
        .await?;
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::SavedSql,
                id: saved_sql_id.to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![Grant {
                    principal_id: owner,
                    actions: [Action::Read].into_iter().collect(),
                }],
            },
        )
        .await?;
    let saved_sql_revoked = service
        .query_composition_metric_authorized_for_principals(&space_id, &[viewer], metric_request)
        .await?;
    assert!(!saved_sql_revoked.ok);
    assert!(saved_sql_revoked.result.is_none());
    assert_eq!(
        saved_sql_revoked.diagnostics[0].code,
        ugoite_domain::composition::CompositionDiagnosticCode::SourceUnavailable
    );
    Ok(())
}
