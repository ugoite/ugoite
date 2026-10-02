use super::*;
use crate::authorization::{Authorizer, ResourceKind, ResourceRef};
use crate::service::UgoiteService;
use crate::{iceberg_store, publication_context_for_change};
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::id::{EntryId, FieldId, RevisionId};
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use uuid::Uuid;

async fn seed_raw_compositions(
    service: &UgoiteService,
    space_id: &str,
    owner_id: Uuid,
    entry_ids: &[Uuid],
) -> anyhow::Result<()> {
    let workspace_path = service.workspace_path(space_id);
    let registry = ensure_composition_registry(service.operator(), &workspace_path).await?;
    let field_id = |name: &str| -> anyhow::Result<FieldId> {
        registry
            .fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.id)
            .ok_or_else(|| anyhow::anyhow!("missing Composition carrier field {name}"))
    };
    let name_field = field_id("name")?;
    let kind_field = field_id("kind")?;
    let version_field = field_id("format_version")?;
    let spec_field = field_id("spec")?;
    let committed_at_micros = 1_800_000_000_000_000;
    let change_id = Uuid::now_v7().to_string();
    let author = owner_id.to_string();
    let revisions = entry_ids
        .iter()
        .enumerate()
        .map(|(index, entry_uuid)| EntryRevision {
            form_id: registry.id,
            entry_id: EntryId::from(*entry_uuid),
            revision_id: RevisionId::from(Uuid::from_u128(8_000 + index as u128)),
            change_id: change_id.clone(),
            parent_revision_id: None,
            entry_version: 1,
            expected_version: None,
            operation: EntryOperation::Upsert,
            committed_at_micros: committed_at_micros + index as i64,
            author_id: author.clone(),
            form_version: registry.version,
            source_kind: "test".into(),
            source_id: None,
            entry: EntryMetadata {
                external_id: entry_uuid.to_string(),
                tags: vec![format!("tag-{index}")],
                created_at_micros: committed_at_micros,
                updated_at_micros: committed_at_micros + index as i64,
                updated_by: author.clone(),
                ..EntryMetadata::default()
            },
            values: BTreeMap::from([
                (name_field, FieldValue::String(format!("Raw {index}"))),
                (kind_field, FieldValue::String("dashboard".into())),
                (version_field, FieldValue::Integer(99)),
                (spec_field, FieldValue::String("not: [valid YAML".into())),
            ]),
            extra_attributes: BTreeMap::new(),
            extension_metadata: BTreeMap::new(),
        })
        .collect::<Vec<_>>();
    let change = ChangeCommand {
        change_id,
        run_id: None,
        actor_principal_id: author,
        message: Some("seed raw Composition list entries".into()),
        reverts_change_id: None,
        created_at_micros: committed_at_micros,
    };
    let workspace = iceberg_store::native_workspace(service.operator(), &workspace_path).await?;
    workspace
        .commit(publication_context_for_change(
            &change,
            "test.composition.list-seed",
            &revisions,
        )?)?
        .append_composition_revisions_authorized(registry.id, revisions, None)
        .await?;
    Ok(())
}

#[tokio::test]
async fn composition_list_is_bounded_ordered_raw_and_authorized() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!("memory://composition-list-{}", Uuid::now_v7()))?;
    let owner = Uuid::from_u128(3_429_001);
    let viewer = Uuid::from_u128(3_429_002);
    let space_id = service
        .create_space_for_principal("composition-list", owner, "Owner")
        .await?
        .to_string();
    let entry_ids = [
        Uuid::from_u128(101),
        Uuid::from_u128(202),
        Uuid::from_u128(303),
    ];
    seed_raw_compositions(&service, &space_id, owner, &entry_ids).await?;

    service
        .upsert_form(
            &space_id,
            &json!({"name": "Ordinary", "fields": {"label": {"type": "string"}}}),
        )
        .await?;
    service
        .create_structured_entry_with_receipt(
            &space_id,
            &Uuid::from_u128(404).to_string(),
            "Ordinary".into(),
            Vec::new(),
            BTreeMap::from([("label".to_string(), json!("unrelated"))]),
            BTreeMap::new(),
            &owner.to_string(),
        )
        .await?;

    let first = service
        .list_compositions_local_page(&space_id, 2, 0)
        .await?;
    assert_eq!(first.offset, 0);
    assert_eq!(first.limit, 2);
    assert!(first.has_more);
    assert_eq!(
        first
            .items
            .iter()
            .map(|item| item.entry_id.as_str())
            .collect::<Vec<_>>(),
        [entry_ids[0].to_string(), entry_ids[1].to_string()]
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
    );
    assert_eq!(first.items[0].name, Some(json!("Raw 0")));
    assert_eq!(first.items[0].kind, Some(json!("dashboard")));
    assert_eq!(first.items[0].format_version, Some(json!(99)));
    assert_eq!(first.items[0].tags, ["tag-0"]);
    assert!(first.items.iter().all(|item| {
        !serde_json::to_value(item)
            .expect("list item serializes")
            .as_object()
            .expect("list item is an object")
            .contains_key("spec")
    }));
    let second = service
        .list_compositions_local_page(&space_id, 2, 2)
        .await?;
    assert!(!second.has_more);
    assert_eq!(second.items.len(), 1);
    assert_eq!(second.items[0].entry_id, entry_ids[2].to_string());

    let authorizer = Authorizer::new(service.operator().clone());
    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: viewer,
                kind: PrincipalKind::Human,
                display_name: "Viewer".into(),
                state: PrincipalState::Active,
                created_at: Utc::now().to_rfc3339(),
            },
            SpaceRole::Viewer,
        )
        .await?;
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: entry_ids[1].to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    let visible = service
        .list_compositions_authorized_for_principals_page(&space_id, &[viewer], 10, 0)
        .await?;
    assert_eq!(visible.items.len(), 2);
    assert!(visible
        .items
        .iter()
        .all(|item| item.entry_id != entry_ids[1].to_string()));

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Form,
                id: COMPOSITION_REGISTRY_FORM_NAME.to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    let denied = service
        .list_compositions_authorized_for_principals_page(&space_id, &[viewer], 10, 0)
        .await?;
    assert!(denied.items.is_empty());
    assert!(!denied.has_more);

    Ok(())
}

#[tokio::test]
async fn composition_list_missing_registry_is_read_only_and_pages_are_validated(
) -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-list-empty-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::from_u128(3_429_003);
    let space_id = service
        .create_space_for_principal("composition-list-empty", owner, "Owner")
        .await?
        .to_string();
    let workspace_path = service.workspace_path(&space_id);

    let page = service
        .list_compositions_local_page(&space_id, 10, 0)
        .await?;
    assert!(page.items.is_empty());
    assert!(!page.has_more);
    assert!(
        iceberg_store::native_workspace_read_only(service.operator(), &workspace_path)
            .await?
            .list_forms()
            .await?
            .iter()
            .all(|form| !form
                .name
                .eq_ignore_ascii_case(COMPOSITION_REGISTRY_FORM_NAME))
    );

    for (limit, offset) in [
        (0, 0),
        (COMPOSITION_LIST_MAX_PAGE_SIZE + 1, 0),
        (1, usize::MAX),
    ] {
        let error = service
            .list_compositions_local_page(&space_id, limit, offset)
            .await
            .expect_err("invalid Composition list pagination is rejected");
        assert_eq!(
            error.downcast_ref::<AppError>().unwrap().code(),
            ErrorCode::InvalidInput
        );
    }
    Ok(())
}

#[tokio::test]
async fn composition_list_fails_closed_on_registry_identity_mismatch() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-list-conflict-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::from_u128(3_429_004);
    let space_id = service
        .create_space_for_principal("composition-list-conflict", owner, "Owner")
        .await?
        .to_string();
    let workspace_path = service.workspace_path(&space_id);
    let mut wrong_marker = composition_registry_definition()?;
    wrong_marker
        .extension_metadata
        .insert("ugoite.registry".to_string(), json!("composition.other"));
    crate::form::create_system_form(service.operator(), &workspace_path, &wrong_marker).await?;

    let error = service
        .list_compositions_local_page(&space_id, 10, 0)
        .await
        .expect_err("list must reject a registry marker mismatch");
    assert!(error.to_string().contains("composition_registry_conflict"));
    assert_eq!(
        error.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::CompositionRegistryConflict
    );
    Ok(())
}
