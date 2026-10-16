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
use ugoite_domain::composition::{
    canonicalize_composition, CompositionComponent, CompositionDocument, CompositionFormBinding,
    CompositionFormat, CompositionKind, CompositionSource, CompositionSpec, CompositionValue,
    CompositionVariable, CompositionVariableReference, CompositionVariableType,
    DashboardFlowLayout, FlowItem, FlowLayoutKind, FlowRow,
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
            variables: Vec::new(),
            sources: vec![CompositionSource::InlineSql {
                id: "source".to_string(),
                name: "Preview rows".to_string(),
                sql: format!(
                    "SELECT \"{}\" FROM \"{form_name}\" ORDER BY \"_ugoite_updated_at\" DESC, \"_ugoite_id\"",
                    sql_column_name(field_id)
                ),
                form_bindings: Vec::new(),
                expected_result: Vec::new(),
                schema_fingerprint: String::new(),
            }],
            components: vec![CompositionComponent::Table {
                id: "table".to_string(),
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
    Ok((service, space_id, document, form.form_id))
}

async fn save_local_composition(
    service: &UgoiteService,
    space_id: &str,
    document: CompositionDocument,
) -> Result<ugoite_iceberg::composition::CompositionSaveResult> {
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
    assert!(preview
        .variable_definitions
        .is_some_and(|definitions| definitions.is_empty()));
    let plan = preview.plan.context("preview should return a plan")?;
    assert_eq!(plan.draft_fingerprint, canonical.fingerprint);
    assert_eq!(plan.sources.len(), 1);
    assert_eq!(plan.component_bindings.len(), 1);
    let stable_relation = sql_relation_name(form_id);
    assert!(
        matches!(
            &plan.sources[0],
            ugoite_core::composition::ResolvedSourceRequest::Sql {
                request,
                source_schema_fingerprint,
                ..
        } if request.sql.contains(&stable_relation)
            && !request.sql.contains("Composition-Preview")
                && source_schema_fingerprint.len() == 64
        ),
        "resolved source did not use the stable relation and descriptor: {:?}",
        plan.sources[0]
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
async fn draft_preview_resolves_sql_schema_without_executing_source_rows() -> Result<()> {
    let (service, space_id, mut document, form_id) = setup_preview_space().await?;
    let CompositionSource::InlineSql { sql, .. } = &mut document.spec.sources[0] else {
        anyhow::bail!("fixture should use inline SQL");
    };
    *sql = format!(
        "SELECT CAST(\"{}\" AS BIGINT) AS numeric_title FROM \"Composition-Preview\"",
        sql_column_name(FieldId::new(100)?)
    );
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
        &plan.sources[0],
        ugoite_core::composition::ResolvedSourceRequest::Sql {
            request,
            source_schema_fingerprint,
            ..
        } if request.sql.contains(&sql_relation_name(form_id))
            && source_schema_fingerprint.len() == 64
    ));
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
            variables: vec![CompositionVariable {
                id: "minimum".to_string(),
                variable_type: CompositionVariableType::Integer,
                required: false,
                default: None,
                format: None,
            }],
            sources: vec![CompositionSource::SavedSql {
                id: "minimum_query".to_string(),
                entry_id: EntryId::from_uuid(sql_id),
                revision_id: RevisionId::from_uuid(revision_id),
                expected_result: Vec::new(),
                schema_fingerprint: String::new(),
                variables: BTreeMap::from([(
                    "minimum".to_string(),
                    CompositionValue::Variable(CompositionVariableReference {
                        variable: "minimum".to_string(),
                    }),
                )]),
            }],
            components: vec![CompositionComponent::Metric {
                id: "total".to_string(),
                source: "minimum_query".to_string(),
                column: "total".to_string(),
                presentation: None,
            }],
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "main".to_string(),
                    items: vec![FlowItem::Component {
                        component: "total".to_string(),
                    }],
                }],
            },
        },
    };
    assert!(!document.has_complete_source_descriptors());
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

    let preview = service
        .preview_composition_local(
            &space_id,
            &canonical.yaml,
            &BTreeMap::from([("minimum".to_string(), json!(7))]),
            &[],
        )
        .await?;
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
    let plan = preview
        .plan
        .context("preview should resolve parameterized SQL")?;
    assert_eq!(plan.sources.len(), 1);
    assert!(matches!(
        &plan.sources[0],
        ugoite_core::composition::ResolvedSourceRequest::Sql {
            request,
            source_schema_fingerprint,
            ..
        } if request.parameters.get("minimum") == Some(&json!(7))
            && source_schema_fingerprint.len() == 64
    ));

    let authorized = service
        .save_composition_authorized_for_principals(
            &space_id,
            ugoite_iceberg::composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document.clone(),
            },
            "owner",
            &[Uuid::from_u128(3_434_001)],
        )
        .await?;
    assert!(authorized.document.has_complete_source_descriptors());

    let saved = save_local_composition(&service, &space_id, document).await?;
    assert!(saved.document.has_complete_source_descriptors());
    let CompositionSource::SavedSql {
        expected_result,
        schema_fingerprint,
        ..
    } = &saved.document.spec.sources[0]
    else {
        anyhow::bail!("saved Composition should retain its Saved SQL source");
    };
    assert_eq!(expected_result.len(), 1);
    assert_eq!(expected_result[0].name, "total");
    assert_eq!(schema_fingerprint.len(), 64);
    Ok(())
}

#[tokio::test]
async fn preview_ignores_yaml_form_binding_spoof_and_keeps_exact_base_form() -> Result<()> {
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
    // plausible. Only the exact saved base can establish the prior Form ID.
    let mut forged = saved.document.clone();
    if let CompositionSource::InlineSql { form_bindings, .. } = &mut forged.spec.sources[0] {
        *form_bindings = vec![CompositionFormBinding {
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
    let new_plan = new_preview
        .plan
        .context("new preview should resolve by current Form name")?;
    let expected_relation = sql_relation_name(original_form_id);
    let spoofed_relation = sql_relation_name(replacement.form_id);
    assert!(matches!(
        &new_plan.sources[0],
        ugoite_core::composition::ResolvedSourceRequest::Sql { request, .. }
            if request.sql.contains(&expected_relation)
                && !request.sql.contains(&spoofed_relation)
    ));

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
        &plan.sources[0],
        ugoite_core::composition::ResolvedSourceRequest::Sql { request, .. }
            if request.sql.contains(&expected_relation)
                && !request.sql.contains(&spoofed_relation)
    ));
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
    document.spec.variables = vec![
        CompositionVariable {
            id: "region".to_string(),
            variable_type: CompositionVariableType::String,
            required: false,
            default: None,
            format: None,
        },
        CompositionVariable {
            id: "month".to_string(),
            variable_type: CompositionVariableType::String,
            required: true,
            default: None,
            format: None,
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
