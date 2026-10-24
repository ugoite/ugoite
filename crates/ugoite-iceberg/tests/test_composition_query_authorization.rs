mod common;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use ugoite_core::sql_query::{SavedSqlRevisionRef, SqlQueryRequest};
use ugoite_domain::composition::{
    CompositionComponent, CompositionDocument, CompositionFormat, CompositionKind,
    CompositionResultFieldType, CompositionSpec, DashboardFlowLayout, FlowItem, FlowLayoutKind,
    FlowRow, VariableGraphArgumentBinding, VariableGraphColumn, VariableGraphNode,
    VariableGraphNodeKind, VariableGraphQuery, VariableGraphQueryDefinition,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
};
use ugoite_domain::identity::{
    AccessPolicy, Action, Grant, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::composition::CompositionSaveRequest;
use ugoite_iceberg::saved_sql::{SqlKind, SqlPayload};
use ugoite_iceberg::service::UgoiteService;
use uuid::Uuid;

fn resolved_sql_request(
    resolution: &ugoite_iceberg::service::CompositionResolution,
) -> Result<SqlQueryRequest> {
    let plan = resolution
        .plan
        .as_ref()
        .context("authorized Composition resolution should return a plan")?;
    let [query] = plan.queries.as_slice() else {
        anyhow::bail!("test Composition should resolve to exactly one query variable");
    };
    query
        .request
        .clone()
        .context("query variable should contain a bounded SQL request")
}

fn scalar_descriptor(
    result_type: CompositionResultFieldType,
    nullable: bool,
) -> VariableGraphScalarDescriptor {
    VariableGraphScalarDescriptor {
        scalar_type: match result_type {
            CompositionResultFieldType::String => VariableGraphScalarType::String,
            CompositionResultFieldType::Boolean => VariableGraphScalarType::Boolean,
            CompositionResultFieldType::Integer => VariableGraphScalarType::Integer,
            CompositionResultFieldType::Float => VariableGraphScalarType::Float,
            CompositionResultFieldType::Decimal => VariableGraphScalarType::Decimal,
            CompositionResultFieldType::Date => VariableGraphScalarType::Date,
            CompositionResultFieldType::Timestamp => VariableGraphScalarType::Timestamp,
            CompositionResultFieldType::Json => VariableGraphScalarType::Json,
        },
        nullable,
    }
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
    let metric_sql_specs = vec![
        (
            "row_count_sql",
            "Metric row count",
            "row_count",
            CompositionResultFieldType::Integer,
            format!("SELECT COUNT(*) AS row_count FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "value_count_sql",
            "Metric value count",
            "value_count",
            CompositionResultFieldType::Integer,
            format!("SELECT COUNT(\"{amount_column}\") AS value_count FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "total_sql",
            "Metric total",
            "total",
            CompositionResultFieldType::Integer,
            format!("SELECT SUM(\"{amount_column}\") AS total FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "average_sql",
            "Metric average",
            "average",
            CompositionResultFieldType::Float,
            format!("SELECT AVG(\"{amount_column}\") AS average FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "minimum_sql",
            "Metric minimum",
            "minimum",
            CompositionResultFieldType::Integer,
            format!("SELECT MIN(\"{amount_column}\") AS minimum FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "maximum_sql",
            "Metric maximum",
            "maximum",
            CompositionResultFieldType::Integer,
            format!("SELECT MAX(\"{amount_column}\") AS maximum FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "null_total_sql",
            "Metric null total",
            "null_total",
            CompositionResultFieldType::Integer,
            format!("SELECT SUM(\"{all_null_column}\") AS null_total FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "empty_count_sql",
            "Metric empty count",
            "row_count",
            CompositionResultFieldType::Integer,
            format!("SELECT COUNT(*) AS row_count FROM \"{form_name}\" WHERE \"{amount_column}\" < 0"),
            json!([]),
        ),
        (
            "empty_total_sql",
            "Metric empty total",
            "total",
            CompositionResultFieldType::Integer,
            format!("SELECT SUM(\"{amount_column}\") AS total FROM \"{form_name}\" WHERE \"{amount_column}\" < 0"),
            json!([]),
        ),
        (
            "empty_rows_sql",
            "Metric empty rows",
            "total",
            CompositionResultFieldType::Integer,
            format!("SELECT \"{amount_column}\" AS total FROM \"{form_name}\" WHERE \"{amount_column}\" < 0"),
            json!([]),
        ),
        (
            "multiple_rows_sql",
            "Metric multiple rows",
            "total",
            CompositionResultFieldType::Integer,
            format!("SELECT \"{amount_column}\" AS total FROM \"{form_name}\" ORDER BY \"{amount_column}\""),
            json!([]),
        ),
        (
            "decimal_sql",
            "Metric decimal",
            "total",
            CompositionResultFieldType::Decimal,
            format!("SELECT CAST(SUM(\"{amount_column}\") AS DECIMAL(30, 4)) AS total FROM \"{form_name}\""),
            json!([]),
        ),
        (
            "parameter_sql",
            "Metric parameter",
            "total",
            CompositionResultFieldType::Integer,
            "SELECT {{minimum}} AS total".to_string(),
            json!([{
                "name": "minimum",
                "type": "integer",
                "description": "Selected amount"
            }]),
        ),
    ];
    let mut saved_sqls = BTreeMap::new();
    for (id, name, _, _, sql, variables) in &metric_sql_specs {
        let saved = service
            .create_saved_sql(
                &space_id,
                None,
                &saved_sql(name, sql.clone(), variables.clone()),
                "owner",
            )
            .await?;
        saved_sqls.insert(*id, saved);
    }
    let query_variable = |id: &str, column: &str, result_type| {
        let sql = saved_sqls.get(id).unwrap();
        let nullable = !matches!(id, "row_count_sql" | "value_count_sql" | "empty_count_sql");
        let arguments = if id == "parameter_sql" {
            vec![VariableGraphArgumentBinding {
                name: "minimum".to_string(),
                variable: "minimum".to_string(),
            }]
        } else {
            Vec::new()
        };
        VariableGraphNode {
            id: id.to_string(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar_descriptor(result_type, nullable),
                },
                query: VariableGraphQuery {
                    definition: VariableGraphQueryDefinition::SavedSql {
                        entry_id: sql["id"].as_str().map(str::to_owned),
                        revision_id: sql["revision_id"].as_str().map(str::to_owned),
                        column: Some(column.to_string()),
                    },
                    arguments,
                    form_bindings: Vec::new(),
                    schema_fingerprint: String::new(),
                },
            },
        }
    };
    let metric = |id: &str, variable: &str, _column: &str| CompositionComponent::Metric {
        id: id.to_string(),
        variable: variable.to_string(),
        presentation: None,
    };
    let mut components = [
        ("row_count", "row_count_sql", "row_count"),
        ("value_count", "value_count_sql", "value_count"),
        ("total", "total_sql", "total"),
        ("average", "average_sql", "average"),
        ("minimum", "minimum_sql", "minimum"),
        ("maximum", "maximum_sql", "maximum"),
        ("null_total", "null_total_sql", "null_total"),
        ("empty_count", "empty_count_sql", "row_count"),
        ("empty_total", "empty_total_sql", "total"),
        ("no_rows", "empty_rows_sql", "total"),
        ("multiple_rows", "multiple_rows_sql", "total"),
        ("decimal_total", "decimal_sql", "total"),
        ("parameter_value", "parameter_sql", "total"),
    ]
    .into_iter()
    .map(|(id, source, column)| metric(id, source, column))
    .collect::<Vec<_>>();
    components.push(CompositionComponent::Input {
        id: "minimum_input".to_string(),
        variable: "minimum".to_string(),
    });
    let mut graph_variables = metric_sql_specs
        .iter()
        .map(|(id, _, column, result_type, _, _)| query_variable(id, column, *result_type))
        .collect::<Vec<_>>();
    graph_variables.push(VariableGraphNode {
        id: "minimum".to_string(),
        kind: VariableGraphNodeKind::Input {
            value_type: scalar_descriptor(CompositionResultFieldType::Integer, false),
            required: true,
            default: None,
        },
    });
    let component_ids = components
        .iter()
        .map(|component| component.id().to_string())
        .collect::<Vec<_>>();
    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Metric query contract".to_string(),
        spec: CompositionSpec {
            variables: graph_variables,
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
        draft_base: None,
        component_id: component_id.to_string(),
        variables: BTreeMap::from([("minimum".to_string(), json!(minimum))]),
        unset_variables: Vec::new(),
    };

    let resolution = service
        .resolve_composition_local(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &BTreeMap::from([("minimum".to_string(), json!(0))]),
            &[],
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

    let parameterized_response = service
        .query_composition_metric_local(&space_id, query("parameter_value", 9_007_199_254_740_991))
        .await?;
    assert!(
        parameterized_response.ok,
        "parameterized Saved SQL metric failed: {parameterized_response:?}"
    );
    let parameterized = parameterized_response
        .result
        .context("parameterized Saved SQL metric should return the exact integer")?;
    assert_eq!(parameterized.value, json!("9007199254740991"));
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
    service
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
    let title_column = ugoite_domain::form::sql_column_name(field_id);
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
        spec: CompositionSpec {
            variables: vec![VariableGraphNode {
                id: "source".to_string(),
                kind: VariableGraphNodeKind::Query {
                    returns: VariableGraphResultType::Table {
                        columns: vec![
                            VariableGraphColumn {
                                name: "id".to_string(),
                                value_type: scalar_descriptor(CompositionResultFieldType::String, false),
                            },
                            VariableGraphColumn {
                                name: "title".to_string(),
                                value_type: scalar_descriptor(CompositionResultFieldType::String, true),
                            },
                        ],
                    },
                    query: VariableGraphQuery {
                        definition: VariableGraphQueryDefinition::InlineSql {
                            sql: format!(
                                "SELECT \"_ugoite_id\" AS id, \"{title_column}\" AS title FROM \"{form_name}\" ORDER BY \"_ugoite_id\""
                            ),
                        },
                        arguments: Vec::new(),
                        form_bindings: Vec::new(),
                        schema_fingerprint: String::new(),
                    },
                },
            }],
            components: vec![CompositionComponent::Table {
                id: "table".to_string(),
                variable: "source".to_string(),
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
            &[],
            &[viewer],
        )
        .await?;
    assert!(resolution.diagnostics.is_empty());
    let compiled_request = resolved_sql_request(&resolution)?;

    // Revoke Form access after resolve but before execution. The SQL boundary
    // must deny the stale plan using the caller's current ACL.
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
    let _first_page_after_revoke = service
        .query_sql_authorized_for_principals(&space_id, &[viewer], compiled_request.clone())
        .await
        .expect_err("the stale SQL plan must be denied after Form access is revoked");

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
            &[],
            &[viewer],
        )
        .await?;
    assert!(resolution.diagnostics.is_empty());
    let mut compiled_request = resolved_sql_request(&resolution)?;
    // Keep the page small so this query exercises a continuation.
    compiled_request.limit = 2;
    let first_page = service
        .query_sql_authorized_for_principals(&space_id, &[viewer], compiled_request.clone())
        .await?;
    assert_eq!(first_page.rows.len(), 2);
    assert!(first_page
        .rows
        .iter()
        .all(|row| { row.get("id").and_then(Value::as_str) != Some("composition-query-02") }));
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
        .query_sql_authorized_for_principals(
            &space_id,
            &[viewer],
            SqlQueryRequest {
                form_binding_guards: Vec::new(),
                continuation: Some(cursor),
                execution_id: None,
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
        form_binding_guards: Vec::new(),
        sql: String::new(),
        parameters: Map::new(),
        parameter_types: BTreeMap::new(),
        include_exact_integer_values: false,
        limit: 2,
        continuation: None,
        execution_id: None,
        saved_sql: Some(SavedSqlRevisionRef {
            id: "metric-scalar-source".to_string(),
            revision_id: saved_sql["revision_id"]
                .as_str()
                .expect("saved SQL revision id")
                .to_string(),
        }),
        binding_base: None,
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
        spec: CompositionSpec {
            variables: vec![VariableGraphNode {
                id: "amount_sql".to_string(),
                kind: VariableGraphNodeKind::Query {
                    returns: VariableGraphResultType::Scalar {
                        value_type: scalar_descriptor(CompositionResultFieldType::Integer, true),
                    },
                    query: VariableGraphQuery {
                        definition: VariableGraphQueryDefinition::SavedSql {
                            entry_id: saved_sql["id"].as_str().map(str::to_owned),
                            revision_id: saved_sql["revision_id"].as_str().map(str::to_owned),
                            column: Some("total".to_string()),
                        },
                        arguments: Vec::new(),
                        form_bindings: Vec::new(),
                        schema_fingerprint: String::new(),
                    },
                },
            }],
            components: vec![CompositionComponent::Metric {
                id: "amount_metric".to_string(),
                variable: "amount_sql".to_string(),
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
        draft_base: None,
        component_id: "amount_metric".to_string(),
        variables: BTreeMap::new(),
        unset_variables: Vec::new(),
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
        ugoite_domain::composition::CompositionDiagnosticCode::QueryUnavailable
    );
    Ok(())
}
