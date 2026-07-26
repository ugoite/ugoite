mod common;
use common::setup_operator;
#[cfg(unix)]
use opendal::services::Fs;
#[cfg(unix)]
use opendal::Operator;
use serde_json::Value;
#[cfg(unix)]
use tempfile::tempdir;
use ugoite_core::{form, space};

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
    let op = Operator::new(builder)?.finish();

    space::create_space(&op, "private-space", dir.path().to_string_lossy().as_ref()).await?;

    let spaces_root = dir.path().join("spaces");
    let space_dir = spaces_root.join("private-space");

    let mode = |path: &std::path::Path| -> anyhow::Result<u32> {
        Ok(std::fs::metadata(path)?.permissions().mode() & 0o777)
    };

    assert_eq!(mode(&spaces_root)?, 0o700);
    assert_eq!(mode(&space_dir)?, 0o700);
    for dir_name in [
        "security",
        "forms",
        "assets",
        "materialized_views",
        "sql_sessions",
    ] {
        assert_eq!(mode(&space_dir.join(dir_name))?, 0o700);
    }
    for file_name in ["meta.json", "settings.json"] {
        assert_eq!(mode(&space_dir.join(file_name))?, 0o600);
    }

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
async fn authentication_cutover_dry_run_and_migration_use_uuid_directory() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "legacy-space", "/tmp").await?;
    let settings_path = "spaces/legacy-space/settings.json";
    let mut settings: Value = serde_json::from_slice(&op.read(settings_path).await?.to_vec())?;
    settings["members"] = serde_json::json!({"old-user": {"role": "owner"}});
    settings["member_invitations"] = serde_json::json!({"raw-secret": {}});
    op.write(settings_path, serde_json::to_vec(&settings)?)
        .await?;

    let report = space::authentication_cutover_report(&op).await?;
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].member_count, 1);
    assert!(report[0].requires_migration);
    assert!(!serde_json::to_string(&report)?.contains("raw-secret"));

    let migrated = space::migrate_authentication_cutover(&op).await?;
    assert_eq!(migrated.len(), 1);
    assert!(!migrated[0].requires_migration);
    let id = migrated[0].target_space_id.to_string();
    assert!(op.exists(&format!("spaces/{id}/meta.json")).await?);
    assert!(!op.exists("spaces/legacy-space/meta.json").await?);
    let settings: Value = serde_json::from_slice(
        &op.read(&format!("spaces/{id}/settings.json"))
            .await?
            .to_vec(),
    )?;
    assert!(settings.get("members").is_none());
    assert!(settings.get("member_invitations").is_none());
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
