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
    canonicalize_composition, CompositionComponent, CompositionDocument, CompositionFormat,
    CompositionKind, CompositionSpec, DashboardFlowLayout, FlowItem, FlowLayoutKind, FlowRow,
    VariableGraphArgumentBinding, VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind,
    VariableGraphQuery, VariableGraphQueryDefinition, VariableGraphResultType,
    VariableGraphScalarDescriptor, VariableGraphScalarType,
};
use ugoite_domain::form::{sql_column_name, sql_relation_name};
use ugoite_domain::id::{EntryId, FieldId, FormId, RevisionId};
use ugoite_iceberg::saved_sql::{SqlKind, SqlPayload};
use ugoite_iceberg::service::UgoiteService;
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
                && !request.sql.contains("Composition-Preview")
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
    let created = service
        .create_saved_sql(
            &space_id,
            Some(&sql_id.to_string()),
            &SqlPayload {
                name: Some("Minimum value".to_string()),
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
    let revision_id = Uuid::parse_str(
        created
            .get("revision_id")
            .and_then(Value::as_str)
            .context("saved SQL should return its revision ID")?,
    )?;

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
                        required: false,
                        default: Some(VariableGraphLiteral::Integer(7)),
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

    let preview = service
        .preview_composition_local(&space_id, &canonical.yaml, &BTreeMap::new(), &[])
        .await?;
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
    let plan = preview
        .plan
        .context("preview should resolve parameterized SQL")?;
    assert_eq!(plan.queries.len(), 1);
    assert!(matches!(
        plan.queries[0].request.as_ref(),
        Some(request) if request.parameters.get("minimum") == Some(&json!(7))
            && plan.queries[0].schema_fingerprint.len() == 64
    ));

    let document_with_descriptors =
        apply_preview_query_descriptors(&service, &space_id, document.clone()).await?;
    let authorized = service
        .save_composition_authorized_for_principals(
            &space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document_with_descriptors,
            },
            "owner",
            &[Uuid::from_u128(3_434_001)],
        )
        .await?;
    assert!(authorized.document.has_complete_query_descriptors());

    let saved = save_local_composition(&service, &space_id, document).await?;
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
async fn preview_ignores_yaml_form_binding_spoof_with_or_without_exact_base() -> Result<()> {
    let (service, space_id, document, original_form_id) = setup_preview_space().await?;
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

    // Form B has the same result schema, making its forged descriptor
    // plausible. Draft descriptors are never authoritative: without a base,
    // the SQL relation resolves by its current Form identity; with a base,
    // the exact saved Form ID remains pinned.
    let mut forged = saved.document.clone();
    let (expected_returns, expected_schema_fingerprint, expected_form_bindings) =
        match &saved.document.spec.variables[0].kind {
            VariableGraphNodeKind::Query { query, returns } => (
                returns.clone(),
                query.schema_fingerprint.clone(),
                query.form_bindings.clone(),
            ),
            _ => anyhow::bail!("saved Composition should retain its query variable"),
        };
    if let VariableGraphNodeKind::Query { query, returns } = &mut forged.spec.variables[0].kind {
        query.form_bindings = vec![ugoite_domain::composition::VariableGraphFormBinding {
            relation: "Composition-Preview".to_owned(),
            form_id: replacement.form_id,
        }];
        query.schema_fingerprint = "0".repeat(64);
        let VariableGraphResultType::Table { columns } = returns else {
            anyhow::bail!("saved query should retain its table result intent");
        };
        columns[0].value_type = VariableGraphScalarDescriptor {
            scalar_type: VariableGraphScalarType::Integer,
            nullable: false,
        };
    }
    let forged_yaml = canonicalize_composition(&forged)
        .map_err(|code| anyhow::anyhow!(code.as_str()))?
        .yaml;

    let new_preview = service
        .preview_composition_local(&space_id, &forged_yaml, &BTreeMap::new(), &[])
        .await?;
    let new_plan = new_preview
        .plan
        .context("draft should resolve by its current Form relation")?;
    let expected_relation = sql_relation_name(original_form_id);
    let spoofed_relation = sql_relation_name(replacement.form_id);
    assert!(matches!(
        new_plan.queries[0].request.as_ref(),
        Some(request) if request.sql.contains(&expected_relation)
            && !request.sql.contains(&spoofed_relation)
    ));
    assert_eq!(new_plan.queries[0].returns, expected_returns);
    assert_eq!(
        new_plan.queries[0].schema_fingerprint,
        expected_schema_fingerprint
    );
    assert_eq!(new_plan.queries[0].form_bindings, expected_form_bindings);

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
    let plan = preview
        .plan
        .context("spoofed descriptor should be ignored")?;
    assert!(matches!(
        plan.queries[0].request.as_ref(),
        Some(request) if request.sql.contains(&expected_relation)
            && !request.sql.contains(&spoofed_relation)
    ));
    assert_eq!(plan.queries[0].returns, expected_returns);
    assert_eq!(
        plan.queries[0].schema_fingerprint,
        expected_schema_fingerprint
    );
    assert_eq!(plan.queries[0].form_bindings, expected_form_bindings);
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
