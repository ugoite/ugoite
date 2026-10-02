mod common;

use common::{seed_preexisting_form, setup_operator};
use serde_json::{json, Value};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::composition::parse_composition_yaml;
use ugoite_domain::metadata;
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
    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;

    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
                tags: Some(vec!["dashboard".to_string(), "finance".to_string()]),
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
    assert_eq!(raw.fields["name"], json!(saved.document.name));
    assert_eq!(raw.fields["kind"], json!("dashboard"));
    assert_eq!(raw.fields["format_version"], json!(1));
    assert_eq!(raw.fields["spec"], json!(saved.canonical_yaml));
    assert_eq!(raw.revision.entry.tags, ["dashboard", "finance"]);
    assert_eq!(raw.revision.revision_id, saved.revision_id);
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
                tags: None,
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
    let service = UgoiteService::new(format!(
        "memory://composition-save-update-{}",
        Uuid::now_v7()
    ))?;
    let owner = Uuid::from_u128(3_428_011);
    let space_id = service
        .create_space_for_principal("composition-save-update", owner, "Owner")
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
                tags: Some(vec!["original".to_string()]),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let mut changed = document;
    changed.name = "Monthly expenses updated".to_string();
    let updated = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: changed.clone(),
                tags: None,
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
    assert_eq!(current.revision.entry.tags, ["original"]);

    let stale = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(created.entry_id),
                base_revision_id: Some(created.revision_id),
                document: created.document,
                tags: None,
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
        service.save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: Some(updated.entry_id),
                base_revision_id: Some(updated.revision_id),
                document: left_document,
                tags: None,
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
                tags: None,
            },
            &actor,
            &principals,
        ),
    );
    assert_ne!(left.is_ok(), right.is_ok());
    let conflict = left.err().or_else(|| right.err()).unwrap();
    assert_eq!(
        conflict.downcast_ref::<AppError>().unwrap().code(),
        ErrorCode::RevisionConflict
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
