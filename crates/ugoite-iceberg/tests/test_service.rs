//! Phase 6 service-boundary coverage for server and CLI adapters.

mod common;

use common::LegacyServiceEntryExt;

use anyhow::Result;
use chrono::Utc;
use serde_json::json;
use std::collections::BTreeMap;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::structured_search::StructuredSearch;
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::saved_sql::{SqlKind, SqlPayload};
use ugoite_iceberg::service::UgoiteService;
use uuid::Uuid;

fn semantic_error_code(error: anyhow::Error) -> ErrorCode {
    error
        .downcast::<AppError>()
        .expect("entry write failures must be typed AppErrors")
        .code()
}

/// Raw and structured writes share one admission/auth prelude, so the same
/// denied principal fails with the same semantic error code on both paths.
#[tokio::test]
async fn raw_and_structured_denied_principal_share_semantic_error_code() -> Result<()> {
    let service = UgoiteService::new("memory://raw-structured-denied-parity")?;
    let owner = Uuid::from_u128(301);
    let editor = Uuid::from_u128(302);
    let space_id = service
        .create_space_for_principal("denied-parity", owner, "Owner")
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
        .create_entry_authorized_for_principals(
            &space_id,
            "denied-note",
            "---\nform: Note\n---\n# Denied\n\n## Body\nInitial",
            "owner",
            &[owner],
        )
        .await?;

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

    let raw_create = service
        .create_entry_authorized_for_principals(
            &space_id,
            "denied-raw",
            "---\nform: Note\n---\n# Denied",
            "editor",
            &[editor],
        )
        .await
        .expect_err("denied raw create must fail");
    let mut structured_fields = BTreeMap::new();
    structured_fields.insert("Body".to_string(), json!("Denied"));
    let structured_create = service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "denied-structured",
            "Note".to_string(),
            Vec::new(),
            structured_fields,
            BTreeMap::new(),
            "editor",
            &[editor],
        )
        .await
        .expect_err("denied structured create must fail");
    assert_eq!(
        semantic_error_code(raw_create),
        semantic_error_code(structured_create)
    );

    let raw_update = service
        .update_entry_authorized_for_principals(
            &space_id,
            "denied-note",
            "---\nform: Note\n---\n# Denied",
            None,
            "editor",
            &[editor],
        )
        .await
        .expect_err("denied raw update must fail");
    let mut structured_update_fields = BTreeMap::new();
    structured_update_fields.insert("Body".to_string(), json!("Denied"));
    let structured_update = service
        .update_structured_entry_authorized_for_principals(
            &space_id,
            "denied-note",
            None,
            None,
            structured_update_fields,
            BTreeMap::new(),
            None,
            "editor",
            &[editor],
        )
        .await
        .expect_err("denied structured update must fail");
    assert_eq!(
        semantic_error_code(raw_update),
        semantic_error_code(structured_update)
    );
    Ok(())
}

/// The same admissible update through raw Markdown and structured fields
/// reaches the same durable revision representation.
#[tokio::test]
async fn raw_and_structured_admissible_updates_reach_same_durable_outcome() -> Result<()> {
    let service = UgoiteService::new("memory://raw-structured-update-parity")?;
    let owner = Uuid::from_u128(303);
    let space_id = service
        .create_space_for_principal("update-parity", owner, "Owner")
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
    for entry_id in ["parity-raw", "parity-structured"] {
        service
            .create_entry_authorized_for_principals(
                &space_id,
                entry_id,
                "---\nform: Note\n---\n# Parity\n\n## Body\nInitial",
                "owner",
                &[owner],
            )
            .await?;
    }

    service
        .update_entry_authorized_for_principals(
            &space_id,
            "parity-raw",
            "---\nform: Note\n---\n# Parity\n\n## Body\nUpdated",
            None,
            "owner",
            &[owner],
        )
        .await?;
    let mut fields = BTreeMap::new();
    fields.insert("Body".to_string(), json!("Updated"));
    service
        .update_structured_entry_authorized_for_principals(
            &space_id,
            "parity-structured",
            None,
            None,
            fields,
            BTreeMap::new(),
            None,
            "owner",
            &[owner],
        )
        .await?;

    let raw = service.get_entry(&space_id, "parity-raw").await?;
    let structured = service.get_entry(&space_id, "parity-structured").await?;
    for key in ["content", "frontmatter", "sections"] {
        assert_eq!(raw[key], structured[key], "durable {key} must agree");
    }
    Ok(())
}

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
        name: Some(name.to_string()),
        kind: SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT 1".to_string(),
        variables: json!([]),
    };
    service
        .create_saved_sql(&space_id, "visible", &payload("Visible"), "owner")
        .await?;
    service
        .create_saved_sql(&space_id, "hidden", &payload("Hidden"), "owner")
        .await?;
    service
        .authorized_saved_sql_entry_scope_for_principals(&space_id, &[])
        .await
        .expect_err("authorized saved-SQL reads must reject an empty principal set");

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

#[tokio::test]
async fn authorized_structured_search_has_exact_policy_filtered_rows_and_no_form_leak() -> Result<()>
{
    let service = UgoiteService::new("memory://authorized-structured-search-contract")?;
    let owner = Uuid::from_u128(401);
    let viewer = Uuid::from_u128(402);
    let space_id = service
        .create_space_for_principal("authorized-structured-search-contract", owner, "Owner")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "Task",
                "fields": {"summary": {"type": "string"}}
            }),
        )
        .await?;
    service
        .create_entry(
            &space_id,
            "visible-task",
            "---\nform: Task\nsummary: visible\n---\n# Visible",
            "owner",
        )
        .await?;
    service
        .create_entry(
            &space_id,
            "hidden-task",
            "---\nform: Task\nsummary: hidden\n---\n# Hidden",
            "owner",
        )
        .await?;

    let criteria = StructuredSearch {
        form: "Task".to_owned(),
        updated_from: None,
        updated_to: None,
        conditions: Vec::new(),
        limit: Some(100),
        offset: None,
    };
    let direct = service.search_structured(&space_id, &criteria).await?;
    let direct_ids = direct
        .iter()
        .map(|row| row["_ugoite_id"].as_str().expect("Entry id").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(direct_ids, vec!["hidden-task", "visible-task"]);

    let authorizer = Authorizer::new(service.operator().clone());
    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: viewer,
                kind: PrincipalKind::Human,
                display_name: "Viewer".to_owned(),
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
                id: "hidden-task".to_owned(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;

    let viewer_rows = service
        .search_structured_authorized_for_principals(&space_id, &[viewer], &criteria)
        .await?;
    let viewer_ids = viewer_rows
        .iter()
        .map(|row| row["_ugoite_id"].as_str().expect("Entry id").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(viewer_ids, vec!["visible-task"]);

    let unknown = StructuredSearch {
        form: "NotARealForm".to_owned(),
        ..criteria
    };
    let authorized_unknown = service
        .search_structured_authorized_for_principals(&space_id, &[viewer], &unknown)
        .await?;
    assert!(authorized_unknown.is_empty());
    let direct_error = service
        .search_structured(&space_id, &unknown)
        .await
        .expect_err("direct Search retains the explicit Form-not-found error");
    let direct_error = direct_error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("direct unknown Form error is typed");
    assert_eq!(
        direct_error.code(),
        ugoite_core::error::ErrorCode::FormNotFound
    );
    Ok(())
}

#[tokio::test]
async fn listing_inventory_is_authoritative_for_one_operation() -> Result<()> {
    use std::collections::BTreeMap;
    let service = UgoiteService::new("memory://listing-inventory-authority")?;
    service.create_space("demo").await?;
    let validated = service.list_space_ids().await?;
    assert_eq!(validated, vec!["demo"]);

    // The inventory method must not re-run discovery: an explicitly empty
    // inventory stays empty even though durable state contains a Space.
    let empty = service
        .list_spaces_authorized_for_inventory(Vec::new(), &BTreeMap::new())
        .await?;
    assert!(empty.is_empty());

    // A validated ID without a principal scope fails closed instead of
    // falling back to another Space or a repeated scan.
    let error = service
        .list_spaces_authorized_for_inventory(validated, &BTreeMap::new())
        .await
        .expect_err("missing principal scope must fail");
    let typed = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("listing inventory failure is typed");
    assert_eq!(
        typed.code(),
        ugoite_core::error::ErrorCode::SpaceDiscoveryFailed
    );
    let message = typed.to_string();
    assert!(!message.contains("memory://"), "{message}");
    assert!(!message.contains("request"), "{message}");
    Ok(())
}

#[tokio::test]
async fn authorized_sql_rejects_non_read_only_input_before_space_lookup() -> Result<()> {
    let service = UgoiteService::new("memory://authorized-sql-admission-order")?;
    let error = service
        .execute_sql_query_authorized(
            "missing-space",
            Uuid::from_u128(403),
            "INSERT INTO hidden_table VALUES (1)",
        )
        .await
        .expect_err("authorized SQL must reject writes before Space lookup");
    let error = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("read-only SQL failure is typed");
    assert_eq!(
        error.code(),
        ugoite_core::error::ErrorCode::ReadOnlySqlRequired
    );
    Ok(())
}

#[tokio::test]
async fn authorized_structured_search_rejects_invalid_input_before_space_lookup() -> Result<()> {
    let service = UgoiteService::new("memory://authorized-structured-search-admission-order")?;
    let criteria = StructuredSearch {
        form: "Task".to_owned(),
        updated_from: None,
        updated_to: None,
        conditions: Vec::new(),
        limit: Some(0),
        offset: None,
    };
    let error = service
        .search_structured_authorized_for_principals(
            "missing-space",
            &[Uuid::from_u128(404)],
            &criteria,
        )
        .await
        .expect_err("invalid structured Search must fail before Space lookup");
    let error = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("structured Search admission failure is typed");
    assert_eq!(error.code(), ugoite_core::error::ErrorCode::InvalidInput);
    Ok(())
}
