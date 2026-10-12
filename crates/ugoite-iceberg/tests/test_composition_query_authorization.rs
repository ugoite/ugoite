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
    CompositionComponent, CompositionDocument, CompositionFieldSchemaEntry, CompositionFormat,
    CompositionKind, CompositionParameter, CompositionParameterType, CompositionResultColumn,
    CompositionResultFieldType, CompositionSortDirection, CompositionSource, CompositionSpec,
    CompositionValue, DashboardFlowLayout, EntryQueryProjectionTemplate, EntryQuerySortTemplate,
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
async fn saved_sql_columns_resolve_to_bounded_metric_results() -> Result<()> {
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
    service
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
    let amount_column = ugoite_domain::form::sql_column_name(amount_field_id);
    let all_null_column =
        ugoite_domain::form::sql_column_name(ugoite_domain::id::FieldId::new(102)?);
    for (id, amount) in [
        ("metric-entry-01", json!(9_007_199_254_740_993_i64)),
        ("metric-entry-02", json!(1)),
        ("metric-entry-null", Value::Null),
    ] {
        service
            .create_structured_entry_with_receipt(
                &space_id,
                id,
                form_name.to_string(),
                Vec::new(),
                BTreeMap::from([
                    ("amount".to_string(), amount),
                    ("all_null".to_string(), Value::Null),
                ]),
                BTreeMap::new(),
                "owner",
            )
            .await?;
    }

    let saved_sql = |name: &str, sql: String, variables: Value| SqlPayload {
        name: Some(name.to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql,
        variables,
    };
    let summary_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric summary",
                format!(
                    "SELECT COUNT(*) AS row_count, COUNT(\"{amount_column}\") AS value_count, SUM(\"{amount_column}\") AS total, AVG(\"{amount_column}\") AS average, MIN(\"{amount_column}\") AS minimum, MAX(\"{amount_column}\") AS maximum, SUM(\"{all_null_column}\") AS null_total FROM \"{form_name}\""
                ),
                json!([]),
            ),
            "owner",
        )
        .await?;
    let empty_summary_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric empty summary",
                format!(
                    "SELECT COUNT(*) AS row_count, SUM(\"{amount_column}\") AS total FROM \"{form_name}\" WHERE \"{amount_column}\" < 0"
                ),
                json!([]),
            ),
            "owner",
        )
        .await?;
    let empty_rows_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric empty rows",
                format!(
                    "SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" < 0"
                ),
                json!([]),
            ),
            "owner",
        )
        .await?;
    let multiple_rows_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric multiple rows",
                format!(
                    "SELECT \"{amount_column}\" AS total FROM \"{form_name}\" ORDER BY \"{amount_column}\""
                ),
                json!([]),
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
                    "SELECT CAST(SUM(\"{amount_column}\") AS DECIMAL(30, 4)) AS total FROM \"{form_name}\""
                ),
                json!([]),
            ),
            "owner",
        )
        .await?;
    let parameter_sql = service
        .create_saved_sql(
            &space_id,
            None,
            &saved_sql(
                "Metric parameter",
                format!(
                    "SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" = {{{{minimum}}}}"
                ),
                json!([{
                    "name": "minimum",
                    "type": "integer",
                    "description": "Selected amount"
                }]),
            ),
            "owner",
        )
        .await?;

    let summary_result = vec![
        ("row_count", CompositionResultFieldType::Integer),
        ("value_count", CompositionResultFieldType::Integer),
        ("total", CompositionResultFieldType::Integer),
        ("average", CompositionResultFieldType::Float),
        ("minimum", CompositionResultFieldType::Integer),
        ("maximum", CompositionResultFieldType::Integer),
        ("null_total", CompositionResultFieldType::Integer),
    ]
    .into_iter()
    .map(|(name, result_type)| CompositionResultColumn {
        name: name.to_string(),
        result_type,
    })
    .collect::<Vec<_>>();
    let source = |id: &str, sql: &Value, expected_result| CompositionSource::SavedSql {
        id: id.to_string(),
        entry_id: serde_json::from_value(sql["id"].clone()).unwrap(),
        revision_id: serde_json::from_value(sql["revision_id"].clone()).unwrap(),
        expected_result,
        variables: BTreeMap::new(),
    };
    let metric = |id: &str, source: &str, column: &str| CompositionComponent::Metric {
        id: id.to_string(),
        label: None,
        source: source.to_string(),
        column: column.to_string(),
        presentation: None,
    };
    let components = [
        ("row_count", "summary_sql", "row_count"),
        ("value_count", "summary_sql", "value_count"),
        ("total", "summary_sql", "total"),
        ("average", "summary_sql", "average"),
        ("minimum", "summary_sql", "minimum"),
        ("maximum", "summary_sql", "maximum"),
        ("null_total", "summary_sql", "null_total"),
        ("empty_count", "empty_summary_sql", "row_count"),
        ("empty_total", "empty_summary_sql", "total"),
        ("no_rows", "empty_rows_sql", "total"),
        ("multiple_rows", "multiple_rows_sql", "total"),
        ("decimal_total", "decimal_sql", "total"),
        ("parameter_value", "parameter_sql", "total"),
    ]
    .into_iter()
    .map(|(id, source, column)| metric(id, source, column))
    .collect::<Vec<_>>();
    let source_list = vec![
        source("summary_sql", &summary_sql, summary_result),
        source(
            "empty_summary_sql",
            &empty_summary_sql,
            vec![
                CompositionResultColumn {
                    name: "row_count".to_string(),
                    result_type: CompositionResultFieldType::Integer,
                },
                CompositionResultColumn {
                    name: "total".to_string(),
                    result_type: CompositionResultFieldType::Integer,
                },
            ],
        ),
        source(
            "empty_rows_sql",
            &empty_rows_sql,
            vec![CompositionResultColumn {
                name: "total".to_string(),
                result_type: CompositionResultFieldType::Integer,
            }],
        ),
        source(
            "multiple_rows_sql",
            &multiple_rows_sql,
            vec![CompositionResultColumn {
                name: "total".to_string(),
                result_type: CompositionResultFieldType::Integer,
            }],
        ),
        source(
            "decimal_sql",
            &decimal_sql,
            vec![CompositionResultColumn {
                name: "total".to_string(),
                result_type: CompositionResultFieldType::Decimal,
            }],
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
                CompositionValue::Parameter(
                    ugoite_domain::composition::CompositionParameterReference {
                        parameter: "minimum".to_string(),
                    },
                ),
            )]),
        },
    ];
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
            sources: source_list,
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
    let query = |component_id: &str, minimum: i64| CompositionMetricQueryRequest {
        target: CompositionMetricQueryTarget::SavedRevision {
            composition_id: saved.entry_id.to_string(),
            revision_id: saved.revision_id.to_string(),
        },
        component_id: component_id.to_string(),
        parameters: BTreeMap::from([("minimum".to_string(), json!(minimum))]),
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
        "SQL-backed Metric Composition should resolve: {:?}",
        resolution.diagnostics
    );

    for (component_id, expected_value) in [
        ("row_count", json!("3")),
        ("value_count", json!("2")),
        ("total", json!("9007199254740994")),
        ("average", json!(4_503_599_627_370_497_f64)),
        ("minimum", json!("1")),
        ("maximum", json!("9007199254740993")),
        ("null_total", Value::Null),
        ("empty_count", json!("0")),
        ("empty_total", Value::Null),
    ] {
        let response = service
            .query_composition_metric_local(&space_id, query(component_id, 0))
            .await
            .with_context(|| format!("querying {component_id} through Saved SQL"))?;
        assert!(response.ok, "{component_id} failed: {response:?}");
        let result = response
            .result
            .with_context(|| format!("{component_id} should return its selected SQL column"))?;
        assert_eq!(result.value, expected_value, "{component_id}");
    }

    let decimal = service
        .query_composition_metric_local(&space_id, query("decimal_total", 0))
        .await?
        .result
        .context("decimal SQL column should produce a scalar")?;
    assert_eq!(decimal.result_type, CompositionResultFieldType::Decimal);
    assert_eq!(decimal.value, json!("9007199254740994.0000"));

    let parameterized = service
        .query_composition_metric_local(&space_id, query("parameter_value", 9_007_199_254_740_993))
        .await?
        .result
        .context("parameterized Saved SQL metric should return the exact integer")?;
    assert_eq!(parameterized.value, json!("9007199254740993"));
    let changed_parameter = service
        .query_composition_metric_local(&space_id, query("parameter_value", 1))
        .await?
        .result
        .context("changing a Saved SQL parameter should select a different scalar")?;
    assert_eq!(changed_parameter.value, json!("1"));

    for component_id in ["no_rows", "multiple_rows"] {
        let response = service
            .query_composition_metric_local(&space_id, query(component_id, 0))
            .await?;
        assert!(!response.ok, "{component_id} must not return a prefix row");
        assert!(response.result.is_none());
        assert_eq!(
            response.diagnostics[0].code,
            if component_id == "no_rows" {
                ugoite_domain::composition::CompositionDiagnosticCode::MetricScalarNoRows
            } else {
                ugoite_domain::composition::CompositionDiagnosticCode::MetricScalarMultipleRows
            }
        );
    }

    service
        .create_structured_entry_with_receipt(
            &space_id,
            "metric-entry-03",
            form_name.to_string(),
            Vec::new(),
            BTreeMap::from([
                ("amount".to_string(), json!(2)),
                ("all_null".to_string(), Value::Null),
            ]),
            BTreeMap::new(),
            "owner",
        )
        .await?;
    let updated_count = service
        .query_composition_metric_local(&space_id, query("row_count", 0))
        .await?
        .result
        .context("the shared SQL result should be re-executed after Entry changes")?;
    assert_eq!(updated_count.value, json!("4"));
    let updated_total = service
        .query_composition_metric_local(&space_id, query("total", 0))
        .await?
        .result
        .context("the selected SUM column should reflect the new Entry")?;
    assert_eq!(updated_total.value, json!("9007199254740996"));
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
async fn authorized_composition_metrics_recheck_current_entry_and_saved_sql_access() -> Result<()> {
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
    let amount_field_id = ugoite_domain::id::FieldId::new(101)?;
    let amount_column = ugoite_domain::form::sql_column_name(amount_field_id);
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
                source: "amount_sql".to_string(),
                column: "total".to_string(),
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
        ugoite_domain::composition::CompositionDiagnosticCode::MetricScalarNoRows
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
