//! Integration tests for asset lifecycle management.
//! REQ-ASSET-001

use std::path::Path;
use std::process::Command;

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
}

fn immutable_space_path(root: &str) -> std::path::PathBuf {
    std::fs::read_dir(Path::new(root).join("spaces"))
        .expect("spaces directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("meta.json").is_file())
        .expect("UUID Space directory")
}

/// REQ-ASSET-001: Asset upload and exact-key lifecycle.
#[test]
fn test_asset_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    // Create space first
    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "asset-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Create a temp file to upload
    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"test asset content").unwrap();

    let space_path = format!("{root}/spaces/asset-space");

    // Upload asset
    let upload_output = Command::new(ugoite_bin())
        .args(["asset", "upload", &space_path, asset_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        upload_output.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&upload_output.stderr)
    );
}

/// REQ-ASSET-001: Asset upload strips traversal from explicit filenames.
#[test]
fn test_asset_req_asset_001_upload_strips_filename_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "asset-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"test asset content").unwrap();

    let space_path = format!("{root}/spaces/asset-space");
    let upload_output = Command::new(ugoite_bin())
        .args([
            "asset",
            "upload",
            &space_path,
            asset_file.to_str().unwrap(),
            "--filename",
            "nested/../../outside.txt",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        upload_output.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&upload_output.stderr)
    );

    let asset: serde_json::Value =
        serde_json::from_slice(&upload_output.stdout).expect("asset upload JSON");
    let asset_name = asset["name"].as_str().expect("asset name");
    let asset_id = asset["asset_id"].as_str().expect("asset id");

    assert_eq!(asset_name, "outside.txt");
    let stored_space = immutable_space_path(&root);
    assert!(stored_space.join("assets").join(asset_id).exists());
    assert!(!stored_space.join("outside.txt").exists());
}

/// REQ-ASSET-001: Asset upload normalizes metadata-spoofing explicit filenames.
#[test]
fn test_asset_req_asset_001_upload_normalizes_markdown_heading_filename() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "asset-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"test asset content").unwrap();

    let space_path = format!("{root}/spaces/asset-space");
    let upload_output = Command::new(ugoite_bin())
        .args([
            "asset",
            "upload",
            &space_path,
            asset_file.to_str().unwrap(),
            "--filename",
            "## uploaded_at\nspoofed.txt",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        upload_output.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&upload_output.stderr)
    );

    let asset: serde_json::Value =
        serde_json::from_slice(&upload_output.stdout).expect("asset upload JSON");
    let asset_name = asset["name"].as_str().expect("asset name");

    assert_eq!(asset_name, "uploaded_at spoofed.txt");
    assert!(!asset_name.contains('\n'));
    assert!(!asset_name.starts_with('#'));
    assert!(immutable_space_path(&root)
        .join("assets")
        .join(asset["asset_id"].as_str().expect("asset id"))
        .exists());
}

/// REQ-ASSET-001: Remote CLI upload remains intentionally unavailable while
/// the API client, REST, and frontend multipart surfaces remain portable.
#[test]
fn test_asset_remote_upload_is_explicitly_unavailable() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"remote asset content").unwrap();
    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://127.0.0.1:1",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("configure backend endpoint");
    assert!(set_output.status.success());
    let upload_output = Command::new(ugoite_bin())
        .args([
            "asset",
            "upload",
            "remote-space",
            asset_file.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("run remote asset upload");
    assert!(!upload_output.status.success());
    assert!(String::from_utf8_lossy(&upload_output.stderr)
        .contains("asset upload is not available in backend/api mode"));
}
