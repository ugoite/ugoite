//! Composition draft preview coverage.
//!
//! `preview_composition_local` resolves one unsaved candidate document
//! through the same semantics as a saved revision without creating
//! registry, history, or publication state. The plan identity is the draft
//! fingerprint; no entry or revision reference is minted.

mod common;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use ugoite_core::composition::CompositionRevisionRef;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::composition::{
    canonicalize_composition, CompositionComponent, CompositionDiagnosticCode, CompositionDocument,
    CompositionFormat, CompositionKind, CompositionSpec, DashboardFlowLayout, FlowItem,
    FlowLayoutKind, FlowRow, VariableGraphArgumentBinding, VariableGraphFormBinding,
    VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind, VariableGraphQuery,
    VariableGraphQueryDefinition, VariableGraphResultType, VariableGraphScalarDescriptor,
    VariableGraphScalarType, VariableGraphValidatedResult,
};
use ugoite_domain::form::{sql_column_name, sql_relation_name};
use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
use ugoite_iceberg::saved_sql::{SqlKind, SqlPayload};
use ugoite_iceberg::service::{
    CompositionQueryEvaluationPage, CompositionQueryEvaluationTarget, UgoiteService,
};
use uuid::Uuid;

async fn setup_preview_space() -> Result<(UgoiteService, String, CompositionDocument, FormId)> {
    let op = common::setup_operator()?;
    let service = UgoiteService::from_operator(
        op,
        format!("memory://composition-preview-{}", Uuid::now_v7()),
    );
    let owner = Uuid::from_u128(3_434_001);
    let space_id = service
        .create_space_for_principal("composition-preview", owner, "Owner")
        .await?
        .to_string();

    let form_name = "Composition-Preview";
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
    let field_id = FieldId::new(100)?;
    for index in 0..3 {
        service
            .create_structured_entry_with_receipt(
                &space_id,
                &format!("composition-preview-{index:02}"),
                form_name.to_string(),
                Vec::new(),
                BTreeMap::from([("title".to_string(), Value::String(format!("row-{index}")))]),
                BTreeMap::new(),
                "owner",
            )
            .await?;
    }

    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Preview draft".to_string(),
        spec: CompositionSpec {
            variables: vec![VariableGraphNode {
                id: "source".to_string(),
                kind: VariableGraphNodeKind::Query {
                    returns: VariableGraphResultType::Table { columns: Vec::new() },
                    query: VariableGraphQuery {
                        definition: VariableGraphQueryDefinition::InlineSql {
                            sql: format!(
                                "SELECT \"{}\" AS title FROM \"{form_name}\" ORDER BY \"_ugoite_updated_at\" DESC, \"_ugoite_id\"",
                                sql_column_name(field_id)
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
                presentation: None,
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
    Ok((service, space_id, document, form.form_id))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_empty_form_preview_and_draft_evaluation_keep_form_snapshot_consistent(
) -> Result<()> {
    let op = common::setup_operator()?;
    let service = UgoiteService::from_operator(
        op,
        format!("memory://composition-empty-form-race-{}", Uuid::now_v7()),
    );
    let owner = Uuid::from_u128(3_434_002);
    let space_id = service
        .create_space_for_principal("composition-empty-form-race", owner, "Owner")
        .await?
        .to_string();
    let form_name = format!("Empty-Records-{}", Uuid::now_v7().simple());
    let form = service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": form_name,
                "fields": {
                    "Name": {"id": 100, "type": "string", "required": true},
                    "Quantity": {"id": 101, "type": "integer", "required": true}
                }
            }),
        )
        .await?;
    let variable_id = "empty_rows";
    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Empty Form table".to_owned(),
        spec: CompositionSpec {
            variables: vec![VariableGraphNode {
                id: variable_id.to_owned(),
                kind: VariableGraphNodeKind::Query {
                    returns: VariableGraphResultType::Table {
                        columns: Vec::new(),
                    },
                    query: VariableGraphQuery {
                        definition: VariableGraphQueryDefinition::InlineSql {
                            sql: format!("SELECT * FROM \"{form_name}\""),
                        },
                        arguments: Vec::new(),
                        form_bindings: vec![VariableGraphFormBinding {
                            relation: form_name.clone(),
                            form_id: form.form_id,
                        }],
                        schema_fingerprint: String::new(),
                    },
                },
            }],
            components: vec![CompositionComponent::Table {
                id: "table".to_owned(),
                variable: variable_id.to_owned(),
                presentation: None,
            }],
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_owned(),
                    items: vec![FlowItem::Component {
                        component: "table".to_owned(),
                    }],
                }],
            },
        },
    };
    let yaml = canonicalize_composition(&document)
        .map_err(|code| anyhow::anyhow!(code.as_str()))?
        .yaml;

    // Save starts a metadata preview while the designer may still evaluate
    // the empty Form table. Stress the same snapshot-sensitive service paths
    // together and retain full anyhow chains when either call fails.
    for iteration in 0..24 {
        let service = service.clone();
        let space_id = space_id.clone();
        let yaml = yaml.clone();
        let preview_service = service.clone();
        let preview_space = space_id.clone();
        let preview_yaml = yaml.clone();
        let preview_principals = vec![owner];
        let preview_task = tokio::spawn(async move {
            preview_service
                .preview_composition_authorized_for_principals(
                    &preview_space,
                    &preview_yaml,
                    &BTreeMap::new(),
                    &[],
                    &preview_principals,
                )
                .await
                .map_err(|error| anyhow::anyhow!("preview round {iteration} failed: {error:#}"))
        });
        let evaluation_task = tokio::spawn(async move {
            let principals = vec![owner];
            service
                .evaluate_composition_graph_authorized_for_principals(
                    &space_id,
                    &principals,
                    CompositionQueryEvaluationTarget::Draft { yaml },
                    Uuid::now_v7(),
                    None,
                    None,
                    &BTreeMap::new(),
                    &[],
                    Some(&[variable_id.to_owned()]),
                    None,
                )
                .await
                .map_err(|error| anyhow::anyhow!("evaluation round {iteration} failed: {error:#}"))
        });
        let preview = preview_task.await??;
        let evaluation = evaluation_task.await??;
        assert!(
            preview.plan.is_some(),
            "empty Form table draft should preview"
        );
        assert!(
            evaluation.ok,
            "empty Form table should evaluate: {evaluation:?}"
        );
        assert!(evaluation
            .query_results
            .get(variable_id)
            .is_some_and(|result| matches!(result.result, VariableGraphValidatedResult::Table { ref rows, .. } if rows.is_empty())));
    }
    Ok(())
}

async fn save_local_composition(
    service: &UgoiteService,
    space_id: &str,
    document: CompositionDocument,
) -> Result<ugoite_iceberg::composition::CompositionSaveResult> {
    let document = apply_preview_query_descriptors(service, space_id, document).await?;
    service
        .save_composition_local(
            space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            "preview-test",
        )
        .await
}

async fn save_local_composition_with_work(
    service: &UgoiteService,
    space_id: &str,
    document: CompositionDocument,
    work: ugoite_iceberg::composition::CompositionSaveWork,
) -> Result<ugoite_iceberg::composition::CompositionSaveResult> {
    let document = apply_preview_query_descriptors(service, space_id, document).await?;
    let operation_id = Uuid::now_v7().to_string();
    service
        .save_composition_local_with_work(
            space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            "preview-test",
            &operation_id,
            work,
        )
        .await
}

#[tokio::test]
async fn access_snapshot_reads_exact_revision_without_publishing_or_returning_results() -> Result<()>
{
    use ugoite_domain::identity::AccessPolicy;
    use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};

    let (service, space_id, document, form_id) = setup_preview_space().await?;
    let saved = save_local_composition(&service, &space_id, document).await?;
    let owner = Uuid::from_u128(3_434_001);
    let changes_before = service.list_changes(&space_id).await?;

    let snapshot = service
        .check_composition_access_snapshot_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &[owner],
        )
        .await?;

    assert_eq!(snapshot.composition_entry_id, saved.entry_id.to_string());
    assert_eq!(
        snapshot.composition_revision_id,
        saved.revision_id.to_string()
    );
    assert_eq!(snapshot.publication_fingerprint.len(), 64);
    assert!(snapshot.authorization_fingerprint.starts_with("sha256:"));
    assert_eq!(service.list_changes(&space_id).await?, changes_before);

    service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": "Composition-Preview",
                "fields": {
                    "title": {"id": 100, "type": "string", "required": false},
                    "note": {"type": "string", "required": false}
                }
            }),
        )
        .await?;
    let changed_form = service
        .check_composition_access_snapshot_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &[owner],
        )
        .await?;
    assert_ne!(
        changed_form.publication_fingerprint,
        snapshot.publication_fingerprint
    );
    assert_eq!(
        changed_form.authorization_fingerprint,
        snapshot.authorization_fingerprint
    );

    let authorizer = Authorizer::new(service.operator().clone());
    let form_resource = ResourceRef {
        kind: ResourceKind::Form,
        id: form_id.to_string(),
        parent: None,
    };
    let authorization_before = authorizer.state(&space_id).await?;
    authorizer
        .set_policy(
            &space_id,
            owner,
            &form_resource,
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    let authorization_after = authorizer.state(&space_id).await?;
    assert_eq!(
        authorization_after.revision,
        authorization_before.revision + 1
    );
    let changed_acl = service
        .check_composition_access_snapshot_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &[owner],
        )
        .await?;
    assert_ne!(
        changed_acl.authorization_fingerprint,
        changed_form.authorization_fingerprint
    );
    assert_eq!(
        changed_acl.publication_fingerprint,
        changed_form.publication_fingerprint
    );

    let sql_id = Uuid::from_u128(3_434_003);
    let created_sql = service
        .create_saved_sql(
            &space_id,
            Some(&sql_id.to_string()),
            &SqlPayload {
                name: Some("Lease snapshot SQL".to_owned()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: "SELECT 1 AS value".to_owned(),
                variables: json!([]),
            },
            "owner",
        )
        .await?;
    let created_revision_id = created_sql["revision_id"]
        .as_str()
        .context("Saved SQL create revision ID")?;
    let created_sql_snapshot = service
        .check_composition_access_snapshot_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &[owner],
        )
        .await?;
    assert_ne!(
        created_sql_snapshot.publication_fingerprint,
        changed_acl.publication_fingerprint
    );

    let updated_sql = service
        .update_saved_sql(
            &space_id,
            &sql_id.to_string(),
            &SqlPayload {
                name: Some("Renamed lease snapshot SQL".to_owned()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: "SELECT 2 AS value".to_owned(),
                variables: json!([]),
            },
            created_revision_id,
            "owner",
        )
        .await?;
    assert!(updated_sql["revision_id"].is_string());
    let updated_sql_snapshot = service
        .check_composition_access_snapshot_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &[owner],
        )
        .await?;
    assert_ne!(
        updated_sql_snapshot.publication_fingerprint,
        created_sql_snapshot.publication_fingerprint
    );
    service
        .delete_saved_sql(&space_id, &sql_id.to_string(), "owner")
        .await?;
    let deleted_sql_snapshot = service
        .check_composition_access_snapshot_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &[owner],
        )
        .await?;
    assert_ne!(
        deleted_sql_snapshot.publication_fingerprint,
        updated_sql_snapshot.publication_fingerprint
    );

    let denied = service
        .check_composition_access_snapshot_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &[Uuid::now_v7()],
        )
        .await
        .expect_err("a different principal must not read the snapshot");
    assert!(denied.downcast_ref::<AppError>().is_some());
    Ok(())
}

async fn apply_preview_query_descriptors(
    service: &UgoiteService,
    space_id: &str,
    mut document: CompositionDocument,
) -> Result<CompositionDocument> {
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;
    let preview = service
        .preview_composition_local(space_id, &canonical.yaml, &BTreeMap::new(), &[])
        .await?;
    let Some(plan) = preview.plan else {
        return Ok(document);
    };
    for node in &mut document.spec.variables {
        let VariableGraphNodeKind::Query { query, returns } = &mut node.kind else {
            continue;
        };
        let descriptor = plan
            .queries
            .iter()
            .find(|query| query.variable_id == node.id)
            .with_context(|| format!("preview omitted query variable {}", node.id))?;
        *returns = descriptor.returns.clone();
        query.form_bindings = descriptor.form_bindings.clone();
        query.schema_fingerprint = descriptor.schema_fingerprint.clone();
    }
    Ok(document)
}

#[tokio::test]
async fn draft_preview_resolves_without_publication() -> Result<()> {
    let (service, space_id, document, form_id) = setup_preview_space().await?;
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

    let preview = service
        .preview_composition_local(&space_id, &canonical.yaml, &BTreeMap::new(), &[])
        .await
        .context("draft preview should resolve")?;
    assert!(preview.diagnostics.is_empty());
    assert_eq!(
        preview.draft_fingerprint.as_deref(),
        Some(canonical.fingerprint.as_str())
    );
    let definitions = preview
        .variable_definitions
        .as_deref()
        .context("preview exposes declared graph variables")?;
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].id, "source");
    let plan = preview.plan.context("preview should return a plan")?;
    assert_eq!(plan.draft_fingerprint, canonical.fingerprint);
    assert_eq!(plan.queries.len(), 1);
    assert_eq!(plan.component_bindings.len(), 1);
    let stable_relation = sql_relation_name(form_id);
    assert!(
        matches!(
            plan.queries[0].request.as_ref(),
            Some(request) if request.sql.contains(&stable_relation)
                && request.sql.contains("AS \"Composition-Preview\"")
                && plan.queries[0]
                    .form_bindings
                    .iter()
                    .any(|binding| binding.form_id == form_id)
                && plan.queries[0].schema_fingerprint.len() == 64
        ),
        "resolved query did not use the stable relation and descriptor: {:?}",
        plan.queries[0]
    );

    // Reformatting the same draft keeps the fingerprint; the plan follows.
    let reformatted = format!("{}\n", canonical.yaml);
    let reparsed = service
        .preview_composition_local(&space_id, &reformatted, &BTreeMap::new(), &[])
        .await
        .context("reformatted draft preview should resolve")?;
    assert_eq!(
        reparsed.draft_fingerprint.as_deref(),
        Some(canonical.fingerprint.as_str())
    );

    // Preview creates no registry, history, or publication state.
    let listed = service
        .list_compositions_local_page(&space_id, 100, 0)
        .await?;
    assert!(listed.items.is_empty());
    Ok(())
}

#[tokio::test]
async fn graph_evaluation_commits_no_selected_results_when_one_query_fails() -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    for (id, sql, scalar_type, nullable) in [
        (
            "row_count",
            "SELECT COUNT(*) AS value FROM \"Composition-Preview\"",
            VariableGraphScalarType::Integer,
            false,
        ),
        (
            "empty_value",
            "SELECT \"field_100\" AS value FROM \"Composition-Preview\" WHERE \"field_100\" = 'row-0'",
            VariableGraphScalarType::String,
            true,
        ),
    ] {
        document.spec.variables.push(VariableGraphNode {
            id: id.to_string(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: VariableGraphScalarDescriptor {
                        scalar_type,
                        nullable,
                    },
                },
                query: VariableGraphQuery {
                    definition: VariableGraphQueryDefinition::InlineSql {
                        sql: sql.to_string(),
                    },
                    arguments: Vec::new(),
                    form_bindings: Vec::new(),
                    schema_fingerprint: String::new(),
                },
            },
        });
        document.spec.components.push(CompositionComponent::Metric {
            id: format!("{id}_metric"),
            variable: id.to_string(),
            presentation: None,
        });
        document.spec.layout.rows.push(FlowRow {
            id: format!("{id}_row"),
            items: vec![FlowItem::Component {
                component: format!("{id}_metric"),
            }],
        });
    }

    let saved = save_local_composition(&service, &space_id, document).await?;
    // The strict save preflight validates every query against the current
    // Publication. Change only the source rows afterward to exercise the
    // runtime all-or-nothing result contract when one query no longer matches.
    service
        .delete_entry(&space_id, "composition-preview-00", "preview-test")
        .await?;
    let response = service
        .evaluate_composition_graph_local(
            &space_id,
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            None,
            None,
        )
        .await?;

    assert!(!response.ok);
    assert!(
        response.query_results.is_empty(),
        "a failed selected closure has no values or cursors"
    );
    assert!(response
        .computed_query_ids
        .contains(&"row_count".to_owned()));
    assert!(response
        .computed_query_ids
        .contains(&"empty_value".to_owned()));
    assert!(response
        .query_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.variable_id == "row_count"
            && diagnostic.code.as_str() == "evaluation_aborted"));
    assert!(response.query_diagnostics.iter().any(|diagnostic| {
        diagnostic.variable_id == "empty_value"
            && diagnostic
                .result_diagnostic
                .as_ref()
                .and_then(|result| result.result_diagnostic.as_ref())
                .is_some_and(|result| result.code.as_str() == "scalar_no_rows")
    }));
    // A separate selection can succeed without evaluating the failing branch.
    let independent = service
        .evaluate_composition_graph_local(
            &space_id,
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            Some(&["row_count".to_owned()]),
            None,
        )
        .await?;
    assert!(independent.ok);
    assert_eq!(independent.computed_query_ids, vec!["row_count"]);
    assert!(independent.query_diagnostics.is_empty());
    assert!(matches!(
        independent
            .query_results
            .get("row_count")
            .map(|result| &result.result),
        Some(VariableGraphValidatedResult::Scalar {
            value: VariableGraphLiteral::Integer(2)
        })
    ));
    Ok(())
}

#[tokio::test]
async fn saved_graph_fails_closed_when_form_schema_changes() -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    let VariableGraphNodeKind::Query { query, .. } = &mut document.spec.variables[0].kind else {
        anyhow::bail!("fixture should use an inline SQL query variable");
    };
    query.definition = VariableGraphQueryDefinition::InlineSql {
        sql: "SELECT * FROM \"Composition-Preview\"".to_owned(),
    };
    let original_schema_fingerprint = service
        .preview_composition_local(
            &space_id,
            &canonicalize_composition(&document)
                .map_err(|code| anyhow::anyhow!(code.as_str()))?
                .yaml,
            &BTreeMap::new(),
            &[],
        )
        .await?
        .plan
        .context("original schema preview")?
        .queries[0]
        .schema_fingerprint
        .clone();
    let saved = save_local_composition(&service, &space_id, document.clone()).await?;

    // Adding a field changes the stored SELECT * result descriptor. A saved
    // graph must not silently adopt that new schema during evaluation.
    service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": "Composition-Preview",
                "fields": {
                    "title": {"id": 100, "type": "string", "required": false},
                    "note": {"id": 101, "type": "string", "required": false}
                }
            }),
        )
        .await?;

    let response = service
        .evaluate_composition_graph_local(
            &space_id,
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            None,
            None,
        )
        .await?;

    assert!(!response.ok);
    assert!(response.plan.is_none());
    assert!(response.query_results.is_empty());
    assert!(response.diagnostics.iter().any(|diagnostic| {
        diagnostic.variable_id.as_deref() == Some("source")
            && diagnostic.code == CompositionDiagnosticCode::QuerySchemaChanged
    }));

    // Authoring preview resolves the current schema so the graph can be
    // repaired. Saving that server-derived descriptor creates a new
    // Composition revision that runtime can evaluate without exposing the old
    // result under the changed schema.
    let draft_preview = service
        .preview_composition_local(
            &space_id,
            &canonicalize_composition(&document)
                .map_err(|code| anyhow::anyhow!(code.as_str()))?
                .yaml,
            &BTreeMap::new(),
            &[],
        )
        .await?;
    let current_schema_fingerprint = draft_preview
        .plan
        .as_ref()
        .context("current schema preview")?
        .queries[0]
        .schema_fingerprint
        .clone();
    assert_ne!(current_schema_fingerprint, original_schema_fingerprint);
    let repaired = apply_preview_query_descriptors(&service, &space_id, document).await?;
    let repaired = service
        .save_composition_local(
            &space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: Some(saved.entry_id),
                base_revision_id: Some(saved.revision_id),
                document: repaired,
            },
            "preview-test",
        )
        .await?;
    assert_eq!(repaired.entry_id, saved.entry_id);
    assert_ne!(repaired.revision_id, saved.revision_id);

    // The prior immutable revision remains stale after the repaired revision
    // is appended.
    let old_revision = service
        .evaluate_composition_graph_local(
            &space_id,
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            None,
            None,
        )
        .await?;
    assert!(!old_revision.ok);
    assert!(old_revision.query_results.is_empty());

    let repaired_plan = service
        .resolve_composition_local(
            &space_id,
            &repaired.entry_id.to_string(),
            &repaired.revision_id.to_string(),
            &BTreeMap::new(),
            &[],
        )
        .await?;
    assert_eq!(
        repaired_plan
            .plan
            .context("repaired revision plan")?
            .queries[0]
            .schema_fingerprint,
        current_schema_fingerprint
    );
    let repaired_evaluation = service
        .evaluate_composition_graph_authorized_for_principals(
            &space_id,
            &[Uuid::from_u128(3_434_001)],
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: repaired.entry_id.to_string(),
                revision_id: repaired.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            None,
            None,
        )
        .await?;
    assert!(
        repaired_evaluation.ok,
        "repaired schema should evaluate: {repaired_evaluation:?}"
    );
    assert!(matches!(
        repaired_evaluation
            .query_results
            .get("source")
            .map(|result| &result.result),
        Some(VariableGraphValidatedResult::Table { columns, .. }) if columns.len() > 1
    ));
    Ok(())
}

#[tokio::test]
async fn preview_plans_query_arguments_with_server_derived_upstream_types() -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    let forged_type = VariableGraphScalarDescriptor {
        scalar_type: VariableGraphScalarType::String,
        nullable: false,
    };
    let query = |sql: &str, arguments| VariableGraphNodeKind::Query {
        returns: VariableGraphResultType::Scalar {
            value_type: forged_type,
        },
        query: VariableGraphQuery {
            definition: VariableGraphQueryDefinition::InlineSql {
                sql: sql.to_owned(),
            },
            arguments,
            form_bindings: Vec::new(),
            schema_fingerprint: String::new(),
        },
    };
    document.spec.variables = vec![
        VariableGraphNode {
            id: "downstream".to_owned(),
            kind: query(
                "SELECT $upstream AS value",
                vec![VariableGraphArgumentBinding {
                    name: "upstream".to_owned(),
                    variable: "upstream".to_owned(),
                }],
            ),
        },
        VariableGraphNode {
            id: "upstream".to_owned(),
            kind: query("SELECT 1 AS value", Vec::new()),
        },
    ];
    document.spec.components = vec![CompositionComponent::Metric {
        id: "metric".to_owned(),
        variable: "downstream".to_owned(),
        presentation: None,
    }];
    document.spec.layout.rows[0].items = vec![FlowItem::Component {
        component: "metric".to_owned(),
    }];

    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;
    let preview = service
        .preview_composition_local(&space_id, &canonical.yaml, &BTreeMap::new(), &[])
        .await?;
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
    let plan = preview
        .plan
        .context("preview should resolve query dependencies")?;
    for query in &plan.queries {
        let VariableGraphResultType::Scalar { value_type } = &query.returns else {
            anyhow::bail!("query results should remain scalar");
        };
        assert_eq!(value_type.scalar_type, VariableGraphScalarType::Integer);
    }
    Ok(())
}

#[tokio::test]
async fn local_saved_sql_arguments_accept_planner_derived_scalar_queries() -> Result<()> {
    let (service, space_id, _, _) = setup_preview_space().await?;
    let sql_id = Uuid::from_u128(3_434_002);
    let created = service
        .create_saved_sql(
            &space_id,
            Some(&sql_id.to_string()),
            &SqlPayload {
                name: Some("Integer parameter".to_string()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: "SELECT {{minimum}} AS total".to_string(),
                variables: json!([{
                    "name": "minimum",
                    "type": "integer",
                    "description": "",
                }]),
            },
            "owner",
        )
        .await?;
    let revision_id = created
        .get("revision_id")
        .and_then(Value::as_str)
        .context("Saved SQL should return its revision ID")?;
    let entry_id = EntryId::from_uuid(sql_id).to_string();
    let revision_id = RevisionId::from_uuid(Uuid::parse_str(revision_id)?);

    let document_for = |source_returns, source_sql: &str, name: &str| {
        let source_query = |returns| VariableGraphNodeKind::Query {
            returns,
            query: VariableGraphQuery {
                definition: VariableGraphQueryDefinition::InlineSql {
                    sql: source_sql.to_string(),
                },
                arguments: Vec::new(),
                form_bindings: Vec::new(),
                schema_fingerprint: String::new(),
            },
        };
        CompositionDocument {
            format: CompositionFormat::UgoiteComposition,
            format_version: 1,
            kind: CompositionKind::Dashboard,
            name: name.to_string(),
            spec: CompositionSpec {
                variables: vec![
                    VariableGraphNode {
                        id: "upstream".to_string(),
                        kind: source_query(source_returns),
                    },
                    VariableGraphNode {
                        id: "total".to_string(),
                        kind: VariableGraphNodeKind::Query {
                            returns: VariableGraphResultType::Scalar {
                                value_type: VariableGraphScalarDescriptor {
                                    scalar_type: VariableGraphScalarType::Integer,
                                    nullable: true,
                                },
                            },
                            query: VariableGraphQuery {
                                definition: VariableGraphQueryDefinition::SavedSql {
                                    entry_id: Some(entry_id.clone()),
                                    revision_id: Some(revision_id.to_string()),
                                    column: Some("total".to_string()),
                                },
                                arguments: vec![VariableGraphArgumentBinding {
                                    name: "minimum".to_string(),
                                    variable: "upstream".to_string(),
                                }],
                                form_bindings: Vec::new(),
                                schema_fingerprint: String::new(),
                            },
                        },
                    },
                ],
                components: vec![CompositionComponent::Metric {
                    id: "total_metric".to_string(),
                    variable: "total".to_string(),
                    presentation: None,
                }],
                layout: DashboardFlowLayout {
                    kind: FlowLayoutKind::Flow,
                    rows: vec![FlowRow {
                        id: "main".to_string(),
                        items: vec![FlowItem::Component {
                            component: "total_metric".to_string(),
                        }],
                    }],
                },
            },
        }
    };

    let integer = VariableGraphScalarDescriptor {
        scalar_type: VariableGraphScalarType::Integer,
        nullable: false,
    };
    let saved = save_local_composition(
        &service,
        &space_id,
        document_for(
            VariableGraphResultType::Scalar {
                value_type: integer,
            },
            "SELECT 7 AS value",
            "Scalar query argument",
        ),
    )
    .await?;
    assert!(saved.document.has_complete_query_descriptors());

    let mismatch = save_local_composition(
        &service,
        &space_id,
        document_for(
            VariableGraphResultType::Scalar {
                value_type: VariableGraphScalarDescriptor {
                    scalar_type: VariableGraphScalarType::String,
                    nullable: false,
                },
            },
            "SELECT 'seven' AS value",
            "Mismatched scalar query argument",
        ),
    )
    .await
    .expect_err("a planner-derived string cannot bind an integer parameter");
    assert_eq!(
        mismatch.downcast_ref::<AppError>().map(AppError::code),
        Some(ErrorCode::InvalidInput)
    );

    let saved_compositions = service
        .list_compositions_local_page(&space_id, 100, 0)
        .await?;
    assert_eq!(saved_compositions.items.len(), 1);
    Ok(())
}

#[cfg(debug_assertions)]
struct CompositionFinalAuthorizationGateCleanup {
    gate: std::sync::Arc<ugoite_iceberg::service::TestCompositionFinalAuthorizationGate>,
    evaluation_abort: Option<tokio::task::AbortHandle>,
}

#[cfg(debug_assertions)]
impl Drop for CompositionFinalAuthorizationGateCleanup {
    fn drop(&mut self) {
        self.gate.release();
        ugoite_iceberg::service::clear_test_composition_final_authorization_gate();
        if let Some(abort) = &self.evaluation_abort {
            abort.abort();
        }
    }
}

#[cfg(debug_assertions)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn graph_evaluation_drops_query_results_when_form_access_is_revoked_before_final_check(
) -> Result<()> {
    use ugoite_domain::identity::AccessPolicy;
    use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};

    let (service, space_id, document, form_id) = setup_preview_space().await?;
    let owner = Uuid::from_u128(3_434_001);
    let yaml = canonicalize_composition(&document)
        .map_err(|code| anyhow::anyhow!(code.as_str()))?
        .yaml;
    let gate = ugoite_iceberg::service::TestCompositionFinalAuthorizationGate::new(
        space_id.clone(),
        "source",
    );
    ugoite_iceberg::service::install_test_composition_final_authorization_gate(gate.clone());
    let evaluating_service = service.clone();
    let evaluating_space = space_id.clone();
    let mut evaluation = tokio::spawn(async move {
        evaluating_service
            .evaluate_composition_graph_authorized_for_principals(
                &evaluating_space,
                &[owner],
                CompositionQueryEvaluationTarget::Draft { yaml },
                Uuid::now_v7(),
                None,
                None,
                &BTreeMap::new(),
                &[],
                None,
                None,
            )
            .await
    });
    let mut cleanup = CompositionFinalAuthorizationGateCleanup {
        gate: gate.clone(),
        evaluation_abort: Some(evaluation.abort_handle()),
    };

    let result: Result<()> = async {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            gate.wait_until_entered(),
        )
        .await?;
        assert!(
            gate.staged_row_count() > 0,
            "the gate must be reached only after a nonempty source table result is staged"
        );
        let authorizer = Authorizer::new(service.operator().clone());
        let resource = ResourceRef {
            kind: ResourceKind::Form,
            id: form_id.to_string(),
            parent: None,
        };
        let authorization_before = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            authorizer.state(&space_id),
        )
        .await??;
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            authorizer.set_policy(
                &space_id,
                owner,
                &resource,
                AccessPolicy {
                    policy_id: Uuid::now_v7(),
                    inherit_space_role: false,
                    grants: Vec::new(),
                },
            ),
        )
        .await??;
        gate.release();

        let joined_evaluation =
            tokio::time::timeout(std::time::Duration::from_secs(10), &mut evaluation).await??;
        let error =
            joined_evaluation.expect_err("revoked evaluation must not return query results");
        assert_eq!(
            error
                .downcast_ref::<AppError>()
                .expect("final authorization check returns a typed AppError")
                .kind(),
            ugoite_core::error::ErrorKind::Forbidden
        );
        let authorization_after = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            authorizer.state(&space_id),
        )
        .await??;
        assert_eq!(
            authorization_after.revision,
            authorization_before.revision + 1
        );
        Ok(())
    }
    .await;

    if result.is_err() {
        evaluation.abort();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), &mut evaluation).await;
    }
    if evaluation.is_finished() {
        cleanup.evaluation_abort = None;
    }
    result
}

#[tokio::test]
async fn graph_evaluation_does_not_execute_an_unrequested_query_branch() -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    let integer = VariableGraphScalarDescriptor {
        scalar_type: VariableGraphScalarType::Integer,
        nullable: false,
    };
    let query = |sql: String| VariableGraphNodeKind::Query {
        returns: VariableGraphResultType::Scalar {
            value_type: integer,
        },
        query: VariableGraphQuery {
            definition: VariableGraphQueryDefinition::InlineSql { sql },
            arguments: Vec::new(),
            form_bindings: Vec::new(),
            schema_fingerprint: String::new(),
        },
    };
    let unresolved_metadata_query =
        "SELECT 1 AS value FROM \"Missing-Composition-Form\" LIMIT 1".to_owned();
    document.spec.variables = vec![
        VariableGraphNode {
            id: "selected".to_owned(),
            kind: query("SELECT 1 AS value".to_owned()),
        },
        VariableGraphNode {
            id: "unrelated".to_owned(),
            kind: query(unresolved_metadata_query),
        },
    ];
    document.spec.components = vec![CompositionComponent::Metric {
        id: "metric".to_owned(),
        variable: "selected".to_owned(),
        presentation: None,
    }];
    document.spec.layout.rows[0].items = vec![FlowItem::Component {
        component: "metric".to_owned(),
    }];
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

    let requested = vec!["selected".to_owned()];
    let response = service
        .evaluate_composition_graph_authorized_for_principals(
            &space_id,
            &[Uuid::from_u128(3_434_001)],
            CompositionQueryEvaluationTarget::Draft {
                yaml: canonical.yaml,
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            Some(&requested),
            None,
        )
        .await?;

    assert!(response.ok, "{:?}", response.query_diagnostics);
    assert_eq!(response.computed_query_ids, vec!["selected".to_owned()]);
    assert_eq!(response.query_results.len(), 1);
    assert!(response.query_results.contains_key("selected"));
    assert_eq!(
        response
            .plan
            .as_ref()
            .expect("selected graph plan")
            .queries
            .iter()
            .map(|query| query.variable_id.as_str())
            .collect::<Vec<_>>(),
        vec!["selected"]
    );
    assert!(response.authorization_fingerprint.is_some());
    Ok(())
}

#[tokio::test]
async fn local_graph_table_pages_keep_query_continuations_across_unrelated_input_changes(
) -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    let rows = (0..101)
        .map(|index| format!("SELECT 'page-row-{index:03}' AS title"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let VariableGraphNodeKind::Query { query, .. } = &mut document.spec.variables[0].kind else {
        anyhow::bail!("fixture should contain one table query variable");
    };
    query.definition = VariableGraphQueryDefinition::InlineSql {
        sql: format!("{rows} ORDER BY title"),
    };
    document.spec.variables.push(VariableGraphNode {
        id: "unrelated_input".to_owned(),
        kind: VariableGraphNodeKind::Input {
            value_type: VariableGraphScalarDescriptor {
                scalar_type: VariableGraphScalarType::Integer,
                nullable: false,
            },
            required: true,
            default: None,
        },
    });
    document.spec.components.push(CompositionComponent::Input {
        id: "unrelated-input-component".to_owned(),
        variable: "unrelated_input".to_owned(),
    });
    document.spec.layout.rows[0]
        .items
        .push(FlowItem::Component {
            component: "unrelated-input-component".to_owned(),
        });
    let saved = save_local_composition_with_work(
        &service,
        &space_id,
        document,
        ugoite_iceberg::composition::CompositionSaveWork {
            variables: BTreeMap::from([("unrelated_input".to_owned(), json!(1))]),
            unset_variables: Vec::new(),
        },
    )
    .await?;
    let evaluation_id = Uuid::now_v7();
    let requested_queries = ["source".to_owned()];
    let target = || CompositionQueryEvaluationTarget::SavedRevision {
        composition_id: saved.entry_id.to_string(),
        revision_id: saved.revision_id.to_string(),
    };

    let first = service
        .evaluate_composition_graph_local(
            &space_id,
            target(),
            evaluation_id,
            None,
            None,
            &BTreeMap::from([("unrelated_input".to_owned(), json!(1))]),
            &[],
            Some(&requested_queries),
            None,
        )
        .await?;
    assert!(first.ok, "{:?}", first.query_diagnostics);
    let first_result = first
        .query_results
        .get("source")
        .context("local graph query returns its first table page")?;
    assert!(first_result.has_more);
    let cursor = first_result
        .next
        .clone()
        .context("first local graph page includes a continuation")?;

    let second = service
        .evaluate_composition_graph_local(
            &space_id,
            target(),
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::from([("unrelated_input".to_owned(), json!(2))]),
            &[],
            Some(&requested_queries),
            Some(CompositionQueryEvaluationPage {
                variable_id: "source".to_owned(),
                cursor: cursor.clone(),
            }),
        )
        .await?;
    assert!(second.ok, "{:?}", second.query_diagnostics);
    assert_ne!(second.snapshot_fingerprint, first.snapshot_fingerprint);
    assert_eq!(
        second.publication_fingerprint,
        first.publication_fingerprint
    );
    let second_result = second
        .query_results
        .get("source")
        .context("local graph query returns its continuation page")?;
    assert!(!second_result.has_more);
    let VariableGraphValidatedResult::Table { rows, .. } = &second_result.result else {
        anyhow::bail!("local graph result should remain a table");
    };
    assert_eq!(
        rows.len(),
        1,
        "only the last row from the pinned snapshot remains"
    );
    assert!(!rows.iter().flatten().any(|value| {
        matches!(value, VariableGraphLiteral::String(value) if value == "after-snapshot")
    }));

    for supplied in [
        BTreeMap::new(),
        BTreeMap::from([("unrelated_input".to_owned(), json!("invalid"))]),
    ] {
        let page = service
            .evaluate_composition_graph_local(
                &space_id,
                target(),
                Uuid::now_v7(),
                None,
                None,
                &supplied,
                &["unrelated_input".to_owned()],
                Some(&requested_queries),
                Some(CompositionQueryEvaluationPage {
                    variable_id: "source".to_owned(),
                    cursor: cursor.clone(),
                }),
            )
            .await?;
        assert!(
            page.ok,
            "{:?} {:?}",
            page.diagnostics, page.query_diagnostics
        );
        assert!(!page
            .plan
            .as_ref()
            .context("continuation plan")?
            .variable_values
            .contains_key("unrelated_input"));
        assert!(
            matches!(page.query_results.get("source").map(|result| &result.result), Some(VariableGraphValidatedResult::Table { rows, .. }) if rows.len() == 1)
        );
    }

    // A new Publication invalidates a continuation rather than mixing rows
    // from two storage snapshots into one graph result.
    service
        .create_structured_entry_with_receipt(
            &space_id,
            "composition-preview-after-first-page",
            "Composition-Preview".to_owned(),
            Vec::new(),
            BTreeMap::from([(
                "title".to_string(),
                Value::String("after-snapshot".to_owned()),
            )]),
            BTreeMap::new(),
            "owner",
        )
        .await?;
    let stale = service
        .evaluate_composition_graph_local(
            &space_id,
            target(),
            evaluation_id,
            None,
            None,
            &BTreeMap::from([("unrelated_input".to_owned(), json!(2))]),
            &[],
            Some(&requested_queries),
            Some(CompositionQueryEvaluationPage {
                variable_id: "source".to_owned(),
                cursor,
            }),
        )
        .await;
    assert!(stale.is_err(), "stale graph cursor should be rejected");
    assert!(stale
        .expect_err("stale graph cursor is rejected")
        .to_string()
        .contains("Publication changed during descriptor resolution"));
    Ok(())
}

#[tokio::test]
async fn draft_preview_resolves_sql_schema_without_executing_source_rows() -> Result<()> {
    let (service, space_id, mut document, form_id) = setup_preview_space().await?;
    let VariableGraphNodeKind::Query { query, .. } = &mut document.spec.variables[0].kind else {
        anyhow::bail!("fixture should use an inline SQL query variable");
    };
    query.definition = VariableGraphQueryDefinition::InlineSql {
        sql: format!(
            "SELECT CAST(\"{}\" AS BIGINT) AS numeric_title FROM \"Composition-Preview\"",
            sql_column_name(FieldId::new(100)?)
        ),
    };
    query.form_bindings.clear();
    query.schema_fingerprint.clear();
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

    // The rows contain strings such as `row-0`, so execution of this cast
    // fails. Draft resolution only needs the planned result type and must
    // leave execution to the later preview query.
    let preview = service
        .preview_composition_local(&space_id, &canonical.yaml, &BTreeMap::new(), &[])
        .await
        .context("draft preview should resolve from the planned SQL schema")?;
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
    let plan = preview.plan.context("preview should return a plan")?;
    assert!(matches!(
        plan.queries[0].request.as_ref(),
        Some(request) if request.sql.contains(&sql_relation_name(form_id))
            && plan.queries[0].schema_fingerprint.len() == 64
    ));
    Ok(())
}

#[tokio::test]
async fn inline_sql_planning_rejects_unsupported_decimal_and_json_arguments() -> Result<()> {
    for scalar_type in [
        VariableGraphScalarType::Decimal,
        VariableGraphScalarType::Json,
    ] {
        let (service, space_id, mut document, _) = setup_preview_space().await?;
        let query = document
            .spec
            .variables
            .iter_mut()
            .find(|node| node.id == "source")
            .context("preview fixture query")?;
        let VariableGraphNodeKind::Query { query, .. } = &mut query.kind else {
            anyhow::bail!("preview fixture source must be a query variable");
        };
        let VariableGraphQueryDefinition::InlineSql { sql } = &mut query.definition else {
            anyhow::bail!("preview fixture source must use inline SQL");
        };
        let order_by = sql
            .find(" ORDER BY ")
            .context("preview fixture should include a deterministic order")?;
        sql.insert_str(order_by, " WHERE $threshold IS NOT NULL");
        query.arguments.push(VariableGraphArgumentBinding {
            name: "threshold".to_string(),
            variable: "threshold".to_string(),
        });
        document.spec.variables.push(VariableGraphNode {
            id: "threshold".to_string(),
            kind: VariableGraphNodeKind::Constant {
                value_type: VariableGraphScalarDescriptor {
                    scalar_type,
                    nullable: false,
                },
                value: match scalar_type {
                    VariableGraphScalarType::Decimal => {
                        VariableGraphLiteral::Decimal("1.25".to_string())
                    }
                    VariableGraphScalarType::Json => {
                        VariableGraphLiteral::Json(json!({"value": 1}))
                    }
                    _ => unreachable!("only unsupported scalars are covered"),
                },
            },
        });
        let canonical =
            canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

        let preview_error = service
            .preview_composition_local(&space_id, &canonical.yaml, &BTreeMap::new(), &[])
            .await
            .expect_err("unsupported inline argument type must fail during preview planning");
        assert_eq!(
            preview_error.downcast_ref::<AppError>().map(AppError::code),
            Some(ErrorCode::InvalidInput)
        );

        let save_error = save_local_composition(&service, &space_id, document)
            .await
            .expect_err("unsupported inline argument type must fail during save planning");
        assert_eq!(
            save_error.downcast_ref::<AppError>().map(AppError::code),
            Some(ErrorCode::InvalidInput)
        );
    }
    Ok(())
}

#[tokio::test]
async fn parameterized_saved_sql_placeholder_previews_and_saves_with_server_schema() -> Result<()> {
    let (service, space_id, _, _) = setup_preview_space().await?;
    let sql_id = Uuid::from_u128(3_434_010);
    let pinned_sql_input = "SELECT {{minimum}} AS total";
    let pinned_sql_text = "SELECT $minimum AS total";
    let latest_sql_text = "SELECT {{minimum}} + 100 AS total";
    let created = service
        .create_saved_sql(
            &space_id,
            Some(&sql_id.to_string()),
            &SqlPayload {
                name: Some("Minimum value".to_string()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: pinned_sql_input.to_string(),
                variables: json!([{
                    "name": "minimum",
                    "type": "integer",
                    "description": "",
                }]),
            },
            "owner",
        )
        .await?;
    let revision_id = Uuid::parse_str(
        created
            .get("revision_id")
            .and_then(Value::as_str)
            .context("saved SQL should return its revision ID")?,
    )?;
    let first_revision_id = RevisionId::from_uuid(revision_id).to_string();
    let updated = service
        .update_saved_sql(
            &space_id,
            &sql_id.to_string(),
            &SqlPayload {
                name: Some("Minimum value".to_string()),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql: latest_sql_text.to_string(),
                variables: json!([{
                    "name": "minimum",
                    "type": "integer",
                    "description": "",
                }]),
            },
            &first_revision_id,
            "owner",
        )
        .await?;
    let latest_revision_id = updated
        .get("revision_id")
        .and_then(Value::as_str)
        .context("updated Saved SQL should return its revision ID")?;
    assert_ne!(latest_revision_id, first_revision_id);

    let document = CompositionDocument {
        format: CompositionFormat::UgoiteComposition,
        format_version: 1,
        kind: CompositionKind::Dashboard,
        name: "Parameterized Saved SQL".to_string(),
        spec: CompositionSpec {
            variables: vec![
                VariableGraphNode {
                    id: "minimum".to_string(),
                    kind: VariableGraphNodeKind::Input {
                        value_type: VariableGraphScalarDescriptor {
                            scalar_type: VariableGraphScalarType::Integer,
                            nullable: false,
                        },
                        required: true,
                        default: None,
                    },
                },
                VariableGraphNode {
                    id: "minimum_query".to_string(),
                    kind: VariableGraphNodeKind::Query {
                        returns: VariableGraphResultType::Scalar {
                            value_type: VariableGraphScalarDescriptor {
                                scalar_type: VariableGraphScalarType::Integer,
                                nullable: true,
                            },
                        },
                        query: VariableGraphQuery {
                            definition: VariableGraphQueryDefinition::SavedSql {
                                entry_id: Some(EntryId::from_uuid(sql_id).to_string()),
                                revision_id: Some(RevisionId::from_uuid(revision_id).to_string()),
                                column: Some("total".to_string()),
                            },
                            arguments: vec![VariableGraphArgumentBinding {
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
                CompositionComponent::Input {
                    id: "minimum_input".to_string(),
                    variable: "minimum".to_string(),
                },
                CompositionComponent::Metric {
                    id: "total".to_string(),
                    variable: "minimum_query".to_string(),
                    presentation: None,
                },
            ],
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_string(),
                    items: vec![
                        FlowItem::Component {
                            component: "minimum_input".to_string(),
                        },
                        FlowItem::Component {
                            component: "total".to_string(),
                        },
                    ],
                }],
            },
        },
    };
    assert!(!document.has_complete_query_descriptors());
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

    let principal_ids = [Uuid::from_u128(3_434_001)];
    let preview = service
        .preview_composition_authoring_authorized_for_principals_with_base(
            &space_id,
            &canonical.yaml,
            &BTreeMap::new(),
            &[],
            &principal_ids,
            None,
        )
        .await?;
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
    let plan = preview
        .plan
        .context("preview should resolve parameterized SQL")?;
    assert_eq!(plan.queries.len(), 1);
    assert!(plan.queries[0].request.is_none());
    assert!(!plan.variable_values.contains_key("minimum"));
    assert_eq!(plan.queries[0].sql_text.as_deref(), Some(pinned_sql_text));
    assert_eq!(plan.queries[0].schema_fingerprint.len(), 64);

    let resolved_preview = service
        .preview_composition_authoring_authorized_for_principals_with_base(
            &space_id,
            &canonical.yaml,
            &BTreeMap::from([("minimum".to_owned(), json!(7))]),
            &[],
            &principal_ids,
            None,
        )
        .await?;
    let resolved_plan = resolved_preview.plan.context("bound preview request")?;
    assert!(matches!(
        resolved_plan.queries[0].request.as_ref(),
        Some(request) if request.parameters.get("minimum") == Some(&json!(7))
    ));
    assert_eq!(
        resolved_plan.queries[0].sql_text.as_deref(),
        Some(pinned_sql_text)
    );

    let document_with_descriptors =
        apply_preview_query_descriptors(&service, &space_id, document.clone()).await?;
    let authorized_operation_id = Uuid::now_v7().to_string();
    let authorized = service
        .save_composition_authorized_for_principals_with_work(
            &space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document_with_descriptors,
            },
            "owner",
            &principal_ids,
            &authorized_operation_id,
            ugoite_iceberg::composition::CompositionSaveWork {
                variables: BTreeMap::from([("minimum".to_owned(), json!(7))]),
                unset_variables: Vec::new(),
            },
        )
        .await?;
    assert!(authorized.document.has_complete_query_descriptors());

    let saved = save_local_composition_with_work(
        &service,
        &space_id,
        document,
        ugoite_iceberg::composition::CompositionSaveWork {
            variables: BTreeMap::from([("minimum".to_owned(), json!(7))]),
            unset_variables: Vec::new(),
        },
    )
    .await?;
    assert!(saved.document.has_complete_query_descriptors());
    let Some(VariableGraphNode {
        kind: VariableGraphNodeKind::Query { query, .. },
        ..
    }) = saved
        .document
        .spec
        .variables
        .iter()
        .find(|node| node.id == "minimum_query")
    else {
        anyhow::bail!("saved Composition should retain its Saved SQL query variable");
    };
    assert_eq!(query.schema_fingerprint.len(), 64);

    let requested_queries = ["minimum_query".to_owned()];
    let runtime_without_required_input = service
        .evaluate_composition_graph_authorized_for_principals(
            &space_id,
            &principal_ids,
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::new(),
            &[],
            Some(&requested_queries),
            None,
        )
        .await?;
    assert!(!runtime_without_required_input.ok);
    assert!(runtime_without_required_input
        .diagnostics
        .iter()
        .any(|diagnostic| {
            diagnostic.code
                == ugoite_domain::composition::CompositionDiagnosticCode::VariableMissing
                && diagnostic.variable_id.as_deref() == Some("minimum")
        }));

    let runtime_with_required_input = service
        .evaluate_composition_graph_authorized_for_principals(
            &space_id,
            &principal_ids,
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            Uuid::now_v7(),
            None,
            None,
            &BTreeMap::from([("minimum".to_owned(), json!(7))]),
            &[],
            Some(&requested_queries),
            None,
        )
        .await?;
    assert!(
        runtime_with_required_input.ok,
        "{:?}",
        runtime_with_required_input.query_diagnostics
    );
    assert!(matches!(
        runtime_with_required_input
            .query_results
            .get("minimum_query")
            .map(|result| &result.result),
        Some(VariableGraphValidatedResult::Scalar {
            value: VariableGraphLiteral::Integer(7)
        })
    ));
    Ok(())
}

#[tokio::test]
async fn saves_reject_forged_complete_query_result_descriptors() -> Result<()> {
    let (service, space_id, document, _) = setup_preview_space().await?;
    let mut forged = apply_preview_query_descriptors(&service, &space_id, document).await?;
    let Some(VariableGraphNode {
        kind: VariableGraphNodeKind::Query { returns, .. },
        ..
    }) = forged
        .spec
        .variables
        .iter_mut()
        .find(|node| node.id == "source")
    else {
        anyhow::bail!("fixture should include the source query variable");
    };
    let VariableGraphResultType::Table { columns } = returns else {
        anyhow::bail!("fixture source should return a table");
    };
    columns[0].value_type = VariableGraphScalarDescriptor {
        scalar_type: VariableGraphScalarType::Integer,
        nullable: false,
    };

    let error = service
        .save_composition_local(
            &space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: forged.clone(),
            },
            "preview-test",
        )
        .await
        .expect_err("local save must reject a client-forged result schema");
    assert_eq!(
        error.downcast_ref::<AppError>().map(AppError::code),
        Some(ErrorCode::InvalidInput)
    );

    let error = service
        .save_composition_authorized_for_principals(
            &space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: forged,
            },
            "preview-test",
            &[Uuid::from_u128(3_434_001)],
        )
        .await
        .expect_err("authorized save must reject a client-forged result schema");
    assert_eq!(
        error.downcast_ref::<AppError>().map(AppError::code),
        Some(ErrorCode::InvalidInput)
    );
    Ok(())
}

#[tokio::test]
async fn preview_rejects_mismatched_yaml_form_binding_guard_with_or_without_exact_base(
) -> Result<()> {
    let (service, space_id, document, _) = setup_preview_space().await?;
    let saved = save_local_composition(&service, &space_id, document).await?;
    let replacement = service
        .upsert_form_result(
            &space_id,
            &json!({
                "name": "Composition-Preview-B",
                "fields": {
                    "title": {"id": 100, "type": "string", "required": false}
                }
            }),
        )
        .await?;

    // A mismatched compare-only guard cannot select an unrelated Form, either
    // with a new draft or with the exact Composition base loaded.
    let mut forged = saved.document.clone();
    if let VariableGraphNodeKind::Query { query, .. } = &mut forged.spec.variables[0].kind {
        query.form_bindings = vec![ugoite_domain::composition::VariableGraphFormBinding {
            relation: "Composition-Preview".to_owned(),
            form_id: replacement.form_id,
        }];
    }
    let forged_yaml = canonicalize_composition(&forged)
        .map_err(|code| anyhow::anyhow!(code.as_str()))?
        .yaml;

    let new_preview = service
        .preview_composition_local(&space_id, &forged_yaml, &BTreeMap::new(), &[])
        .await?;
    assert!(
        new_preview.plan.is_none(),
        "a forged guard cannot select another Form"
    );

    let preview = service
        .preview_composition_local_with_base(
            &space_id,
            &forged_yaml,
            &BTreeMap::new(),
            &[],
            Some(CompositionRevisionRef {
                entry_id: saved.entry_id,
                revision_id: saved.revision_id,
            }),
        )
        .await?;
    assert!(
        preview.plan.is_none(),
        "a base pin cannot be overridden by a mismatched guard"
    );
    Ok(())
}

#[tokio::test]
async fn invalid_draft_preview_reports_diagnostics_without_identity() -> Result<()> {
    let (service, space_id, _document, _) = setup_preview_space().await?;
    let preview = service
        .preview_composition_local(&space_id, "not: [valid", &BTreeMap::new(), &[])
        .await
        .context("invalid draft preview should report diagnostics")?;
    assert!(preview.plan.is_none());
    assert_eq!(preview.draft_fingerprint, None);
    assert_eq!(preview.variable_definitions, None);
    assert!(!preview.diagnostics.is_empty());

    let listed = service
        .list_compositions_local_page(&space_id, 100, 0)
        .await?;
    assert!(listed.items.is_empty());
    Ok(())
}

#[tokio::test]
async fn preview_exposes_variable_definitions_in_declaration_order() -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    // Declaration order is region-first; Input placement order is month-first.
    document.spec.components.clear();
    document.spec.layout.rows.clear();
    document.spec.variables = vec![
        VariableGraphNode {
            id: "region".to_string(),
            kind: VariableGraphNodeKind::Input {
                value_type: VariableGraphScalarDescriptor {
                    scalar_type: VariableGraphScalarType::String,
                    nullable: false,
                },
                required: false,
                default: None,
            },
        },
        VariableGraphNode {
            id: "month".to_string(),
            kind: VariableGraphNodeKind::Input {
                value_type: VariableGraphScalarDescriptor {
                    scalar_type: VariableGraphScalarType::String,
                    nullable: false,
                },
                required: true,
                default: None,
            },
        },
    ];
    document.spec.components.extend([
        CompositionComponent::Input {
            id: "month_input".to_string(),
            variable: "month".to_string(),
        },
        CompositionComponent::Input {
            id: "region_input".to_string(),
            variable: "region".to_string(),
        },
    ]);
    document.spec.layout.rows.insert(
        0,
        FlowRow {
            id: "controls".to_string(),
            items: vec![
                FlowItem::Component {
                    component: "month_input".to_string(),
                },
                FlowItem::Component {
                    component: "region_input".to_string(),
                },
            ],
        },
    );
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

    let preview = service
        .preview_composition_local(
            &space_id,
            &canonical.yaml,
            &BTreeMap::from([("month".to_string(), json!("2026-10"))]),
            &[],
        )
        .await
        .context("placed variables should preview")?;
    assert!(preview.diagnostics.is_empty());
    let definitions = preview
        .variable_definitions
        .context("preview exposes declared variable definitions")?;
    let placed_ids: Vec<_> = definitions
        .iter()
        .map(|variable| variable.id.as_str())
        .collect();
    assert_eq!(placed_ids, ["region", "month"]);
    Ok(())
}

#[tokio::test]
async fn portable_inline_sql_count_keeps_form_identity_after_rename_and_replacement() -> Result<()>
{
    let (service, space_id, document, form_id) = setup_preview_space().await?;
    let yaml = canonicalize_composition(&document)
        .expect("valid graph document")
        .yaml;
    let saved = save_local_composition(&service, &space_id, document.clone()).await?;
    let preview = service
        .preview_composition_local(&space_id, &yaml, &BTreeMap::new(), &[])
        .await?;
    let query = preview
        .plan
        .context("authorized query plan")?
        .queries
        .remove(0);
    let sql = query.portable_sql.context("portable SQL uses Form IDs")?;
    assert!(
        sql.contains(&format!(
            "{} AS \"Composition-Preview\"",
            sql_relation_name(form_id)
        )),
        "portable SQL should keep the stable Form relation while retaining its readable alias: {sql}"
    );
    // Exercise the native append-only Form evolution contract; the public
    // name-addressed upsert API is not a Form-rename adapter.
    let workspace = ugoite_iceberg::iceberg_store::native_mutation_workspace(
        service.operator(),
        &service.workspace_path(&space_id),
    )
    .await?;
    let current_form = workspace.load_form(form_id).await?;
    let changes = ugoite_domain::form::FormChangeSet {
        form_id,
        expected_version: Some(current_form.version),
        changes: vec![ugoite_domain::form::FormChange::RenameForm {
            name: "Renamed-Preview".to_owned(),
        }],
    };
    workspace
        .commit(ugoite_iceberg::publication_context(
            "portable-sql-rename",
            "test.form.evolve",
            &changes,
        )?)?
        .evolve_form(&changes)
        .await?;
    let replacement = service.upsert_form_result(&space_id, &json!({
        "name": "Composition-Preview", "fields": { "title": { "id": 100, "type": "string" } }
    })).await?;
    assert_ne!(replacement.form_id, form_id);
    let resolved = service
        .resolve_composition_local(
            &space_id,
            &saved.entry_id.to_string(),
            &saved.revision_id.to_string(),
            &BTreeMap::new(),
            &[],
        )
        .await?;
    let resolved = resolved
        .plan
        .context("saved Composition must retain its Form binding after rename")?;
    assert_eq!(
        resolved.queries[0].portable_sql.as_deref(),
        Some(sql.as_str())
    );
    let count = service
        .count_sql(
            &space_id,
            ugoite_core::sql_query::SqlQueryCountRequest {
                binding_base: None,
                form_binding_guards: Vec::new(),
                sql,
                parameters: Default::default(),
                parameter_types: Default::default(),
                saved_sql: None,
            },
        )
        .await?;
    assert_eq!(
        count, 3,
        "portable SQL must keep the original Form, not its same-name replacement"
    );
    Ok(())
}

#[tokio::test]
async fn portable_saved_sql_copy_keeps_pinned_revision_and_selected_scalar_column() -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    let sql_id = Uuid::now_v7();
    let payload = SqlPayload {
        name: Some("Pinned scalar".to_owned()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT CAST(11 AS BIGINT) AS \"a\"\"b\", CAST(99 AS BIGINT) AS other".to_owned(),
        variables: json!([]),
    };
    let created = service
        .create_saved_sql(&space_id, Some(&sql_id.to_string()), &payload, "owner")
        .await?;
    let revision = created["revision_id"]
        .as_str()
        .context("Saved SQL revision")?;
    service
        .update_saved_sql(
            &space_id,
            &sql_id.to_string(),
            &SqlPayload {
                sql: "SELECT CAST(999 AS BIGINT) AS \"a\"\"b\", CAST(0 AS BIGINT) AS other"
                    .to_owned(),
                ..payload
            },
            revision,
            "owner",
        )
        .await?;
    document.spec.variables[0].kind = VariableGraphNodeKind::Query {
        returns: VariableGraphResultType::Scalar {
            value_type: VariableGraphScalarDescriptor {
                scalar_type: VariableGraphScalarType::Integer,
                nullable: false,
            },
        },
        query: VariableGraphQuery {
            definition: VariableGraphQueryDefinition::SavedSql {
                entry_id: Some(EntryId::from_uuid(sql_id).to_string()),
                revision_id: Some(revision.to_owned()),
                column: Some("a\"b".to_owned()),
            },
            arguments: Vec::new(),
            form_bindings: Vec::new(),
            schema_fingerprint: String::new(),
        },
    };
    document.spec.components[0] = CompositionComponent::Metric {
        id: "table".to_owned(),
        variable: "source".to_owned(),
        presentation: None,
    };
    let yaml = canonicalize_composition(&document)
        .expect("valid graph document")
        .yaml;
    let preview = service
        .preview_composition_local(&space_id, &yaml, &BTreeMap::new(), &[])
        .await?;
    let query = preview
        .plan
        .context("pinned scalar plan")?
        .queries
        .remove(0);
    assert!(query
        .sql_text
        .as_deref()
        .context("pinned SQL")?
        .contains("11"));
    let sql = query.portable_sql.context("independent inline copy")?;
    let page = service
        .query_sql(
            &space_id,
            ugoite_core::sql_query::SqlQueryRequest {
                form_binding_guards: Vec::new(),
                sql,
                limit: 2,
                ..Default::default()
            },
        )
        .await?;
    assert_eq!(page.columns, vec!["a\"b"]);
    assert_eq!(page.rows, vec![json!({"a\"b": 11})]);
    Ok(())
}

#[tokio::test]
async fn authorized_saved_and_draft_query_selection_isolates_unrelated_missing_and_invalid_inputs(
) -> Result<()> {
    let (service, space_id, mut document, _) = setup_preview_space().await?;
    document.spec.variables.push(VariableGraphNode {
        id: "other".to_owned(),
        kind: VariableGraphNodeKind::Input {
            value_type: VariableGraphScalarDescriptor {
                scalar_type: VariableGraphScalarType::Integer,
                nullable: false,
            },
            required: true,
            default: None,
        },
    });
    document.spec.components.push(CompositionComponent::Input {
        id: "other-input".to_owned(),
        variable: "other".to_owned(),
    });
    document.spec.layout.rows[0]
        .items
        .push(FlowItem::Component {
            component: "other-input".to_owned(),
        });
    let document = apply_preview_query_descriptors(&service, &space_id, document).await?;
    let operation_id = Uuid::now_v7().to_string();
    let saved = service
        .save_composition_local_with_work(
            &space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            "preview-test",
            &operation_id,
            ugoite_iceberg::composition::CompositionSaveWork {
                variables: BTreeMap::from([("other".to_owned(), json!(7))]),
                unset_variables: Vec::new(),
            },
        )
        .await?;
    let yaml = canonicalize_composition(&saved.document)
        .map_err(|code| anyhow::anyhow!(code.as_str()))?
        .yaml;
    for supplied in [
        BTreeMap::new(),
        BTreeMap::from([("other".to_owned(), json!("invalid"))]),
        BTreeMap::from([("other".to_owned(), json!(9_223_372_036_854_775_808_u64))]),
    ] {
        for target in [
            CompositionQueryEvaluationTarget::SavedRevision {
                composition_id: saved.entry_id.to_string(),
                revision_id: saved.revision_id.to_string(),
            },
            CompositionQueryEvaluationTarget::Draft { yaml: yaml.clone() },
        ] {
            let result = service
                .evaluate_composition_graph_authorized_for_principals(
                    &space_id,
                    &[Uuid::from_u128(3_434_001)],
                    target,
                    Uuid::now_v7(),
                    None,
                    None,
                    &supplied,
                    &[],
                    Some(&["source".to_owned()]),
                    None,
                )
                .await?;
            assert!(
                result.ok,
                "{:?} {:?}",
                result.diagnostics, result.query_diagnostics
            );
            assert_eq!(result.computed_query_ids, ["source"]);
            assert!(!result
                .plan
                .as_ref()
                .context("authorized evaluation plan")?
                .variable_values
                .contains_key("other"));
        }
    }
    Ok(())
}
