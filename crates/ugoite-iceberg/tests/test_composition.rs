mod common;

use chrono::Utc;
use common::{seed_preexisting_form, setup_operator};
use serde_json::{json, Value};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::composition::parse_composition_yaml;
use ugoite_domain::id::{EntryId, RevisionId};
use ugoite_domain::identity::{PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole};
use ugoite_domain::metadata;
use ugoite_iceberg::authorization::Authorizer;
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::{composition, form, iceberg_store, space};
use uuid::Uuid;

const MONTHLY_EXPENSE: &str =
    include_str!("../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml");

fn registry_form_definition(extension_metadata: Value, fields: Value) -> Value {
    json!({
        "id": uuid::Uuid::now_v7().to_string(),
        "name": composition::COMPOSITION_REGISTRY_FORM_NAME,
        "version": 1,
        "fields": fields,
        "allow_extra_attributes": "deny",
        "extension_metadata": extension_metadata,
    })
}

fn canonical_registry_fields() -> Value {
    json!({
        "name": {"id": 100, "type": "string", "required": true},
        "kind": {"id": 101, "type": "string", "required": true},
        "format_version": {"id": 102, "type": "integer", "required": true},
        "spec": {"id": 103, "type": "string", "required": true},
    })
}

fn assert_registry_conflict(error: &anyhow::Error) {
    assert!(error.to_string().contains("composition_registry_conflict"));
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<AppError>()
            .is_some_and(|app_error| app_error.code() == ErrorCode::CompositionRegistryConflict)
    }));
}

#[tokio::test]
async fn raw_composition_read_does_not_create_the_registry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://composition-raw-empty");
    let owner = Uuid::from_u128(3_428_000);
    let space_id = service
        .create_space_for_principal("composition-raw-empty", owner, "Owner")
        .await?
        .to_string();
    let ws_path = service.workspace_path(&space_id);

    let error = service
        .get_composition_raw_authorized_for_principals(&space_id, "missing-composition", &[owner])
        .await
        .expect_err("missing Composition should be not found");
    assert_eq!(
        error.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::EntryNotFound
    );
    assert!(iceberg_store::native_workspace_read_only(&op, &ws_path)
        .await?
        .list_forms()
        .await?
        .iter()
        .all(|form| !form
            .name
            .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)));
    Ok(())
}

#[tokio::test]
async fn composition_save_persists_canonical_carrier_and_receipt() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op, "memory://composition-save-create");
    let owner = Uuid::from_u128(3_428_010);
    let space_id = service
        .create_space_for_principal("composition-save-create", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.tags = vec!["dashboard".to_string(), "finance".to_string()];

    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    assert_eq!(saved.receipt.committed_revision_ids, [saved.revision_id]);
    assert_eq!(saved.receipt.command_id.len(), 36);
    assert!(saved.canonical_yaml.ends_with('\n'));
    let raw = service
        .get_composition_raw_authorized_for_principals(
            &space_id,
            &saved.entry_id.to_string(),
            &[owner],
        )
        .await?;
    assert_eq!(raw.revision.change_id, saved.receipt.command_id);
    assert_eq!(raw.fields["name"], json!(saved.document.name));
    assert_eq!(raw.fields["kind"], json!("dashboard"));
    assert_eq!(raw.fields["format_version"], json!(1));
    assert_eq!(raw.fields["spec"], json!(saved.canonical_yaml));
    assert_eq!(saved.document.tags, ["dashboard", "finance"]);
    assert_eq!(raw.revision.entry.tags, saved.document.tags);
    assert_eq!(raw.revision.revision_id, saved.revision_id);
    Ok(())
}

#[tokio::test]
async fn composition_save_replays_the_original_create_and_update_after_response_loss(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op, "memory://composition-save-idempotency");
    let owner = Uuid::from_u128(3_428_011);
    let space_id = service
        .create_space_for_principal("composition-save-idempotency", owner, "Owner")
        .await?
        .to_string();
    let original = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let create_request = || composition::CompositionSaveRequest {
        entry_id: None,
        base_revision_id: None,
        document: original.clone(),
    };

    // The first response is intentionally discarded to model a committed
    // publication whose response did not reach the caller.
    let first_create = service
        .save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            create_request(),
            &owner.to_string(),
            &[owner],
            "create-response-lost-1",
        )
        .await?;
    let replayed_create = service
        .save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            create_request(),
            &owner.to_string(),
            &[owner],
            "create-response-lost-1",
        )
        .await?;
    assert_eq!(replayed_create, first_create);

    let mut updated_document = first_create.document.clone();
    updated_document.name = "Response loss update".to_string();
    let update_request = || composition::CompositionSaveRequest {
        entry_id: Some(first_create.entry_id),
        base_revision_id: Some(first_create.revision_id),
        document: updated_document.clone(),
    };
    let first_update = service
        .save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            update_request(),
            &owner.to_string(),
            &[owner],
            "update-response-lost-1",
        )
        .await?;
    let replayed_update = service
        .save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            update_request(),
            &owner.to_string(),
            &[owner],
            "update-response-lost-1",
        )
        .await?;
    assert_eq!(replayed_update, first_update);

    let mut changed_document = updated_document;
    changed_document.name = "Different payload".to_string();
    let changed_payload = service
        .save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(first_create.entry_id),
                base_revision_id: Some(first_create.revision_id),
                document: changed_document,
            },
            &owner.to_string(),
            &[owner],
            "update-response-lost-1",
        )
        .await
        .expect_err("an operation identity cannot be reused for different content");
    assert_eq!(
        changed_payload.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::IdempotencyConflict
    );

    let history = service
        .composition_history_local_page(&space_id, &first_create.entry_id.to_string(), 10, 0)
        .await?;
    assert_eq!(history.total, 2, "replays must not append revisions");
    let current = service
        .get_composition_raw_local(&space_id, &first_create.entry_id.to_string())
        .await?;
    assert_eq!(current.revision.revision_id, first_update.revision_id);
    Ok(())
}

#[tokio::test]
async fn concurrent_composition_save_retries_share_one_publication() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op, "memory://composition-save-idempotency-race");
    let owner = Uuid::from_u128(3_428_012);
    let space_id = service
        .create_space_for_principal("composition-save-idempotency-race", owner, "Owner")
        .await?
        .to_string();
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let left_request = composition::CompositionSaveRequest {
        entry_id: None,
        base_revision_id: None,
        document: document.clone(),
    };
    let right_request = composition::CompositionSaveRequest {
        entry_id: None,
        base_revision_id: None,
        document,
    };
    let owner_text = owner.to_string();
    let owner_principals = [owner];
    let (left, right) = tokio::join!(
        service.save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            left_request,
            &owner_text,
            &owner_principals,
            "concurrent-save-1",
        ),
        service.save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            right_request,
            &owner_text,
            &owner_principals,
            "concurrent-save-1",
        ),
    );
    let left = left?;
    let right = right?;
    assert_eq!(left, right);
    let history = service
        .composition_history_local_page(&space_id, &left.entry_id.to_string(), 10, 0)
        .await?;
    assert_eq!(history.total, 1);
    Ok(())
}

#[tokio::test]
async fn composition_save_rechecks_authorization_when_replaying_a_committed_operation(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://composition-save-replay-auth");
    let owner = Uuid::from_u128(3_428_013);
    let editor = Uuid::from_u128(3_428_014);
    let space_id = service
        .create_space_for_principal("composition-save-replay-auth", owner, "Owner")
        .await?
        .to_string();
    let authorizer = Authorizer::new(op);
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
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let request = || composition::CompositionSaveRequest {
        entry_id: None,
        base_revision_id: None,
        document: document.clone(),
    };
    let saved = service
        .save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            request(),
            &editor.to_string(),
            &[editor],
            "revoked-save-replay-1",
        )
        .await?;
    authorizer
        .revoke_principal(&space_id, owner, editor)
        .await?;

    let replay = service
        .save_composition_authorized_for_principals_with_operation_id(
            &space_id,
            request(),
            &editor.to_string(),
            &[editor],
            "revoked-save-replay-1",
        )
        .await
        .expect_err("a replay must use current authorization");
    assert_eq!(
        replay.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::Forbidden
    );
    let history = service
        .composition_history_local_page(&space_id, &saved.entry_id.to_string(), 10, 0)
        .await?;
    assert_eq!(history.total, 1);
    Ok(())
}

#[tokio::test]
async fn denied_composition_create_and_update_have_no_storage_side_effects() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(
        op.clone(),
        format!("memory://composition-denied-save-{}", Uuid::now_v7()),
    );
    let owner = Uuid::from_u128(3_428_020);
    let viewer = Uuid::from_u128(3_428_021);
    let space_id = service
        .create_space_for_principal("composition-denied-save", owner, "Owner")
        .await?
        .to_string();
    Authorizer::new(op)
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
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;

    let denied_create = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document.clone(),
            },
            &viewer.to_string(),
            &[viewer],
        )
        .await
        .expect_err("viewer cannot create a Composition");
    assert_eq!(
        denied_create.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::Forbidden
    );
    assert!(iceberg_store::native_workspace_read_only(
        service.operator(),
        &service.workspace_path(&space_id)
    )
    .await?
    .list_forms()
    .await?
    .iter()
    .all(|form| !form
        .name
        .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)));

    let created = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document.clone(),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let mut changed = document;
    changed.name.push_str(" (denied update)");
    let denied_update = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: changed,
            },
            &viewer.to_string(),
            &[viewer],
        )
        .await
        .expect_err("viewer cannot update a Composition");
    assert_eq!(
        denied_update.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::Forbidden
    );

    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &created.entry_id.to_string(),
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 1);
    assert_eq!(
        history.revisions[0].revision.revision_id,
        created.revision_id
    );
    Ok(())
}

#[tokio::test]
async fn invalid_composition_save_does_not_create_registry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://composition-save-invalid");
    let owner = Uuid::from_u128(3_428_012);
    let space_id = service
        .create_space_for_principal("composition-save-invalid", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.format_version = 99;

    let error = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await
        .expect_err("unsupported format versions cannot be persisted");
    assert_eq!(
        error.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::InvalidInput
    );
    let forms = iceberg_store::native_workspace_read_only(&op, &service.workspace_path(&space_id))
        .await?
        .list_forms()
        .await?;
    assert!(forms.iter().all(|form| !form
        .name
        .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)));
    Ok(())
}

#[tokio::test]
async fn composition_update_requires_exact_base_and_reports_stale_revision() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let root_uri = format!("memory://composition-save-update-{}", Uuid::now_v7());
    let service = UgoiteService::from_operator(op.clone(), root_uri.clone());
    let other_service = UgoiteService::from_operator(op, root_uri);
    let owner = Uuid::from_u128(3_428_011);
    let space_id = service
        .create_space_for_principal("composition-save-update", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.tags = vec!["original".to_string()];
    let created = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document.clone(),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let mut changed = document;
    changed.name = "Monthly expenses updated".to_string();
    changed.tags = vec!["updated".to_string()];
    let updated = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: changed.clone(),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    assert_eq!(updated.entry_id, created.entry_id);
    assert_ne!(updated.revision_id, created.revision_id);
    assert_eq!(updated.document, changed);
    let current = service
        .get_composition_raw_authorized_for_principals(
            &space_id,
            &updated.entry_id.to_string(),
            &[owner],
        )
        .await?;
    assert_eq!(current.revision.entry.tags, changed.tags);

    let stale = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: created.document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await
        .expect_err("stale update must conflict");
    let stale = stale.downcast_ref::<AppError>().unwrap();
    assert_eq!(stale.code(), ErrorCode::RevisionConflict);
    let current_revision_id = updated.revision_id.to_string();
    assert_eq!(
        stale
            .detail()
            .and_then(|detail| detail["current_revision_id"].as_str()),
        Some(current_revision_id.as_str())
    );

    let mut left_document = updated.document.clone();
    left_document.name = "Concurrent left update".to_string();
    let mut right_document = updated.document.clone();
    right_document.name = "Concurrent right update".to_string();
    let actor = owner.to_string();
    let principals = [owner];
    let (left, right) = tokio::join!(
        other_service.save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(updated.entry_id),
                base_revision_id: Some(updated.revision_id),
                document: left_document,
            },
            &actor,
            &principals,
        ),
        service.save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(updated.entry_id),
                base_revision_id: Some(updated.revision_id),
                document: right_document,
            },
            &actor,
            &principals,
        ),
    );
    let (winner, conflict) = match (left, right) {
        (Ok(winner), Err(conflict)) | (Err(conflict), Ok(winner)) => (winner, conflict),
        (Ok(_), Ok(_)) => panic!("both stale-base writers unexpectedly committed"),
        (Err(left), Err(right)) => panic!("both stale-base writers failed: {left}; {right}"),
    };
    assert_eq!(
        conflict.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::RevisionConflict
    );
    let latest = service
        .get_composition_raw_authorized_for_principals(
            &space_id,
            &updated.entry_id.to_string(),
            &[owner],
        )
        .await?;
    assert_eq!(latest.revision.revision_id, winner.revision_id);
    assert_eq!(latest.revision.change_id, winner.receipt.command_id);
    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &updated.entry_id.to_string(),
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 3);
    Ok(())
}

#[tokio::test]
async fn composition_restore_appends_exact_historical_revision_with_receipt() -> anyhow::Result<()>
{
    let service = UgoiteService::new(format!(
        "memory://composition-restore-authorized-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-restore-authorized", owner, "Owner")
        .await?
        .to_string();
    let mut original = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    original.tags = vec!["original".to_string()];
    let created = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: original.clone(),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let mut changed = original.clone();
    changed.name = "Changed after original".to_string();
    changed.tags = vec!["changed".to_string()];
    let updated = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: changed,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let restored = service
        .restore_composition_authorized_for_principals(
            &space_id,
            &created.entry_id.to_string(),
            &created.revision_id.to_string(),
            &owner.to_string(),
            &[owner],
        )
        .await?;

    assert_eq!(restored.entry_id, created.entry_id);
    assert_ne!(restored.revision_id, created.revision_id);
    assert_eq!(restored.restored_from_revision_id, created.revision_id);
    assert_eq!(restored.document, original);
    assert_eq!(
        restored.receipt.committed_revision_ids,
        [restored.revision_id]
    );
    assert_eq!(restored.receipt.command_id.len(), 36);
    let current = service
        .get_composition_raw_authorized_for_principals(
            &space_id,
            &created.entry_id.to_string(),
            &[owner],
        )
        .await?;
    assert_eq!(current.revision.revision_id, restored.revision_id);
    assert_eq!(current.revision.change_id, restored.receipt.command_id);
    assert_eq!(
        current.revision.parent_revision_id,
        Some(updated.revision_id)
    );
    assert_eq!(
        current.revision.operation,
        ugoite_domain::entry::EntryOperation::Restore
    );
    assert_eq!(
        current.revision.entry.restored_from,
        Some(created.revision_id)
    );
    assert_eq!(
        current.revision.source_id.as_deref(),
        Some(created.revision_id.to_string().as_str())
    );
    assert_eq!(current.revision.entry.tags, restored.document.tags);
    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &created.entry_id.to_string(),
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 3);
    assert_eq!(
        history.revisions.last().unwrap().revision.revision_id,
        restored.revision_id
    );
    Ok(())
}

#[tokio::test]
async fn denied_composition_restore_and_missing_source_leave_history_unchanged(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(
        op.clone(),
        format!("memory://composition-restore-denied-{}", Uuid::now_v7()),
    );
    let owner = Uuid::now_v7();
    let viewer = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-restore-denied", owner, "Owner")
        .await?
        .to_string();
    Authorizer::new(op)
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
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let created = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document.clone(),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let updated = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let denied = service
        .restore_composition_authorized_for_principals(
            &space_id,
            &created.entry_id.to_string(),
            &created.revision_id.to_string(),
            &viewer.to_string(),
            &[viewer],
        )
        .await
        .expect_err("a viewer cannot restore a Composition");
    assert_eq!(
        denied.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::Forbidden
    );
    let missing = service
        .restore_composition_authorized_for_principals(
            &space_id,
            &created.entry_id.to_string(),
            &Uuid::now_v7().to_string(),
            &owner.to_string(),
            &[owner],
        )
        .await
        .expect_err("restore never falls back when an exact source is missing");
    assert_eq!(
        missing.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::EntryNotFound
    );
    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &created.entry_id.to_string(),
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 2);
    assert_eq!(
        history.revisions.last().unwrap().revision.revision_id,
        updated.revision_id
    );
    Ok(())
}

#[tokio::test]
async fn local_composition_restore_uses_an_append_only_revision() -> anyhow::Result<()> {
    let service = UgoiteService::new(format!(
        "memory://composition-restore-local-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::now_v7();
    let space_id = service
        .create_space_for_principal("composition-restore-local", owner, "Owner")
        .await?
        .to_string();
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let created = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document: document.clone(),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let mut changed = document;
    changed.name = "Local restore current state".to_string();
    let updated = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: changed,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let restored = service
        .restore_composition_local(
            &space_id,
            &created.entry_id.to_string(),
            &created.revision_id.to_string(),
            &owner.to_string(),
        )
        .await?;
    assert_eq!(restored.document, created.document);
    let latest = service
        .get_composition_raw_local(&space_id, &created.entry_id.to_string())
        .await?;
    assert_eq!(latest.revision.revision_id, restored.revision_id);
    assert_eq!(
        latest.revision.parent_revision_id,
        Some(updated.revision_id)
    );
    assert_eq!(
        latest.revision.entry.restored_from,
        Some(created.revision_id)
    );
    Ok(())
}

#[tokio::test]
async fn composition_registry_is_reserved_and_reopens_without_recreation() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "composition-registry", "/tmp").await?;
    let ws_path = "spaces/composition-registry";

    assert!(metadata::is_reserved_metadata_form(
        composition::COMPOSITION_REGISTRY_FORM_NAME
    ));
    let first = composition::ensure_composition_registry(&op, ws_path).await?;
    let reopened = composition::ensure_composition_registry(&op, ws_path).await?;

    assert_eq!(first.id, reopened.id);
    assert_eq!(
        reopened,
        composition::composition_registry_definition().map(|mut expected| {
            expected.id = reopened.id;
            expected
        })?
    );
    let same_name = iceberg_store::native_workspace(&op, ws_path)
        .await?
        .list_forms()
        .await?
        .into_iter()
        .filter(|value| {
            value
                .name
                .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)
        })
        .count();
    assert_eq!(same_name, 1);

    let public_upsert = form::upsert_form(
        &op,
        ws_path,
        &json!({
            "name": composition::COMPOSITION_REGISTRY_FORM_NAME,
            "fields": {"spec": {"type": "string"}},
        }),
    )
    .await;
    assert!(public_upsert.is_err());
    assert!(public_upsert.unwrap_err().to_string().contains("reserved"));

    let direct_storage_create = iceberg_store::ensure_form_tables(
        &op,
        ws_path,
        &serde_json::to_value(composition::composition_registry_definition()?)?,
    )
    .await
    .unwrap_err();
    assert_registry_conflict(&direct_storage_create);
    Ok(())
}

#[tokio::test]
async fn concurrent_registry_ensure_uses_one_stable_form() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "composition-concurrent", "/tmp").await?;
    let ws_path = "spaces/composition-concurrent";

    let (left, right) = tokio::join!(
        composition::ensure_composition_registry(&op, ws_path),
        composition::ensure_composition_registry(&op, ws_path),
    );
    let left = left?;
    let right = right?;
    assert_eq!(left.id, right.id);
    assert_eq!(
        iceberg_store::native_workspace(&op, ws_path)
            .await?
            .list_forms()
            .await?
            .into_iter()
            .filter(|form| {
                form.name
                    .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)
            })
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn same_name_existing_form_is_not_adopted_or_migrated() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "composition-existing-form", "/tmp").await?;
    let ws_path = "spaces/composition-existing-form";
    seed_preexisting_form(
        &op,
        ws_path,
        &registry_form_definition(
            json!({"other.extension": "legacy-user-form"}),
            json!({"old": {"id": 100, "type": "string", "required": true}}),
        ),
    )
    .await?;
    let before =
        iceberg_store::load_domain_form(&op, ws_path, composition::COMPOSITION_REGISTRY_FORM_NAME)
            .await?;

    let error = composition::ensure_composition_registry(&op, ws_path)
        .await
        .unwrap_err();
    assert_registry_conflict(&error);

    let after =
        iceberg_store::load_domain_form(&op, ws_path, composition::COMPOSITION_REGISTRY_FORM_NAME)
            .await?;
    assert_eq!(after, before);
    assert_eq!(after.fields[0].name, "old");
    assert_eq!(
        after.extension_metadata.get("other.extension"),
        Some(&json!("legacy-user-form"))
    );
    Ok(())
}

#[tokio::test]
async fn registry_marker_and_schema_mismatch_fail_closed() -> anyhow::Result<()> {
    for (space_id, metadata, fields) in [
        (
            "composition-wrong-marker",
            json!({"ugoite.registry": "other.v1"}),
            canonical_registry_fields(),
        ),
        (
            "composition-wrong-schema",
            json!({"ugoite.registry": "composition.v1"}),
            json!({"name": {"id": 100, "type": "string", "required": true}}),
        ),
    ] {
        let op = setup_operator()?;
        space::create_space(&op, space_id, "/tmp").await?;
        let ws_path = format!("spaces/{space_id}");
        seed_preexisting_form(&op, &ws_path, &registry_form_definition(metadata, fields)).await?;
        let before = iceberg_store::load_domain_form(
            &op,
            &ws_path,
            composition::COMPOSITION_REGISTRY_FORM_NAME,
        )
        .await?;

        let error = composition::ensure_composition_registry(&op, &ws_path)
            .await
            .unwrap_err();
        assert_registry_conflict(&error);

        let after = iceberg_store::load_domain_form(
            &op,
            &ws_path,
            composition::COMPOSITION_REGISTRY_FORM_NAME,
        )
        .await?;
        assert_eq!(after, before);
    }
    Ok(())
}

#[tokio::test]
async fn local_composition_save_creates_updates_and_conflicts_with_receipts() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://composition-local-save");
    let owner = Uuid::from_u128(3_428_090);
    let space_id = service
        .create_space_for_principal("composition-local-save", owner, "Owner")
        .await?
        .to_string();
    let meta_path = format!("spaces/{space_id}/meta.json");
    let initial_meta: Value = serde_json::from_slice(&op.read(&meta_path).await?.to_bytes())?;
    assert_eq!(initial_meta["space_version"], "0.1");

    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.tags = vec!["local".to_string(), "finance".to_string()];
    let created = service
        .save_composition_local(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            "local-cli",
        )
        .await?;

    assert_eq!(
        created.receipt.committed_revision_ids,
        [created.revision_id]
    );
    assert_eq!(created.receipt.command_id.len(), 36);
    assert!(created.canonical_yaml.ends_with('\n'));
    let created_raw = service
        .get_composition_raw_local(&space_id, &created.entry_id.to_string())
        .await?;
    assert_eq!(created_raw.revision.change_id, created.receipt.command_id);
    assert_eq!(created_raw.fields["name"], json!(created.document.name));
    assert_eq!(created_raw.fields["kind"], json!("dashboard"));
    assert_eq!(created_raw.fields["format_version"], json!(1));
    assert_eq!(created_raw.fields["spec"], json!(created.canonical_yaml));
    assert_eq!(created_raw.revision.entry.tags, ["local", "finance"]);

    let mut updated_document = created.document.clone();
    updated_document.name = "Local update".to_string();
    updated_document.tags = vec!["updated".to_string()];
    let updated = service
        .save_composition_local(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: updated_document.clone(),
            },
            "local-cli",
        )
        .await?;
    assert_eq!(updated.entry_id, created.entry_id);
    assert_ne!(updated.revision_id, created.revision_id);
    assert_eq!(updated.document, updated_document);
    assert_eq!(
        updated.receipt.committed_revision_ids,
        [updated.revision_id]
    );
    let updated_raw = service
        .get_composition_raw_local(&space_id, &updated.entry_id.to_string())
        .await?;
    assert_eq!(updated_raw.revision.change_id, updated.receipt.command_id);

    let stale = service
        .save_composition_local(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: created.document,
            },
            "local-cli",
        )
        .await
        .expect_err("stale local updates must conflict");
    let stale = stale.downcast_ref::<AppError>().unwrap();
    assert_eq!(stale.code(), ErrorCode::RevisionConflict);
    assert_eq!(
        stale
            .detail()
            .and_then(|detail| detail["current_revision_id"].as_str()),
        Some(updated.revision_id.to_string().as_str())
    );
    let history = service
        .composition_history_local_page(&space_id, &created.entry_id.to_string(), 10, 0)
        .await?;
    assert_eq!(
        history.total, 2,
        "the stale update must not append a revision"
    );
    let current_raw = service
        .get_composition_raw_local(&space_id, &created.entry_id.to_string())
        .await?;
    assert_eq!(current_raw.revision.revision_id, updated.revision_id);
    assert_eq!(current_raw.revision.entry.tags, ["updated"]);

    let final_meta: Value = serde_json::from_slice(&op.read(&meta_path).await?.to_bytes())?;
    assert_eq!(final_meta["space_version"], initial_meta["space_version"]);
    Ok(())
}

#[tokio::test]
async fn invalid_local_composition_save_does_not_create_registry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service =
        UgoiteService::from_operator(op.clone(), "memory://composition-local-save-invalid");
    let owner = Uuid::from_u128(3_428_091);
    let space_id = service
        .create_space_for_principal("composition-local-save-invalid", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.format_version = 99;

    let error = service
        .save_composition_local(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            "local-cli",
        )
        .await
        .expect_err("unsupported format versions cannot be persisted");
    assert_eq!(
        error.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::InvalidInput
    );
    let forms = iceberg_store::native_workspace_read_only(&op, &service.workspace_path(&space_id))
        .await?
        .list_forms()
        .await?;
    assert!(forms.iter().all(|form| !form
        .name
        .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)));
    Ok(())
}

#[tokio::test]
async fn missing_local_composition_update_does_not_create_registry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service =
        UgoiteService::from_operator(op.clone(), "memory://composition-local-save-missing-entry");
    let owner = Uuid::from_u128(3_428_092);
    let space_id = service
        .create_space_for_principal("composition-local-save-missing-entry", owner, "Owner")
        .await?
        .to_string();
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;

    let error = service
        .save_composition_local(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(EntryId::from(Uuid::from_u128(3_428_093))),
                base_revision_id: Some(RevisionId::from(Uuid::from_u128(3_428_094))),
                document,
            },
            "local-cli",
        )
        .await
        .expect_err("a missing update target must not create a Composition");
    assert_eq!(
        error.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::EntryNotFound
    );

    let forms = iceberg_store::native_workspace_read_only(&op, &service.workspace_path(&space_id))
        .await?
        .list_forms()
        .await?;
    assert!(forms.iter().all(|form| !form
        .name
        .eq_ignore_ascii_case(composition::COMPOSITION_REGISTRY_FORM_NAME)));
    Ok(())
}
