mod common;
use common::setup_operator;
#[cfg(unix)]
use opendal::services::Fs;
#[cfg(unix)]
use opendal::Operator;
use serde_json::Value;
#[cfg(unix)]
use tempfile::tempdir;
use ugoite_iceberg::{form, space};

#[tokio::test]
/// REQ-STO-002, REQ-STO-004
async fn test_space_req_sto_002_create_space_scaffolding() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-space";

    // Call create_space
    space::create_space(&op, ws_id, "/tmp/ugoite").await?;

    // Verify directory structure using exists()
    // OpenDAL's exists() returns bool.
    let ws_path = format!("spaces/{}", ws_id);
    assert!(op.exists(&format!("{}/", ws_path)).await?);

    // Check meta.json
    let meta_path = format!("{}/meta.json", ws_path);
    assert!(op.exists(&meta_path).await?);

    // Check other files/folders
    let settings_path = format!("{}/settings.json", ws_path);
    assert!(op.exists(&settings_path).await?);
    assert!(op.exists(&format!("{}/forms/", ws_path)).await?);
    assert!(op.exists(&format!("{}/assets/", ws_path)).await?);

    // Verify meta.json content
    let meta_bytes = op.read(&meta_path).await?.to_vec();
    let meta: Value = serde_json::from_slice(&meta_bytes)?;
    assert_eq!(meta["id"], ws_id);
    assert_eq!(meta["name"], ws_id);
    assert!(meta.get("created_at").is_some());
    assert!(meta.get("storage").is_some());

    let settings_bytes = op.read(&settings_path).await?.to_vec();
    let settings: Value = serde_json::from_slice(&settings_bytes)?;
    assert_eq!(settings["default_form"], "Entry");

    let forms = form::list_forms(&op, &ws_path).await?;
    let entry_form = forms
        .iter()
        .find(|value| value.get("name").and_then(|name| name.as_str()) == Some("Entry"))
        .expect("starter Entry form");
    assert_eq!(entry_form["allow_extra_attributes"], "allow_columns");

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
/// REQ-STO-003
async fn test_space_req_sto_003_local_space_permissions() -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir()?;
    let builder = Fs::default().root(dir.path().to_string_lossy().as_ref());
    let op = Operator::new(builder)?;

    space::create_space(&op, "private-space", dir.path().to_string_lossy().as_ref()).await?;
    space::validate_complete_bootstrap(&op, "private-space").await?;

    let spaces_root = dir.path().join("spaces");
    let space_dir = spaces_root.join("private-space");

    let mode = |path: &std::path::Path| -> anyhow::Result<u32> {
        Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
    };

    assert_eq!(mode(&spaces_root)?, 0o700);
    assert_eq!(mode(&space_dir)?, 0o700);
    for dir_name in ["security", "forms", "assets", "sql_sessions"] {
        assert_eq!(mode(&space_dir.join(dir_name))?, 0o700);
    }
    for file_name in ["meta.json", "settings.json"] {
        assert_eq!(mode(&space_dir.join(file_name))?, 0o600);
    }

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn missing_local_space_is_reported_as_space_not_found_before_lock_open() -> anyhow::Result<()>
{
    let dir = tempdir()?;
    let op = Operator::new(Fs::default().root(dir.path().to_string_lossy().as_ref()))?;

    let error = space::get_space_raw(&op, "missing-space")
        .await
        .expect_err("missing local Space should be a not-found result");
    assert!(error.to_string().contains("Space not found: missing-space"));
    Ok(())
}

#[tokio::test]
/// REQ-STO-005
async fn test_space_req_sto_005_create_space_idempotency() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-space";

    space::create_space(&op, ws_id, "/tmp").await?;

    // Should fail (result err) when creating again
    let result = space::create_space(&op, ws_id, "/tmp").await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
async fn legacy_space_metadata_schema_is_rejected() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "legacy-schema", "/tmp").await?;
    let meta_path = "spaces/legacy-schema/meta.json";
    let mut meta: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;
    meta["schema_version"] = Value::from(1);
    op.write(meta_path, serde_json::to_vec(&meta)?).await?;

    let error = space::get_space(&op, "legacy-schema").await.unwrap_err();
    assert!(error.to_string().contains("unsupported Space layout"));
    let workspace_error = form::list_forms(&op, "spaces/legacy-schema")
        .await
        .unwrap_err();
    assert!(workspace_error
        .to_string()
        .contains("unsupported Space layout"));
    Ok(())
}

#[tokio::test]
async fn incomplete_current_space_metadata_is_rejected() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "incomplete-metadata", "/tmp").await?;
    let meta_path = "spaces/incomplete-metadata/meta.json";
    let mut meta: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;
    meta.as_object_mut().unwrap().remove("storage");
    op.write(meta_path, serde_json::to_vec(&meta)?).await?;

    let error = space::get_space(&op, "incomplete-metadata")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unsupported Space layout"));
    let workspace_error = form::list_forms(&op, "spaces/incomplete-metadata")
        .await
        .unwrap_err();
    assert!(workspace_error
        .to_string()
        .contains("unsupported Space layout"));
    Ok(())
}

#[tokio::test]
async fn space_metadata_identity_must_match_directory_and_uuidv7_contract() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "identity-contract", "/tmp").await?;
    let meta_path = "spaces/identity-contract/meta.json";
    let original: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;

    let mut wrong_directory = original.clone();
    wrong_directory["space_id"] = Value::String("another-space".to_string());
    op.write(meta_path, serde_json::to_vec(&wrong_directory)?)
        .await?;
    assert!(space::get_space(&op, "identity-contract")
        .await
        .expect_err("directory/space_id mismatch must be rejected")
        .to_string()
        .contains("does not match its directory"));

    let mut wrong_uuid_version = original;
    wrong_uuid_version["space_uid"] = Value::String(uuid::Uuid::new_v4().to_string());
    op.write(meta_path, serde_json::to_vec(&wrong_uuid_version)?)
        .await?;
    assert!(space::get_space(&op, "identity-contract")
        .await
        .expect_err("non-UUIDv7 Space identity must be rejected")
        .to_string()
        .contains("must be a UUIDv7"));
    Ok(())
}

#[tokio::test]
async fn uuid_addressed_space_directory_must_match_metadata_uid() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let space_uid = uuid::Uuid::now_v7();
    space::create_space_with_identity(&op, space_uid, "identity-uid", "/tmp").await?;
    let meta_path = format!("spaces/{space_uid}/meta.json");
    let mut meta: Value = serde_json::from_slice(&op.read(&meta_path).await?.to_vec())?;
    meta["space_uid"] = Value::String(uuid::Uuid::now_v7().to_string());
    op.write(&meta_path, serde_json::to_vec(&meta)?).await?;

    let error = space::list_spaces(&op)
        .await
        .expect_err("UUID directory and metadata UID mismatch must be rejected");
    assert!(error
        .to_string()
        .contains("UUID directory does not match space_uid"));
    Ok(())
}

#[tokio::test]
async fn incomplete_bootstrap_settings_are_rejected() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "invalid-settings", "/tmp").await?;
    let settings_path = "spaces/invalid-settings/settings.json";

    op.write(settings_path, br#"{}"#.to_vec()).await?;
    let error = space::validate_complete_bootstrap(&op, "invalid-settings")
        .await
        .expect_err("settings without default_form must be rejected");
    assert!(error.to_string().contains("requires default_form"));
    assert!(space::get_space(&op, "invalid-settings").await.is_err());
    assert!(space::list_spaces(&op).await.is_err());

    op.write(settings_path, br#"[]"#.to_vec()).await?;
    let error = space::validate_complete_bootstrap(&op, "invalid-settings")
        .await
        .expect_err("non-object settings must be rejected");
    assert!(error.to_string().contains("requires default_form"));
    Ok(())
}

#[tokio::test]
async fn pending_space_patch_journal_recovers_both_authoritative_files() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "patch-recovery", "memory:///").await?;
    let meta_path = "spaces/patch-recovery/meta.json";
    let settings_path = "spaces/patch-recovery/settings.json";
    let old_meta: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;
    let old_settings: Value = serde_json::from_slice(&op.read(settings_path).await?.to_vec())?;
    let mut new_meta = old_meta.clone();
    new_meta["name"] = Value::String("recovered-name".to_string());
    let mut new_settings = old_settings.clone();
    new_settings["default_form"] = Value::String("Entry".to_string());
    op.write(
        "spaces/patch-recovery/.ugoite-space-patch.json",
        serde_json::to_vec(&serde_json::json!({
            "old_metadata": old_meta,
            "new_metadata": new_meta,
            "old_settings": old_settings,
            "new_settings": new_settings,
        }))?,
    )
    .await?;

    space::validate_complete_bootstrap(&op, "patch-recovery").await?;
    let recovered: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;
    assert_eq!(recovered["name"], "recovered-name");
    let journal: Value = serde_json::from_slice(
        &op.read("spaces/patch-recovery/.ugoite-space-patch.json")
            .await?
            .to_vec(),
    )?;
    assert_eq!(journal["status"], "complete");
    Ok(())
}

#[tokio::test]
async fn stale_space_patch_journal_is_discarded_after_a_valid_winner() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "stale-patch", "memory:///").await?;
    let meta_path = "spaces/stale-patch/meta.json";
    let settings_path = "spaces/stale-patch/settings.json";
    let old_meta: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;
    let old_settings: Value = serde_json::from_slice(&op.read(settings_path).await?.to_vec())?;
    let mut new_meta = old_meta.clone();
    new_meta["name"] = Value::String("journal-winner".to_string());
    op.write(
        "spaces/stale-patch/.ugoite-space-patch.json",
        serde_json::to_vec(&serde_json::json!({
            "old_metadata": old_meta,
            "new_metadata": new_meta,
            "old_settings": old_settings.clone(),
            "new_settings": old_settings,
        }))?,
    )
    .await?;
    let mut external_meta: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;
    external_meta["name"] = Value::String("external-winner".to_string());
    op.write(meta_path, serde_json::to_vec(&external_meta)?)
        .await?;

    space::validate_complete_bootstrap(&op, "stale-patch").await?;
    let observed: Value = serde_json::from_slice(&op.read(meta_path).await?.to_vec())?;
    assert_eq!(observed["name"], "external-winner");
    let journal: Value = serde_json::from_slice(
        &op.read("spaces/stale-patch/.ugoite-space-patch.json")
            .await?
            .to_vec(),
    )?;
    assert_eq!(journal["status"], "complete");
    Ok(())
}

#[tokio::test]
async fn completed_space_patch_journal_is_reused_with_version_fencing() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "patch-reuse", "memory:///").await?;

    space::patch_space(&op, "patch-reuse", &serde_json::json!({"name": "first"})).await?;
    space::patch_space(&op, "patch-reuse", &serde_json::json!({"name": "second"})).await?;

    let metadata: Value =
        serde_json::from_slice(&op.read("spaces/patch-reuse/meta.json").await?.to_vec())?;
    assert_eq!(metadata["name"], "second");
    let journal: Value = serde_json::from_slice(
        &op.read("spaces/patch-reuse/.ugoite-space-patch.json")
            .await?
            .to_vec(),
    )?;
    assert_eq!(journal["status"], "complete");
    Ok(())
}

#[tokio::test]
/// REQ-STO-004
async fn test_space_req_sto_004_list_spaces_from_directory() -> anyhow::Result<()> {
    let op = setup_operator()?;

    space::create_space(&op, "sp-a", "/tmp").await?;
    space::create_space(&op, "sp-b", "/tmp").await?;

    let mut listed = space::list_spaces(&op).await?;
    listed.sort();
    assert_eq!(listed, vec!["sp-a".to_string(), "sp-b".to_string()]);

    Ok(())
}

#[tokio::test]
/// REQ-STO-008
async fn test_space_req_sto_008_list_spaces_ignores_missing_meta() -> anyhow::Result<()> {
    let op = setup_operator()?;

    op.create_dir("spaces/no-meta/").await?;

    let listed = space::list_spaces(&op).await?;
    assert!(listed.is_empty());

    Ok(())
}

#[tokio::test]
/// REQ-STO-002
async fn test_space_req_sto_002_test_storage_connection_memory() -> anyhow::Result<()> {
    let result = space::test_storage_connection(&space::StorageConnectionTestConfig {
        uri: "memory://".to_string(),
        endpoint: None,
    })
    .await?;
    assert_eq!(result["status"], "ok");
    assert_eq!(result["mode"], "memory");
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
/// REQ-STO-002
async fn test_space_req_sto_002_test_storage_connection_local() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let local_uri = format!("file://{}", dir.path().display());
    let result = space::test_storage_connection(&space::StorageConnectionTestConfig {
        uri: local_uri,
        endpoint: None,
    })
    .await?;
    assert_eq!(result["status"], "ok");
    assert_eq!(result["mode"], "local");

    Ok(())
}

#[tokio::test]
/// REQ-STO-006
async fn test_space_req_sto_006_test_storage_connection_unknown_rejects_unsupported_scheme(
) -> anyhow::Result<()> {
    let result = space::test_storage_connection(&space::StorageConnectionTestConfig {
        uri: "ftp://somehost".to_string(),
        endpoint: None,
    })
    .await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
/// REQ-STO-006
async fn test_space_req_sto_006_test_storage_connection_rejects_blocked_endpoint(
) -> anyhow::Result<()> {
    let result = space::test_storage_connection(&space::StorageConnectionTestConfig {
        uri: "s3://bucket-name/prefix".to_string(),
        endpoint: Some("http://127.0.0.1:9000".to_string()),
    })
    .await;
    assert!(result.is_err());
    Ok(())
}

#[tokio::test]
/// REQ-STO-006
async fn test_space_req_sto_006_test_storage_connection_accepts_custom_s3_endpoint_validation(
) -> anyhow::Result<()> {
    let result = space::test_storage_connection(&space::StorageConnectionTestConfig {
        uri: "s3://bucket-name/prefix".to_string(),
        endpoint: Some("https://s3.example.test".to_string()),
    })
    .await;
    assert!(
        result.is_err(),
        "unreachable custom endpoint should not be a false success"
    );
    Ok(())
}
