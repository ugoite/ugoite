mod common;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use ugoite_core::sql_query::{SavedSqlRevisionRef, SqlQueryCountRequest, SqlQueryRequest};
use ugoite_domain::composition::{
    CompositionComponent, CompositionDocument, CompositionFormat, CompositionKind, CompositionSpec,
    DashboardFlowLayout, FlowItem, FlowLayoutKind, FlowRow, VariableGraphColumn,
    VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind, VariableGraphQuery,
    VariableGraphQueryDefinition, VariableGraphResultType, VariableGraphScalarDescriptor,
    VariableGraphScalarType,
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
    scalar_type: VariableGraphScalarType,
    nullable: bool,
) -> VariableGraphScalarDescriptor {
    VariableGraphScalarDescriptor {
        scalar_type,
        nullable,
    }
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
                                value_type: scalar_descriptor(VariableGraphScalarType::String, false),
                            },
                            VariableGraphColumn {
                                name: "title".to_string(),
                                value_type: scalar_descriptor(VariableGraphScalarType::String, true),
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
    let evaluation = service
        .evaluate_composition_graph_authorized_for_principals(
            &space_id,
            &[viewer],
            ugoite_iceberg::service::CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            Some(&["source".to_string()]),
            None,
        )
        .await?;
    let count_snapshot = evaluation
        .query_results
        .get("source")
        .and_then(|result| result.count_snapshot.clone())
        .context("Composition evaluation issues a server-owned Count snapshot")?;
    let evaluation_request = evaluation
        .plan
        .as_ref()
        .context("saved Composition evaluation retains its resolved query")?
        .queries
        .first()
        .and_then(|query| query.request.clone())
        .context("saved Composition evaluation resolved a SQL request")?;
    let count_snapshot_as_page = service
        .query_sql_authorized_for_principals(
            &space_id,
            &[viewer],
            SqlQueryRequest {
                continuation: Some(count_snapshot.clone()),
                ..compiled_request.clone()
            },
        )
        .await;
    assert!(
        count_snapshot_as_page.is_err(),
        "Count snapshot tokens cannot continue a SQL page"
    );
    let count_request = || {
        SqlQueryCountRequest {
        // SQL execution fingerprints the original graph source before the
        // adapter binds the human Form name to its stable Form ID. Use that
        // same source representation for the explicit Count request.
        sql: format!(
            "SELECT \"_ugoite_id\" AS id, \"{title_column}\" AS title FROM \"{form_name}\" ORDER BY \"_ugoite_id\""
        ),
        parameters: evaluation_request.parameters.clone(),
        parameter_types: evaluation_request.parameter_types.clone(),
        saved_sql: evaluation_request.saved_sql.clone(),
        binding_base: evaluation_request.binding_base.clone(),
        form_binding_guards: evaluation_request.form_binding_guards.clone(),
        composition_count_snapshot: Some(count_snapshot.clone()),
    }
    };
    let initial_count = service
        .count_sql_authorized_for_principals(&space_id, &[viewer], count_request())
        .await?;
    assert_eq!(initial_count, 4);
    service
        .create_structured_entry_with_receipt(
            &space_id,
            "composition-query-04",
            form_name.to_string(),
            Vec::new(),
            source_fields("late publication row"),
            BTreeMap::new(),
            "owner",
        )
        .await?;
    let pinned_count = service
        .count_sql_authorized_for_principals(&space_id, &[viewer], count_request())
        .await?;
    assert_eq!(pinned_count, 4, "Count stays on the evaluation publication");

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
    let denied_count = service
        .count_sql_authorized_for_principals(&space_id, &[viewer], count_request())
        .await;
    assert!(
        denied_count.is_err(),
        "revoked Composition access fails closed"
    );

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

    // The standalone Saved SQL scalar probe must use the same current Entry
    // ACL as ordinary SQL reads while retaining its exact Saved SQL revision.
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
async fn inline_float_count_snapshot_matches_integer_json_zero() -> Result<()> {
    let op = common::setup_operator()?;
    let service = UgoiteService::from_operator(
        op,
        format!("inline-float-count-snapshot-{}", Uuid::now_v7()),
    );
    let owner = Uuid::from_u128(3_433_301);
    let space_id = service
        .create_space_for_principal("inline-float-count-snapshot", owner, "Owner")
        .await?
        .to_string();
    let form_name = "count-snapshot-float";
    service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": form_name,
                "fields": {"amount": {"id": 201, "type": "double", "required": true}}
            }),
        )
        .await?;
    let amount_column = ugoite_domain::form::sql_column_name(ugoite_domain::id::FieldId::new(201)?);
    for (id, amount) in [("float-count-zero", 0.0), ("float-count-one", 1.0)] {
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

    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Inline SQL float count".to_string(),
        spec: CompositionSpec {
            variables: vec![
                VariableGraphNode {
                    id: "minimum".to_string(),
                    kind: VariableGraphNodeKind::Input {
                        value_type: scalar_descriptor(VariableGraphScalarType::Float, false),
                        required: true,
                        default: Some(VariableGraphLiteral::Float(0.0)),
                    },
                },
                VariableGraphNode {
                    id: "rows".to_string(),
                    kind: VariableGraphNodeKind::Query {
                        returns: VariableGraphResultType::Table {
                            columns: Vec::new(),
                        },
                        query: VariableGraphQuery {
                            definition: VariableGraphQueryDefinition::InlineSql {
                                sql: format!(
                                    "SELECT \"{amount_column}\" AS amount FROM \"{form_name}\" WHERE \"{amount_column}\" >= $minimum ORDER BY \"{amount_column}\""
                                ),
                            },
                            arguments: vec![ugoite_domain::composition::VariableGraphArgumentBinding {
                                name: "minimum".to_string(),
                                variable: "minimum".to_string(),
                            }],
                            form_bindings: Vec::new(),
                            schema_fingerprint: String::new(),
                        },
                    },
                },
            ],
            components: vec![
                CompositionComponent::Table {
                    id: "rows-table".to_string(),
                    variable: "rows".to_string(),
                },
                CompositionComponent::Input {
                    id: "minimum-input".to_string(),
                    variable: "minimum".to_string(),
                },
            ],
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_string(),
                    items: vec![
                        FlowItem::Component {
                            component: "rows-table".to_string(),
                        },
                        FlowItem::Component {
                            component: "minimum-input".to_string(),
                        },
                    ],
                }],
            },
        },
    };
    let saved_composition = service
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
    let evaluation = service
        .evaluate_composition_graph_authorized_for_principals(
            &space_id,
            &[owner],
            ugoite_iceberg::service::CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved_composition.entry_id.to_string(),
                revision_id: saved_composition.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            Some(&["rows".to_string()]),
            None,
        )
        .await?;
    let count_snapshot = evaluation
        .query_results
        .get("rows")
        .and_then(|result| result.count_snapshot.clone())
        .context("inline SQL Composition evaluation Count snapshot")?;
    let evaluation_request = evaluation
        .plan
        .as_ref()
        .context("saved Composition evaluation retains its resolved query")?
        .queries
        .first()
        .and_then(|query| query.request.clone())
        .context("saved Composition evaluation resolved a SQL request")?;
    assert_eq!(
        evaluation_request.parameter_types,
        BTreeMap::from([("minimum".to_string(), "float".to_string())])
    );
    assert!(evaluation_request.form_binding_guards.is_empty());
    assert_eq!(
        evaluation_request.sql,
        evaluation
            .plan
            .as_ref()
            .expect("saved Composition evaluation retains its resolved query")
            .queries[0]
            .portable_sql
            .as_deref()
            .expect("resolved query exposes its ID-bound SQL")
    );
    service
        .create_structured_entry_with_receipt(
            &space_id,
            "float-count-later-publication",
            form_name.to_string(),
            Vec::new(),
            BTreeMap::from([("amount".to_string(), json!(2.0))]),
            BTreeMap::new(),
            "owner",
        )
        .await?;

    // Browser JSON encodes the evaluated JavaScript number 0 as `0`, while
    // the Rust Float input is represented as `0.0` in the evaluation plan.
    let count_request = |parameters| SqlQueryCountRequest {
        sql: evaluation_request.sql.clone(),
        parameters,
        parameter_types: evaluation_request.parameter_types.clone(),
        saved_sql: None,
        binding_base: evaluation_request.binding_base.clone(),
        form_binding_guards: evaluation_request.form_binding_guards.clone(),
        composition_count_snapshot: Some(count_snapshot.clone()),
    };
    let same_float_count = service
        .count_sql_authorized_for_principals(
            &space_id,
            &[owner],
            count_request(evaluation_request.parameters.clone()),
        )
        .await?;
    assert_eq!(same_float_count, 2);
    let count = service
        .count_sql_authorized_for_principals(
            &space_id,
            &[owner],
            count_request(Map::from_iter([("minimum".to_string(), json!(0))])),
        )
        .await?;
    assert_eq!(count, 2);
    Ok(())
}
