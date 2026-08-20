//! Integration tests for asset lifecycle management.
//! REQ-ASSET-001

use std::path::Path;
use std::process::Command;

use base64::Engine;
use p256::ecdsa::SigningKey;
use p256::elliptic_curve::rand_core::OsRng;
use p256::pkcs8::EncodePrivateKey;
use ugoite_cli::config::AuthSession;

#[path = "support/mod.rs"]
mod support;

use support::spawn_recording_server;

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

fn write_test_auth_session(config_path: &Path, base_url: &str) {
    let signing_key = SigningKey::random(&mut OsRng);
    let point = signing_key.verifying_key().to_encoded_point(false);
    let encode = |value: &[u8]| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value);
    let session = AuthSession {
        credential_id: uuid::Uuid::now_v7(),
        device_name: "asset-upload-test".to_string(),
        public_key_jwk: serde_json::json!({
            "kty": "EC",
            "crv": "P-256",
            "x": encode(point.x().unwrap()),
            "y": encode(point.y().unwrap()),
        }),
        private_key_pkcs8: Some(encode(signing_key.to_pkcs8_der().unwrap().as_bytes())),
        access_token: "test-access-token".to_string(),
        refresh_token: "test-refresh-token".to_string(),
        expires_at: i64::MAX,
        base_url: base_url.to_string(),
        space_uid: uuid::Uuid::now_v7(),
    };
    let credentials_path = config_path.parent().unwrap().join("cli-credentials.json");
    std::fs::write(
        credentials_path,
        serde_json::to_vec_pretty(&session).unwrap(),
    )
    .unwrap();
}

/// REQ-ASSET-001: Remote CLI upload preserves the portable multipart wire
/// contract in both supported endpoint modes.
#[test]
fn test_asset_remote_upload_sends_file_part_and_returns_metadata() {
    for (mode, endpoint_flag, expected_path, filename) in [
        (
            "backend",
            "--backend-url",
            "/spaces/remote-space/assets",
            "test-asset.txt",
        ),
        (
            "api",
            "--api-url",
            "/api/spaces/remote-space/assets",
            "nested/../../outside.txt",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("cli-config.json");
        let asset_file = dir.path().join("test-asset.txt");
        std::fs::write(&asset_file, b"remote asset content").unwrap();
        let (base_url, request_rx, server_handle) = spawn_recording_server(
            "HTTP/1.1 201 Created",
            r#"{"asset_id":"asset-remote-1","name":"outside.txt","media_type":"application/octet-stream","size_bytes":20,"sha256":"remote-sha256"}"#,
        );
        let endpoint_url = if mode == "api" {
            format!("{base_url}/api")
        } else {
            base_url.clone()
        };

        let set_output = Command::new(ugoite_bin())
            .args([
                "config",
                "set",
                "--mode",
                mode,
                endpoint_flag,
                &endpoint_url,
            ])
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .output()
            .expect("configure remote endpoint");
        assert!(
            set_output.status.success(),
            "config stderr: {}",
            String::from_utf8_lossy(&set_output.stderr)
        );
        write_test_auth_session(&config_path, &endpoint_url);

        let mut upload_args = vec![
            "asset",
            "upload",
            "remote-space",
            asset_file.to_str().unwrap(),
        ];
        if mode == "api" {
            upload_args.extend(["--filename", filename]);
        }
        let upload_output = Command::new(ugoite_bin())
            .args(upload_args)
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .env("UGOITE_ENABLE_REMOTE_ASSET_UPLOAD", "1")
            .output()
            .expect("run remote asset upload");

        server_handle.join().unwrap();
        let request = request_rx.recv().unwrap();
        assert!(
            upload_output.status.success(),
            "{mode} upload stderr: {}",
            String::from_utf8_lossy(&upload_output.stderr)
        );
        assert!(
            request.starts_with(&format!("POST {expected_path} HTTP/1.1\r\n")),
            "{request}"
        );
        let request_lower = request.to_ascii_lowercase();
        assert!(
            request_lower.contains("content-type: multipart/form-data; boundary="),
            "{request}"
        );
        assert!(
            request.contains(&format!(
                r#"Content-Disposition: form-data; name="file"; filename="{filename}""#
            )),
            "{request}"
        );
        assert!(
            request_lower.contains("content-type: application/octet-stream"),
            "{request}"
        );
        assert!(request.contains("remote asset content"), "{request}");
        assert!(
            request_lower.contains("authorization: dpop test-access-token"),
            "{request}"
        );
        assert!(request_lower.contains("dpop: ey"), "{request}");

        let asset: serde_json::Value =
            serde_json::from_slice(&upload_output.stdout).expect("remote asset JSON");
        assert_eq!(asset["asset_id"], "asset-remote-1");
        assert_eq!(asset["name"], "outside.txt");
        assert_eq!(asset["media_type"], "application/octet-stream");
        assert_eq!(asset["size_bytes"], 20);
        assert_eq!(asset["sha256"], "remote-sha256");
    }
}

#[test]
fn test_asset_remote_upload_requires_explicit_transport_capability() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"remote asset content").unwrap();
    std::fs::write(
        &config_path,
        serde_json::json!({
            "mode": "backend",
            "backend_url": "http://localhost:8000",
            "api_url": "http://localhost:3000/api"
        })
        .to_string(),
    )
    .unwrap();

    let output = Command::new(ugoite_bin())
        .args([
            "asset",
            "upload",
            "remote-space",
            asset_file.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .env_remove("UGOITE_ENABLE_REMOTE_ASSET_UPLOAD")
        .output()
        .expect("run gated remote asset upload");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("UGOITE_ENABLE_REMOTE_ASSET_UPLOAD=1"),
        "{stderr}"
    );
}
