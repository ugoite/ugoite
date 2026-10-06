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
use ugoite_domain::composition::{
    canonicalize_composition, CompositionComponent, CompositionDocument, CompositionFormat,
    CompositionKind, CompositionParameter, CompositionParameterType, CompositionSource,
    CompositionSpec, DashboardFlowLayout, EntryQueryProjectionTemplate, EntryQueryTemplate,
    FlowItem, FlowLayoutKind, FlowRow,
};
use ugoite_domain::form::FieldType;
use ugoite_domain::id::FieldId;
use ugoite_iceberg::service::UgoiteService;
use uuid::Uuid;

async fn setup_preview_space() -> Result<(UgoiteService, String, CompositionDocument)> {
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
        tags: vec!["draft".to_string()],
        spec: CompositionSpec {
            parameters: Vec::new(),
            sources: vec![CompositionSource::EntryQuery {
                id: "source".to_string(),
                form_id: form.form_id,
                field_schema: vec![ugoite_domain::composition::CompositionFieldSchemaEntry {
                    field_id,
                    field_type: FieldType::String,
                    reference_form: None,
                    list_item: None,
                }],
                query: EntryQueryTemplate {
                    text: None,
                    filters: Vec::new(),
                    sort: Vec::new(),
                    page_limit: 100,
                    projection: EntryQueryProjectionTemplate::Preview,
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
    Ok((service, space_id, document))
}

#[tokio::test]
async fn draft_preview_resolves_without_publication() -> Result<()> {
    let (service, space_id, document) = setup_preview_space().await?;
    let canonical =
        canonicalize_composition(&document).map_err(|code| anyhow::anyhow!(code.as_str()))?;

    let preview = service
        .preview_composition_local(&space_id, &canonical.yaml, &BTreeMap::new())
        .await
        .context("draft preview should resolve")?;
    assert!(preview.diagnostics.is_empty());
    assert_eq!(
        preview.draft_fingerprint.as_deref(),
        Some(canonical.fingerprint.as_str())
    );
    assert!(preview
        .parameter_definitions
        .is_some_and(|definitions| definitions.is_empty()));
    let plan = preview.plan.context("preview should return a plan")?;
    assert_eq!(plan.draft_fingerprint, canonical.fingerprint);
    assert_eq!(plan.sources.len(), 1);
    assert_eq!(plan.component_bindings.len(), 1);

    // Reformatting the same draft keeps the fingerprint; the plan follows.
    let reformatted = format!("{}\n", canonical.yaml);
    let reparsed = service
        .preview_composition_local(&space_id, &reformatted, &BTreeMap::new())
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
async fn invalid_draft_preview_reports_diagnostics_without_identity() -> Result<()> {
    let (service, space_id, _document) = setup_preview_space().await?;
    let preview = service
        .preview_composition_local(&space_id, "not: [valid", &BTreeMap::new())
        .await
        .context("invalid draft preview should report diagnostics")?;
    assert!(preview.plan.is_none());
    assert_eq!(preview.draft_fingerprint, None);
    assert_eq!(preview.parameter_definitions, None);
    assert!(!preview.diagnostics.is_empty());

    let listed = service
        .list_compositions_local_page(&space_id, 100, 0)
        .await?;
    assert!(listed.items.is_empty());
    Ok(())
}

#[tokio::test]
async fn preview_exposes_placed_parameters_in_layout_order() -> Result<()> {
    let (service, space_id, mut document) = setup_preview_space().await?;
    // Declaration order is region-first; placement order is month-first.
    document.spec.parameters = vec![
        CompositionParameter {
            id: "region".to_string(),
            label: None,
            parameter_type: CompositionParameterType::String,
            required: false,
            default: None,
            format: None,
        },
        CompositionParameter {
            id: "month".to_string(),
            label: None,
            parameter_type: CompositionParameterType::String,
            required: true,
            default: None,
            format: None,
        },
    ];
    document.spec.layout.rows.insert(
        0,
        FlowRow {
            id: "controls".to_string(),
            items: vec![
                FlowItem::Parameter {
                    parameter: "month".to_string(),
                },
                FlowItem::Parameter {
                    parameter: "region".to_string(),
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
        )
        .await
        .context("placed parameters should preview")?;
    assert!(preview.diagnostics.is_empty());
    let definitions = preview
        .parameter_definitions
        .context("preview exposes semantic parameter definitions")?;
    let placed_ids: Vec<_> = definitions
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect();
    assert_eq!(placed_ids, ["month", "region"]);
    Ok(())
}
