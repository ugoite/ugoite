//! Phase 6 service-boundary coverage for server and CLI adapters.

use anyhow::Result;
use chrono::Utc;
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
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
