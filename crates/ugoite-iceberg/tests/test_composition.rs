mod common;

use common::{seed_preexisting_form, setup_operator};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::identity::{
    AccessPolicy, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use ugoite_domain::metadata;
use ugoite_iceberg::authorization::{Authorizer, ResourceKind, ResourceRef};
use ugoite_iceberg::entry;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::{composition, form, iceberg_store, space};
use uuid::Uuid;

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
    space::create_space(&op, "composition-raw-empty", "/tmp").await?;
    let ws_path = "spaces/composition-raw-empty";

    assert!(
        composition::read_composition_raw(&op, ws_path, "missing-composition")
            .await?
            .is_none()
    );
    assert!(iceberg_store::native_workspace_read_only(&op, ws_path)
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

fn composition_fields(spec: &str, version: i64, name: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("name".to_string(), json!(name)),
        ("kind".to_string(), json!("dashboard")),
        ("format_version".to_string(), json!(version)),
        ("spec".to_string(), json!(spec)),
    ])
}

#[tokio::test]
async fn raw_composition_read_preserves_unknown_version_and_exact_history() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "composition-raw", "/tmp").await?;
    let ws_path = "spaces/composition-raw";
    composition::ensure_composition_registry(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    assert!(
        composition::read_composition_history_page(&op, ws_path, "missing-composition", 10, 0,)
            .await?
            .is_none()
    );
    let first = entry::create_structured_entry_with_scopes_and_change_with_receipt(
        &op,
        ws_path,
        "composition-document",
        composition::COMPOSITION_REGISTRY_FORM_NAME.to_string(),
        vec!["tool".to_string()],
        composition_fields("not: [valid YAML", 99, "Raw tool"),
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;
    let first_revision_id = first.1.committed_revision_ids[0].to_string();

    let raw = composition::read_composition_raw(&op, ws_path, "composition-document")
        .await?
        .expect("current raw Composition");
    assert_eq!(raw.fields["format_version"], json!(99));
    assert_eq!(raw.format_version_probe(), Some(99));
    assert_eq!(raw.fields["spec"], json!("not: [valid YAML"));
    assert_eq!(raw.revision.entry.tags, ["tool"]);

    entry::update_structured_entry_authorized_with_change(
        &op,
        ws_path,
        "composition-document",
        Some(composition::COMPOSITION_REGISTRY_FORM_NAME.to_string()),
        Some(vec!["updated".to_string()]),
        composition_fields("name: updated", 1, "Updated tool"),
        BTreeMap::new(),
        Some(&first_revision_id),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let exact = composition::read_composition_raw_revision(
        &op,
        ws_path,
        "composition-document",
        &first_revision_id,
    )
    .await?
    .expect("requested historical Composition revision");
    assert_eq!(exact.fields["spec"], json!("not: [valid YAML"));
    assert!(composition::read_composition_raw_revision(
        &op,
        ws_path,
        "composition-document",
        &Uuid::from_u128(3_428_099).to_string(),
    )
    .await?
    .is_none());
    let latest = composition::read_composition_raw(&op, ws_path, "composition-document")
        .await?
        .expect("latest raw Composition");
    assert_eq!(latest.fields["spec"], json!("name: updated"));
    assert_ne!(latest.revision.revision_id, exact.revision.revision_id);

    let page =
        composition::read_composition_history_page(&op, ws_path, "composition-document", 1, 0)
            .await?
            .expect("Composition history");
    assert_eq!(page.total, 2);
    assert!(page.has_more);
    assert_eq!(
        page.revisions[0].revision.revision_id,
        exact.revision.revision_id
    );
    Ok(())
}

#[tokio::test]
async fn authorized_composition_raw_read_conceals_missing_and_denied_ids() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let service = UgoiteService::from_operator(op.clone(), "memory://composition-acl");
    let owner = Uuid::from_u128(3_428_001);
    let viewer = Uuid::from_u128(3_428_002);
    let space_id = service
        .create_space_for_principal("composition-acl", owner, "Owner")
        .await?
        .to_string();
    let ws_path = service.workspace_path(&space_id);
    composition::ensure_composition_registry(&op, &ws_path).await?;
    let (_, receipt) = entry::create_structured_entry_with_scopes_and_change_with_receipt(
        &op,
        &ws_path,
        "composition-denied",
        composition::COMPOSITION_REGISTRY_FORM_NAME.to_string(),
        Vec::new(),
        composition_fields("name: tool", 1, "Tool"),
        BTreeMap::new(),
        &owner.to_string(),
        &FakeIntegrityProvider,
        None,
        None,
    )
    .await?;
    let revision_id = receipt.committed_revision_ids[0].to_string();

    let authorizer = Authorizer::new(op);
    authorizer
        .add_human_member(
            &space_id,
            owner,
            SpacePrincipal {
                principal_id: viewer,
                kind: PrincipalKind::Human,
                display_name: "Viewer".to_string(),
                state: PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
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
                id: "composition-denied".to_string(),
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
        .get_composition_raw_authorized_for_principals(&space_id, "composition-denied", &[owner])
        .await?;
    assert_eq!(visible.fields["name"], json!("Tool"));
    let exact = service
        .get_composition_raw_revision_authorized_for_principals(
            &space_id,
            "composition-denied",
            &revision_id,
            &[owner],
        )
        .await?;
    assert_eq!(exact.revision.revision_id.to_string(), revision_id);
    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            "composition-denied",
            &[owner],
            10,
            0,
        )
        .await?;
    assert_eq!(history.total, 1);
    assert_eq!(
        history.revisions[0].revision.revision_id.to_string(),
        revision_id
    );

    let denied = service
        .get_composition_raw_authorized_for_principals(&space_id, "composition-denied", &[viewer])
        .await
        .expect_err("Entry-level deny must hide the Composition");
    let missing = service
        .get_composition_raw_authorized_for_principals(&space_id, "composition-missing", &[viewer])
        .await
        .expect_err("missing Composition must remain concealed");
    let denied = denied.downcast::<AppError>().expect("typed denied error");
    let missing = missing.downcast::<AppError>().expect("typed missing error");
    assert_eq!(denied.code(), ErrorCode::EntryNotFound);
    assert_eq!(denied.code(), missing.code());
    assert!(denied.message().starts_with("Entry not found:"));
    assert!(missing.message().starts_with("Entry not found:"));
    Ok(())
}
