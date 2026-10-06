mod common;

use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use ugoite_core::entry_query::EntryPageRequest;
use ugoite_domain::composition::{
    CompositionComponent, CompositionDocument, CompositionFieldSchemaEntry, CompositionFormat,
    CompositionKind, CompositionSortDirection, CompositionSource, CompositionSpec,
    DashboardFlowLayout, EntryQueryProjectionTemplate, EntryQuerySortTemplate, EntryQueryTemplate,
    FlowItem, FlowLayoutKind, FlowRow,
};
use ugoite_domain::form::FieldType;
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::composition::CompositionSaveRequest;
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
                grants: Vec::new(),
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
    Ok(())
}
