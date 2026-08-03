//! Phase 6 service-boundary coverage for server and CLI adapters.

use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::saved_sql::SqlPayload;
use ugoite_iceberg::service::UgoiteService;
use uuid::Uuid;

#[tokio::test]
async fn test_service_boundary_covers_primary_adapter_operations() -> Result<()> {
    let service = UgoiteService::new("memory://core-service-boundary")?;

    service.create_space("demo").await?;
    let spaces = service.list_space_ids().await?;
    assert_eq!(spaces, vec!["demo"]);

    service
        .upsert_form(
            "demo",
            &serde_json::json!({
                "name": "Note",
                "fields": {
                    "Body": {"type": "markdown"}
                }
            }),
        )
        .await?;
    assert_eq!(service.get_form("demo", "Note").await?["name"], "Note");

    let created = service
        .create_entry(
            "demo",
            "first",
            "---\nform: Note\n---\n# First\n\n## Body\nhello service",
            "test",
        )
        .await?;
    assert_eq!(created["id"], "first");

    let entries = service.list_entries("demo").await?;
    assert_eq!(entries.len(), 1);

    let search = service.search_entries("demo", "service").await?;
    assert_eq!(search.len(), 1);
    assert_eq!(search[0].id, "first");

    let asset = service.save_asset("demo", "hello.txt", b"hello").await?;
    let content = service.read_asset("demo", &asset.asset_id).await?;
    assert_eq!(content.bytes, b"hello");

    service.delete_asset("demo", &asset.asset_id).await?;

    Ok(())
}

#[tokio::test]
async fn authorized_entry_reads_find_entries_after_creating_multiple_asset_forms() -> Result<()> {
    let service = UgoiteService::new("memory://authorized-multiple-asset-forms")?;
    let owner = Uuid::from_u128(301);
    let space_id = service
        .create_space_for_principal("authorized-multiple-asset-forms", owner, "Owner")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "MediaAssets",
                "fields": {
                    "thumbnail": {"type": "asset_reference", "required": true},
                    "microscope_images": {
                        "type": "list",
                        "required": true,
                        "items": {"type": "asset_reference"}
                    }
                }
            }),
        )
        .await?;
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "ContractsAssets",
                "fields": {
                    "contract": {"type": "asset_reference", "required": true},
                    "raw_data": {
                        "type": "list",
                        "required": true,
                        "items": {"type": "asset_reference"}
                    }
                }
            }),
        )
        .await?;

    let thumbnail = service
        .save_asset(&space_id, "thumbnail.txt", b"thumbnail")
        .await?;
    let microscope_a = service
        .save_asset(&space_id, "microscope-a.txt", b"a")
        .await?;
    let contract = service
        .save_asset(&space_id, "contract.pdf", b"contract")
        .await?;
    let raw_data = service
        .save_asset(&space_id, "raw-data.csv", b"raw")
        .await?;

    let media_id = "media-entry";
    service
        .create_entry_authorized_for_principals(
            &space_id,
            media_id,
            &format!(
                "---\nform: MediaAssets\nthumbnail: {}\nmicroscope_images: [{}]\n---\n# Media",
                serde_json::to_string(&thumbnail)?,
                serde_json::to_string(&microscope_a)?
            ),
            "owner",
            &[owner],
        )
        .await?;
    assert_eq!(
        service
            .get_entry_authorized_for_principals(&space_id, media_id, &[owner])
            .await?["id"],
        media_id
    );

    let contracts_id = "contracts-entry";
    service
        .create_entry_authorized_for_principals(
            &space_id,
            contracts_id,
            &format!(
                "---\nform: ContractsAssets\ncontract: {}\nraw_data: [{}]\n---\n# Contracts",
                serde_json::to_string(&contract)?,
                serde_json::to_string(&raw_data)?
            ),
            "owner",
            &[owner],
        )
        .await?;
    assert_eq!(
        service
            .get_entry_authorized_for_principals(&space_id, contracts_id, &[owner])
            .await?["id"],
        contracts_id
    );
    Ok(())
}

#[tokio::test]
async fn authorized_entry_writes_apply_form_entry_and_delegated_principal_policies() -> Result<()> {
    let service = UgoiteService::new("memory://authorized-entry-writes")?;
    let owner = Uuid::from_u128(201);
    let editor = Uuid::from_u128(202);
    let space_id = service
        .create_space_for_principal("authorized-entry-writes", owner, "Owner")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &serde_json::json!({
                "name": "Note",
                "fields": {"Body": {"type": "markdown"}}
            }),
        )
        .await?;
    service
        .create_entry(
            &space_id,
            "note-1",
            "---\nform: Note\n---\n# Initial\n\n## Body\nInitial",
            "owner",
        )
        .await?;
    let initial_revision = service.get_entry(&space_id, "note-1").await?["revision_id"]
        .as_str()
        .expect("initial revision")
        .to_string();

    let authorizer = Authorizer::new(service.operator().clone());
    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: editor,
                kind: PrincipalKind::Human,
                display_name: "Editor".to_string(),
                state: PrincipalState::Active,
                created_at: Utc::now().to_rfc3339(),
            },
            SpaceRole::Editor,
        )
        .await?;
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Form,
                id: "Note".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    assert!(service
        .create_entry_authorized_for_principals(
            &space_id,
            "denied-note",
            "---\nform: Note\n---\n# Denied",
            "editor",
            &[editor],
        )
        .await
        .is_err());
    assert!(service
        .update_entry_authorized_for_principals(
            &space_id,
            "note-1",
            "---\nform: Note\n---\n# Denied",
            None,
            "editor",
            &[editor],
        )
        .await
        .is_err());
    assert!(service
        .restore_entry_authorized_for_principals(
            &space_id,
            "note-1",
            &initial_revision,
            "editor",
            &[editor],
        )
        .await
        .is_err());

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Form,
                id: "Note".to_string(),
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
                id: "note-1".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    assert!(service
        .update_entry_authorized_for_principals(
            &space_id,
            "note-1",
            "---\nform: Note\n---\n# Entry denied",
            None,
            "owner",
            &[owner, editor],
        )
        .await
        .is_err());

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Entry,
                id: "note-1".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: true,
                grants: Vec::new(),
            },
        )
        .await?;
    service
        .update_entry_authorized_for_principals(
            &space_id,
            "note-1",
            "---\nform: Note\n---\n# Delegated update",
            None,
            "owner",
            &[owner, editor],
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn saved_sql_acl_is_applied_before_payload_decode() -> Result<()> {
    let service = UgoiteService::new("memory://saved-sql-acl-boundary")?;
    let owner = Uuid::from_u128(301);
    let viewer = Uuid::from_u128(302);
    let space_id = service
        .create_space_for_principal("saved-sql-acl", owner, "Owner")
        .await?
        .to_string();
    let payload = |name: &str| SqlPayload {
        name: name.to_string(),
        sql: "SELECT 1".to_string(),
        variables: json!([]),
    };
    service
        .create_saved_sql(&space_id, "visible", &payload("Visible"), "owner")
        .await?;
    service
        .create_saved_sql(&space_id, "hidden", &payload("Hidden"), "owner")
        .await?;
    assert_eq!(
        service
            .authorized_saved_sql_entry_scope_for_principals(&space_id, &[])
            .await?,
        ugoite_core::query::EntryScope::Only(std::collections::BTreeSet::new())
    );

    let authorizer = Authorizer::new(service.operator().clone());
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
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::SavedSql,
                id: "hidden".to_string(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;

    let scope = service
        .authorized_saved_sql_entry_scope_for_principals(&space_id, &[viewer])
        .await?;
    let hidden_entry_id = Uuid::new_v5(&Uuid::NAMESPACE_URL, b"hidden").into();
    assert!(matches!(
        &scope,
        ugoite_core::query::EntryScope::AllExcept(ids) if ids.contains(&hidden_entry_id)
    ));
    let listed = service
        .list_saved_sql_authorized_for_principals(&space_id, &[viewer])
        .await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["id"], "visible");
    Ok(())
}
