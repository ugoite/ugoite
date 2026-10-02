mod common;

use common::setup_operator;
use serde_json::{json, Value};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::metadata;
use ugoite_iceberg::{composition, form, iceberg_store, space};

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
    Ok(())
}

#[tokio::test]
async fn same_name_existing_form_is_not_adopted_or_migrated() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "composition-existing-form", "/tmp").await?;
    let ws_path = "spaces/composition-existing-form";
    iceberg_store::ensure_form_tables(
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
        iceberg_store::ensure_form_tables(
            &op,
            &ws_path,
            &registry_form_definition(metadata, fields),
        )
        .await?;
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
