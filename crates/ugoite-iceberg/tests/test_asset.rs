mod common;
use chrono::Utc;
use common::setup_operator;
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode, ErrorKind};
use ugoite_core::query::EntryScope;
use ugoite_domain::identity::{
    AccessPolicy, Action, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_iceberg::asset;
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::entry;
use ugoite_iceberg::form;
use ugoite_iceberg::index;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::space;
use uuid::Uuid;

#[tokio::test]
async fn upload_returns_a_typed_reference_without_creating_an_entry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-space", "/tmp").await?;
    let ws_path = "spaces/asset-space";
    let reference = asset::save_asset(&op, ws_path, "image.png", b"bytes").await?;

    assert_eq!(reference.name, "image.png");
    assert_eq!(reference.size_bytes, 5);
    assert_eq!(reference.media_type, "application/octet-stream");
    assert!(!reference.asset_id.is_empty());
    assert_eq!(reference.sha256.len(), 64);
    assert!(
        op.exists(&format!("{ws_path}/assets/{}", reference.asset_id))
            .await?
    );
    assert!(ugoite_iceberg::entry::list_entries(&op, ws_path)
        .await?
        .is_empty());
    Ok(())
}

#[tokio::test]
async fn asset_size_limit_applies_to_core_write_and_read() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-size-limit", "/tmp").await?;
    let ws_path = "spaces/asset-size-limit";
    let oversized = vec![0_u8; asset::MAX_ASSET_BYTES + 1];
    let error = asset::save_asset(&op, ws_path, "large.bin", &oversized)
        .await
        .expect_err("core upload must enforce the shared Asset size limit");
    assert!(error.to_string().contains("size limit"));

    let asset_id = Uuid::now_v7().to_string();
    op.write(&format!("{ws_path}/assets/{asset_id}"), oversized)
        .await?;
    let error = asset::read_asset(&op, ws_path, &asset_id)
        .await
        .expect_err("raw oversized objects must not be materialized into memory");
    assert!(error.to_string().contains("size limit"));
    Ok(())
}

#[tokio::test]
async fn read_and_delete_use_the_exact_asset_id_key() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-exact-key", "/tmp").await?;
    let ws_path = "spaces/asset-exact-key";
    let reference = asset::save_asset(&op, ws_path, "file.txt", b"data").await?;

    let content = asset::read_asset(&op, ws_path, &reference.asset_id).await?;
    assert_eq!(content.bytes, b"data");
    assert_eq!(content.bytes.len(), reference.size_bytes as usize);

    asset::delete_asset(&op, ws_path, &reference.asset_id, &BTreeMap::new()).await?;
    assert!(
        op.exists(&format!("{ws_path}/assets/{}", reference.asset_id))
            .await?
    );
    let error = asset::read_asset(&op, ws_path, &reference.asset_id)
        .await
        .expect_err("logically deleted assets must not be readable");
    assert_eq!(
        error
            .downcast_ref::<ugoite_core::error::AppError>()
            .expect("asset reads should preserve the typed not-found error")
            .code(),
        ErrorCode::AssetNotFound
    );
    Ok(())
}

#[tokio::test]
async fn upload_normalizes_the_display_name_without_leaking_storage_details() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    space::create_space(&op, "asset-name", "/tmp").await?;
    let ws_path = "spaces/asset-name";
    let reference = asset::save_asset(
        &op,
        ws_path,
        "../../## uploaded_at\nspoofed.txt",
        b"payload",
    )
    .await?;

    assert_eq!(reference.name, "uploaded_at spoofed.txt");
    assert!(!reference.name.contains(['/', '\\', '\n', '\r']));
    let encoded = serde_json::to_string(&reference)?;
    assert!(!encoded.contains("assets/"));
    assert!(!encoded.contains("ugoite://"));
    Ok(())
}

#[tokio::test]
async fn typed_form_asset_references_round_trip_and_guard_deletion() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-form", "/tmp").await?;
    let ws_path = "spaces/asset-form";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Media",
            "fields": {
                "Attachment": {"type": "asset_reference"},
                "Attachments": {
                    "type": "list",
                    "items": {"type": "asset_reference"}
                }
            }
        }),
    )
    .await?;
    let reference = asset::save_asset(&op, ws_path, "image.png", b"bytes").await?;
    reference.validate()?;
    let reference_json = serde_json::to_string(&reference)?;
    let content = format!(
            "---\nform: Media\nAttachment: {reference_json}\nAttachments: [{reference_json}]\n---\n# Photo"
    );
    entry::create_entry(
        &op,
        ws_path,
        "media-1",
        &content,
        "author",
        &FakeIntegrityProvider,
    )
    .await?;

    let entries = entry::list_entries(&op, ws_path).await?;
    assert_eq!(
        entries[0]["properties"]["Attachment"]["asset_id"],
        reference.asset_id
    );
    assert_eq!(
        entries[0]["properties"]["Attachments"][0]["asset_id"],
        reference.asset_id
    );
    let scalar_matches = index::query_index(
        &op,
        ws_path,
        &serde_json::json!({
            "Attachment": {"asset_id": reference.asset_id}
        })
        .to_string(),
    )
    .await?;
    assert_eq!(scalar_matches.len(), 1);
    assert_eq!(scalar_matches[0]["id"], "media-1");
    let list_matches = index::query_index(
        &op,
        ws_path,
        &serde_json::json!({
            "Attachments": {"$contains": {"asset_id": reference.asset_id}}
        })
        .to_string(),
    )
    .await?;
    assert_eq!(list_matches.len(), 1);
    assert_eq!(list_matches[0]["id"], "media-1");
    let workspace = ugoite_iceberg::iceberg_store::native_workspace(&op, ws_path).await?;
    assert!(
        asset::current_asset_reference_exists_in_workspace(
            &workspace,
            &reference.asset_id,
            &BTreeMap::from([("media".to_string(), EntryScope::AllCurrent)]),
        )
        .await?
    );
    let hidden_entry_scope = EntryScope::Only(BTreeSet::from([uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        b"unreadable-entry",
    )
    .into()]));
    assert!(
        !asset::current_asset_reference_exists_in_workspace(
            &workspace,
            &reference.asset_id,
            &BTreeMap::from([("media".to_string(), hidden_entry_scope)]),
        )
        .await?
    );
    let error = asset::delete_asset(
        &op,
        ws_path,
        &reference.asset_id,
        &BTreeMap::from([("media".to_string(), EntryScope::AllCurrent)]),
    )
    .await
    .expect_err("current typed Form values must guard the asset");
    let app_error = error
        .downcast_ref::<AppError>()
        .expect("visible asset conflicts are typed application errors");
    assert_eq!(app_error.kind(), ErrorKind::Conflict);
    assert_eq!(app_error.code(), ErrorCode::AssetReferenced);
    Ok(())
}

#[tokio::test]
async fn asset_deletion_and_reference_creation_share_the_catalog_head_boundary(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-race", "/tmp").await?;
    let ws_path = "spaces/asset-race";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Media",
            "fields": {
                "Attachment": {"type": "asset_reference"},
                "Attachments": {
                    "type": "list",
                    "items": {"type": "asset_reference"}
                }
            }
        }),
    )
    .await?;
    let scopes = BTreeMap::from([("media".to_string(), EntryScope::AllCurrent)]);

    // Reference publication wins first: deletion must observe the current
    // reference and never remove its bytes.
    let first = asset::save_asset(&op, ws_path, "first.bin", b"first").await?;
    let first_json = serde_json::to_string(&first)?;
    entry::create_entry(
        &op,
        ws_path,
        "reference-first",
        &format!(
            "---\nform: Media\nAttachment: {first_json}\nAttachments: [{first_json}]\n---\n# First"
        ),
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    assert!(asset::delete_asset(&op, ws_path, &first.asset_id, &scopes)
        .await
        .is_err());
    // A reference-visible deletion failure must leave the authoritative bytes readable.
    assert_eq!(
        asset::read_asset(&op, ws_path, &first.asset_id)
            .await?
            .bytes,
        b"first"
    );

    // The deletion publication wins first: a later Entry publication must conflict at
    // the same Catalog Head boundary instead of creating a dangling reference.
    let second = asset::save_asset(&op, ws_path, "second.bin", b"second").await?;
    asset::delete_asset(&op, ws_path, &second.asset_id, &scopes).await?;
    let second_json = serde_json::to_string(&second)?;
    assert!(entry::create_entry(
        &op,
        ws_path,
        "reference-after-delete",
        &format!(
            "---\nform: Media\nAttachment: {second_json}\nAttachments: [{second_json}]\n---\n# Second"
        ),
        "author",
        &FakeIntegrityProvider,
    )
    .await
    .is_err());
    assert!(asset::read_asset(&op, ws_path, &second.asset_id)
        .await
        .is_err());

    // The same two operations are also exercised concurrently. Whichever
    // publication wins, the other must lose the optimistic Head comparison.
    let third = asset::save_asset(&op, ws_path, "third.bin", b"third").await?;
    let third_json = serde_json::to_string(&third)?;
    let third_content = format!(
        "---\nform: Media\nAttachment: {third_json}\nAttachments: [{third_json}]\n---\n# Third"
    );
    let delete = asset::delete_asset(&op, ws_path, &third.asset_id, &scopes);
    let create = entry::create_entry(
        &op,
        ws_path,
        "reference-concurrent",
        &third_content,
        "author",
        &FakeIntegrityProvider,
    );
    let (delete_result, create_result) = tokio::join!(delete, create);
    assert_ne!(delete_result.is_ok(), create_result.is_ok());
    if delete_result.is_ok() {
        assert!(asset::read_asset(&op, ws_path, &third.asset_id)
            .await
            .is_err());
    } else {
        assert_eq!(
            asset::read_asset(&op, ws_path, &third.asset_id)
                .await?
                .bytes,
            b"third"
        );
    }
    Ok(())
}

#[tokio::test]
async fn asset_reference_races_are_deterministic_at_the_validation_boundary() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    space::create_space(&op, "asset-race-gate", "/tmp").await?;
    let ws_path = "spaces/asset-race-gate";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Media",
            "fields": {"Attachment": {"type": "asset_reference"}}
        }),
    )
    .await?;
    let scopes = BTreeMap::from([("media".to_string(), EntryScope::AllCurrent)]);

    // The reference has validated against Head N. Deletion publishes Head
    // N+1 while the reference is paused; the stale reference CAS must lose.
    let first = asset::save_asset(&op, ws_path, "first.bin", b"first").await?;
    let first_json = serde_json::to_string(&first)?;
    let gate = ugoite_iceberg::TestValidationGate::new();
    ugoite_iceberg::install_test_validation_gate(gate.clone());
    let create_op = op.clone();
    let create_ws_path = ws_path.to_string();
    let create = tokio::spawn(async move {
        entry::create_entry(
            &create_op,
            &create_ws_path,
            "stale-reference",
            &format!("---\nform: Media\nAttachment: {first_json}\n---\n# First"),
            "author",
            &FakeIntegrityProvider,
        )
        .await
    });
    gate.wait_until_entered().await;
    ugoite_iceberg::clear_test_validation_gate();
    asset::delete_asset(&op, ws_path, &first.asset_id, &scopes).await?;
    gate.release();
    let create_result = create.await?;
    assert!(create_result.is_err());
    assert!(asset::read_asset(&op, ws_path, &first.asset_id)
        .await
        .is_err());

    // The deletion has validated against Head N. The reference publishes
    // Head N+1 before deletion creates its publication; deletion then loses its
    // stale CAS and the bytes remain.
    let second = asset::save_asset(&op, ws_path, "second.bin", b"second").await?;
    let second_json = serde_json::to_string(&second)?;
    let gate = ugoite_iceberg::TestValidationGate::new();
    ugoite_iceberg::install_test_validation_gate(gate.clone());
    let delete_op = op.clone();
    let delete_ws_path = ws_path.to_string();
    let delete_asset_id = second.asset_id.clone();
    let delete_scopes = scopes.clone();
    let delete = tokio::spawn(async move {
        asset::delete_asset(
            &delete_op,
            &delete_ws_path,
            &delete_asset_id,
            &delete_scopes,
        )
        .await
    });
    gate.wait_until_entered().await;
    ugoite_iceberg::clear_test_validation_gate();
    entry::create_entry(
        &op,
        ws_path,
        "reference-wins",
        &format!("---\nform: Media\nAttachment: {second_json}\n---\n# Second"),
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    gate.release();
    assert!(delete.await?.is_err());
    assert_eq!(
        asset::read_asset(&op, ws_path, &second.asset_id)
            .await?
            .bytes,
        b"second"
    );
    Ok(())
}

#[tokio::test]
async fn asset_deletions_keep_catalog_head_bounded() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-lifecycle-bounded", "/tmp").await?;
    let ws_path = "spaces/asset-lifecycle-bounded";
    let store = ugoite_storage::SpaceCatalogStore::new(op.clone(), ws_path)?;
    let initial = store
        .read_exact_head()
        .await?
        .expect("space Head")
        .bytes
        .len();
    for index in 0..64 {
        let reference =
            asset::save_asset(&op, ws_path, &format!("asset-{index}.bin"), b"bytes").await?;
        asset::delete_asset(&op, ws_path, &reference.asset_id, &BTreeMap::new()).await?;
    }
    let final_size = store
        .read_exact_head()
        .await?
        .expect("space Head")
        .bytes
        .len();
    assert!(
        final_size <= initial + 512,
        "Catalog Head grew from {initial} to {final_size} bytes"
    );
    Ok(())
}

#[tokio::test]
async fn authorization_state_hides_scalar_and_list_asset_references() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://asset-auth-scope");
    let owner = Uuid::from_u128(101);
    let viewer = Uuid::from_u128(102);
    let space_id = service
        .create_space_for_principal("asset-auth-scope", owner, "Owner")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &serde_json::json!({
                "name": "Media",
                "fields": {
                    "Attachment": {"type": "asset_reference"},
                    "Attachments": {
                        "type": "list",
                        "items": {"type": "asset_reference"}
                    }
                }
            }),
        )
        .await?;
    let reference = service
        .save_asset(&space_id, "private.bin", b"private")
        .await?;
    let reference_json = serde_json::to_string(&reference)?;
    service
        .create_entry(
            &space_id,
            "hidden",
            &format!(
                "---\nform: Media\nAttachment: {reference_json}\nAttachments: [{reference_json}]\n---\n# Hidden"
            ),
            "owner",
        )
        .await?;

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
                kind: ResourceKind::Entry,
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
    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Asset,
                id: reference.asset_id.clone(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![ugoite_domain::identity::Grant {
                    principal_id: viewer,
                    actions: [Action::Delete].into_iter().collect(),
                }],
            },
        )
        .await?;

    let scopes = service
        .authorized_form_entry_scopes(&space_id, viewer)
        .await?;
    let workspace = ugoite_iceberg::iceberg_store::native_workspace(
        service.operator(),
        &format!("spaces/{space_id}"),
    )
    .await?;
    assert!(
        !asset::current_asset_reference_exists_in_workspace(
            &workspace,
            &reference.asset_id,
            &scopes,
        )
        .await?
    );
    let delete_error = service
        .delete_asset_with_principal(&space_id, &reference.asset_id, Some(viewer))
        .await
        .expect_err("hidden current references must still protect Asset bytes");
    let app_error = delete_error
        .downcast_ref::<AppError>()
        .expect("hidden asset conflicts are typed application errors");
    assert_eq!(app_error.kind(), ErrorKind::Forbidden);
    assert_eq!(app_error.code(), ErrorCode::Forbidden);
    assert_eq!(app_error.message(), "Asset deletion is not permitted");
    assert_eq!(
        asset::read_asset(
            service.operator(),
            &format!("spaces/{space_id}"),
            &reference.asset_id,
        )
        .await?
        .bytes,
        b"private"
    );
    Ok(())
}

#[tokio::test]
async fn asset_text_search_applies_asset_policy_with_entry_parent() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://asset-search-acl");
    let owner = Uuid::from_u128(201);
    let viewer = Uuid::from_u128(202);
    let space_id = service
        .create_space_for_principal("asset-search-acl", owner, "Owner")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &serde_json::json!({
                "name": "Media",
                "fields": {"Attachment": {"type": "asset_reference"}}
            }),
        )
        .await?;
    let reference = service
        .save_asset(&space_id, "private.txt", b"asset-secret")
        .await?;
    let reference_json = serde_json::to_string(&reference)?;
    service
        .create_entry(
            &space_id,
            "visible",
            &format!("---\nform: Media\nAttachment: {reference_json}\n---\nVisible"),
            "owner",
        )
        .await?;
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
                kind: ResourceKind::Asset,
                id: reference.asset_id.clone(),
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: Vec::new(),
            },
        )
        .await?;
    service.reindex(&space_id).await?;
    let denied_results = service
        .search_entries_authorized_for_principals(&space_id, &[viewer], "asset-secret", 10)
        .await?;
    assert!(denied_results.is_empty());

    authorizer
        .set_policy(
            &space_id,
            owner,
            &ResourceRef {
                kind: ResourceKind::Asset,
                id: reference.asset_id,
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![ugoite_domain::identity::Grant {
                    principal_id: viewer,
                    actions: [Action::Read].into_iter().collect(),
                }],
            },
        )
        .await?;
    assert_eq!(
        service
            .search_entries_authorized_for_principals(&space_id, &[viewer], "asset-secret", 10)
            .await?
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>(),
        vec!["visible"]
    );
    Ok(())
}

#[tokio::test]
async fn asset_text_search_finds_an_authorized_match_after_ten_thousand_entries(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://asset-search-over-10k");
    let owner = Uuid::from_u128(301);
    let viewer = Uuid::from_u128(302);
    let space_id = service
        .create_space_for_principal("asset-search-over-10k", owner, "Owner")
        .await?
        .to_string();
    let ws_path = format!("spaces/{space_id}");
    form::upsert_form(
        &op,
        &ws_path,
        &serde_json::json!({
            "name": "Media",
            "fields": {"Attachment": {"type": "asset_reference"}}
        }),
    )
    .await?;
    let reference = asset::save_asset(&op, &ws_path, "private.txt", b"asset-secret").await?;
    let reference_json = serde_json::to_string(&reference)?;

    let author = "owner";
    let mut batch = Vec::with_capacity(ugoite_iceberg::entry::MAX_ENTRY_CREATE_BATCH_SIZE);
    for index in 0..10_000 {
        batch.push(entry::EntryCreateRequest::new(
            format!("entry-{index:05}"),
            format!("---\nform: Media\n---\n# Entry {index:05}"),
        ));
        if batch.len() == ugoite_iceberg::entry::MAX_ENTRY_CREATE_BATCH_SIZE {
            entry::create_entries(
                &op,
                &ws_path,
                std::mem::take(&mut batch),
                author,
                &FakeIntegrityProvider,
            )
            .await?;
        }
    }
    batch.push(entry::EntryCreateRequest::new(
        "target-entry",
        format!("---\nform: Media\nAttachment: {reference_json}\n---\n# Only Asset Match"),
    ));
    entry::create_entries(&op, &ws_path, batch, author, &FakeIntegrityProvider).await?;

    let authorizer = Authorizer::new(op.clone());
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
                kind: ResourceKind::Asset,
                id: reference.asset_id,
                parent: None,
            },
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: false,
                grants: vec![ugoite_domain::identity::Grant {
                    principal_id: viewer,
                    actions: [Action::Read].into_iter().collect(),
                }],
            },
        )
        .await?;

    service.reindex(&space_id).await?;
    let results = service
        .search_entries_authorized_for_principals(&space_id, &[viewer], "asset-secret", 10)
        .await?;
    assert_eq!(
        results
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>(),
        vec!["target-entry"]
    );
    Ok(())
}

#[tokio::test]
async fn deleted_asset_blob_is_retained_after_logical_deletion() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://asset-retention");
    let owner = Uuid::from_u128(401);
    let space_id = service
        .create_space_for_principal("asset-retention", owner, "Owner")
        .await?
        .to_string();
    let reference =
        asset::save_asset(&op, &format!("spaces/{space_id}"), "orphan.txt", b"orphan").await?;
    service.delete_asset(&space_id, &reference.asset_id).await?;

    let path = format!("spaces/{space_id}/assets/{}", reference.asset_id);
    assert!(op.exists(&path).await?);
    assert!(
        !op.exists(&format!(
            "spaces/{space_id}/_ugoite/catalog/asset-lifecycle/{}",
            reference.asset_id
        ))
        .await?
    );
    assert!(
        asset::read_asset(&op, &format!("spaces/{space_id}"), &reference.asset_id)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn asset_text_parser_failure_keeps_authoritative_asset_and_entry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "asset-text-parser-failure", "/tmp").await?;
    let ws_path = "spaces/asset-text-parser-failure";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Media",
            "fields": {"Attachment": {"type": "asset_reference"}}
        }),
    )
    .await?;
    let bytes = b"%PDF-1.7\n(unterminated";
    let reference = asset::save_asset(&op, ws_path, "broken.pdf", bytes).await?;
    let reference_json = serde_json::to_string(&reference)?;
    entry::create_entry(
        &op,
        ws_path,
        "parser-failure-entry",
        &format!("---\nform: Media\nAttachment: {reference_json}\n---\n# Parser failure"),
        "owner",
        &FakeIntegrityProvider,
    )
    .await?;

    let _head = ugoite_iceberg::derived_relation::rebuild_asset_text(&op, ws_path).await?;
    assert_eq!(entry::list_entries(&op, ws_path).await?.len(), 1);
    assert_eq!(
        asset::read_asset(&op, ws_path, &reference.asset_id)
            .await?
            .bytes,
        bytes
    );
    let stats = ugoite_iceberg::derived_relation::asset_text_stats(&op, ws_path).await?;
    assert_eq!(stats["assets_failed"], 1);
    Ok(())
}
