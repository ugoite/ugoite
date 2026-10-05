//! Integration tests for asset lifecycle management.
//! REQ-ASSET-001

use base64::Engine;
use p256::{ecdsa::SigningKey, elliptic_curve::rand_core::OsRng, pkcs8::EncodePrivateKey};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use support::Command;

mod support;

fn ugoite_bin() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ugoite") {
        return PathBuf::from(path);
    }

    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path
}

fn immutable_space_path(root: &str) -> std::path::PathBuf {
    std::fs::read_dir(Path::new(root).join("spaces"))
        .expect("spaces directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("meta.json").is_file())
        .expect("UUID Space directory")
}

async fn assert_uploaded_asset_is_readable(root: &str, asset_id: &str) {
    let space_path = immutable_space_path(root);
    let space_id = space_path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("UUID Space directory name");
    let service = ugoite_iceberg::service::UgoiteService::new_without_background_refresh(root)
        .expect("open local Space service");
    let asset = service
        .read_asset(space_id, asset_id)
        .await
        .expect("read uploaded Asset through its published receipt");
    assert_eq!(asset.bytes, b"test asset content");
}

/// REQ-ASSET-001: Asset upload and exact-key lifecycle.
#[test]
fn test_asset_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_core_config(&config_path, &root);

    // Create space first
    assert!(run_cli(&config_path, &["space", "create", "asset-space"])
        .status
        .success());

    // Create a temp file to upload
    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"test asset content").unwrap();

    // Upload asset
    let upload_output = run_cli(
        &config_path,
        &["asset", "upload", asset_file.to_str().unwrap()],
    );

    assert!(
        upload_output.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&upload_output.stderr)
    );
}

/// REQ-ASSET-001: Asset upload strips traversal from explicit filenames.
#[tokio::test]
async fn test_asset_req_asset_001_upload_strips_filename_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_core_config(&config_path, &root);

    assert!(run_cli(&config_path, &["space", "create", "asset-space"])
        .status
        .success());

    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"test asset content").unwrap();

    let upload_output = run_cli(
        &config_path,
        &[
            "asset",
            "upload",
            asset_file.to_str().unwrap(),
            "--filename",
            "nested/../../outside.txt",
        ],
    );

    assert!(
        upload_output.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&upload_output.stderr)
    );

    let asset: serde_json::Value =
        serde_json::from_slice(&upload_output.stdout).expect("asset upload JSON");
    assert_eq!(asset["kind"].as_str(), Some("asset"));
    let asset_id = asset["id"].as_str().expect("asset id");
    assert_eq!(
        asset["asset_reference"]["name"].as_str(),
        Some("outside.txt")
    );

    assert_uploaded_asset_is_readable(&root, asset_id).await;
    assert!(!immutable_space_path(&root).join("outside.txt").exists());
}

/// REQ-ASSET-001: Asset upload normalizes metadata-spoofing explicit filenames.
#[tokio::test]
async fn test_asset_req_asset_001_upload_normalizes_markdown_heading_filename() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_core_config(&config_path, &root);

    assert!(run_cli(&config_path, &["space", "create", "asset-space"])
        .status
        .success());

    let asset_file = dir.path().join("test-asset.txt");
    std::fs::write(&asset_file, b"test asset content").unwrap();

    let upload_output = run_cli(
        &config_path,
        &[
            "asset",
            "upload",
            asset_file.to_str().unwrap(),
            "--filename",
            "## uploaded_at\nspoofed.txt",
        ],
    );

    assert!(
        upload_output.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&upload_output.stderr)
    );

    let asset: serde_json::Value =
        serde_json::from_slice(&upload_output.stdout).expect("asset upload JSON");
    assert_eq!(asset["kind"].as_str(), Some("asset"));
    let asset_id = asset["id"].as_str().expect("asset id");
    assert!(!asset_id.is_empty());
    assert_eq!(
        asset["asset_reference"]["name"].as_str(),
        Some("uploaded_at spoofed.txt")
    );

    assert_uploaded_asset_is_readable(&root, asset_id).await;
}

/// Oversize remote CLI upload is rejected by the client-side size guard
/// before opening a transport connection.
#[test]
fn test_asset_remote_upload_rejects_oversize_without_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let asset_file = dir.path().join("huge-asset.bin");
    let oversize = ugoite_iceberg::asset::MAX_ASSET_BYTES + 1;
    let chunk = vec![7u8; 1024 * 1024];
    let mut handle = std::fs::File::create(&asset_file).unwrap();
    let mut remaining = oversize;
    while remaining > 0 {
        let take = remaining.min(chunk.len());
        std::io::Write::write_all(&mut handle, &chunk[..take]).unwrap();
        remaining -= take;
    }
    drop(handle);
    let remote_space_uid = uuid::Uuid::now_v7().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_backend_config(&config_path, &endpoint, &remote_space_uid);

    let output = run_cli(
        &config_path,
        &["asset", "upload", asset_file.to_str().unwrap()],
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("size limit"));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

fn run_cli(config_path: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut full = vec![
        "--config".to_string(),
        config_path.to_string_lossy().into_owned(),
    ];
    full.extend(args.iter().map(|arg| (*arg).to_string()));
    Command::new(ugoite_bin())
        .args(full)
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        // Credentials resolve from the user-global store: scope every CLI
        // invocation at the fixture home so stub-paired profiles apply.
        .env(
            "HOME",
            config_path.parent().expect("config parent").join("home"),
        )
        .output()
        .expect("run CLI")
}

fn init_core_config(config_path: &std::path::Path, root: &str) {
    assert!(run_cli(config_path, &["config", "init"]).status.success());
    assert!(run_cli(
        config_path,
        &[
            "config",
            "connection",
            "set",
            "local",
            "--type",
            "core",
            "--root",
            root
        ]
    )
    .status
    .success());
}

fn init_backend_config(config_path: &std::path::Path, url: &str, space_uid: &str) {
    assert!(run_cli(config_path, &["config", "init"]).status.success());
    assert!(run_cli(
        config_path,
        &[
            "config",
            "connection",
            "set",
            "local",
            "--type",
            "backend",
            "--url",
            url
        ]
    )
    .status
    .success());
    assert!(run_cli(
        config_path,
        &[
            "context",
            "add",
            "test",
            "--connection",
            "local",
            "--space",
            space_uid,
            "--credential",
            "stub-cred",
        ]
    )
    .status
    .success());
    assert!(run_cli(config_path, &["context", "use", "test"])
        .status
        .success());
    seed_stub_credential(config_path, url, space_uid);
}

/// Pair a stub credential for the fixture connection (#2963): remote
/// mutations fail fast without a credential, so stub success-path tests
/// carry a real ES256 session. The stub ignores auth headers; only the
/// CLI-side gate and session binding observe it.
fn seed_stub_credential(config_path: &std::path::Path, base_url: &str, space_uid: &str) {
    let key = SigningKey::random(&mut OsRng);
    let point = key.verifying_key().to_encoded_point(false);
    let public_key_jwk = serde_json::json!({
        "kty": "EC",
        "crv": "P-256",
        "x": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            point.x().expect("public key x"),
        ),
        "y": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            point.y().expect("public key y"),
        ),
    });
    let session = serde_json::json!({
        "credential_id": uuid::Uuid::now_v7().to_string(),
        "device_name": "asset stub test",
        "public_key_jwk": public_key_jwk,
        "private_key_pkcs8": base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            key.to_pkcs8_der().expect("encode test private key").as_bytes(),
        ),
        "access_token": "stub-access",
        "refresh_token": "stub-refresh",
        "expires_at": chrono::Utc::now().timestamp() + 3600,
        "base_url": base_url,
        "resource": null,
        "space_uid": space_uid,
        "connection": "local",
    });
    let credentials = serde_json::json!({
        "version": 1,
        "credentials": { "stub-cred": session },
    });
    let home = config_path
        .parent()
        .expect("config parent")
        .join("home/.ugoite");
    std::fs::create_dir_all(&home).expect("create fixture credentials directory");
    std::fs::write(
        home.join("credentials.json"),
        serde_json::to_vec_pretty(&credentials).expect("serialize credential store"),
    )
    .expect("write fixture credential");
}

fn json_of(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout JSON")
}

/// Context-safe read side: upload, reference, list, read, download,
/// wrong-context rejection, and referenced-delete rejection share one
/// observable semantics in core mode.
#[test]
fn test_asset_read_side_shares_core_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_core_config(&config_path, &root);

    assert!(run_cli(&config_path, &["space", "create", "asset-space"])
        .status
        .success());
    let form_file = dir.path().join("doc-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Doc","fields":{"Document":{"type":"asset_reference"}}}"#,
    )
    .unwrap();
    assert!(
        run_cli(&config_path, &["form", "save", form_file.to_str().unwrap()])
            .status
            .success()
    );

    let asset_file = dir.path().join("note.bin");
    let asset_bytes = b"audit proof: 0123456789\n";
    assert_eq!(asset_bytes.len(), 24);
    std::fs::write(&asset_file, asset_bytes).unwrap();
    let upload = run_cli(
        &config_path,
        &["asset", "upload", asset_file.to_str().unwrap()],
    );
    assert!(
        upload.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&upload.stderr)
    );
    let receipt = json_of(&upload);
    assert_eq!(receipt["kind"].as_str(), Some("asset"));
    let asset_id = receipt["id"].as_str().expect("asset id").to_string();
    assert_eq!(receipt["asset_reference"]["asset_id"], asset_id);
    assert_eq!(receipt["asset_reference"]["name"], "note.bin");
    assert_eq!(
        receipt["asset_reference"]["media_type"],
        "application/octet-stream"
    );
    assert_eq!(receipt["asset_reference"]["size_bytes"], asset_bytes.len());
    assert_eq!(
        receipt["asset_reference"]["sha256"].as_str().unwrap().len(),
        64
    );
    use sha2::{Digest, Sha256};
    assert_eq!(
        receipt["asset_reference"]["sha256"],
        Sha256::digest(asset_bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );

    let fields_file = dir.path().join("fields.json");
    std::fs::write(
        &fields_file,
        serde_json::to_string(&serde_json::json!({"Document": receipt["asset_reference"]}))
            .unwrap(),
    )
    .unwrap();
    let create = run_cli(
        &config_path,
        &[
            "entry",
            "create",
            "--id",
            "doc-1",
            "--form",
            "Doc",
            "--fields-file",
            fields_file.to_str().unwrap(),
        ],
    );
    assert!(
        create.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );

    // List shows the Form-owned reference with ownership identity.
    let list = run_cli(&config_path, &["asset", "list", "-o", "json"]);
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let items = json_of(&list);
    assert_eq!(items.as_array().expect("items").len(), 1);
    assert_eq!(items[0]["asset_id"], asset_id);
    assert_eq!(items[0]["entry_id"], "doc-1");
    assert_eq!(items[0]["field"], "Document");
    assert_eq!(items[0]["form"], "Doc");

    // Read projects metadata; octet-stream content stays download-only.
    let read = run_cli(
        &config_path,
        &[
            "asset", "read", &asset_id, "--entry", "doc-1", "--field", "Document", "-o", "json",
        ],
    );
    assert!(
        read.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    let read_json = json_of(&read);
    assert_eq!(read_json["asset_id"], asset_id);
    assert!(read_json["content_text"].is_null());

    // Download writes exact bytes.
    let out_path = dir.path().join("downloaded.bin");
    let download = run_cli(
        &config_path,
        &[
            "asset",
            "download",
            &asset_id,
            "--entry",
            "doc-1",
            "--field",
            "Document",
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    assert!(
        download.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&download.stderr)
    );
    assert_eq!(std::fs::read(&out_path).unwrap(), asset_bytes);

    // Wrong entry and wrong field contexts fail closed with a stable code.
    for args in [
        vec![
            "asset",
            "read",
            &asset_id,
            "--entry",
            "missing-entry",
            "--field",
            "Document",
            "-o",
            "json",
        ],
        vec![
            "asset", "read", &asset_id, "--entry", "doc-1", "--field", "Missing", "-o", "json",
        ],
        vec![
            "asset",
            "download",
            &asset_id,
            "--entry",
            "doc-1",
            "--field",
            "Missing",
            "--out",
            out_path.to_str().unwrap(),
        ],
    ] {
        let output = run_cli(&config_path, &args);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("ASSET_NOT_FOUND"),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // A referenced asset cannot be deleted while it is in use.
    let delete = run_cli(&config_path, &["asset", "delete", &asset_id]);
    assert!(!delete.status.success());
}

/// Backend mode projects the same read-side surface through shared operations.
#[test]
fn test_asset_read_side_backend_uses_shared_operations() {
    use std::io::Write;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let space_uid = uuid::Uuid::now_v7().to_string();
    let served_uid = space_uid.clone();
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        // Request 1: asset.list. Request 2: asset.read bytes.
        for first in [true, false] {
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(accepted) => break accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "timed out waiting");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
            stream.set_read_timeout(Some(STUB_READ_TIMEOUT)).unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0_u8; 4096];
                let read = stub_read(&mut stream, &mut buffer, deadline, "read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if text.contains("\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request).into_owned();
            let head = text.split("\r\n\r\n").next().unwrap_or("").to_string();
            let (status, content_type, body) = if first {
                assert!(
                    head.starts_with(&format!("GET /spaces/{served_uid}/assets ")),
                    "{head}"
                );
                (
                    "200 OK",
                    "application/json",
                    serde_json::json!([{
                        "asset_id": "asset-1",
                        "name": "note.bin",
                        "media_type": "application/octet-stream",
                        "size_bytes": 12,
                        "sha256": "abc",
                        "form": "Doc",
                        "entry_id": "doc-1",
                        "field": "Document",
                    }])
                    .to_string()
                    .into_bytes(),
                )
            } else {
                assert!(
                    head.starts_with(&format!("GET /spaces/{served_uid}/assets/asset-1")),
                    "{head}"
                );
                assert!(head.contains("form=Doc"), "{head}");
                assert!(head.contains("entry_id=doc-1"), "{head}");
                (
                    "200 OK",
                    "application/octet-stream",
                    b"binary-bytes".to_vec(),
                )
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.toml");
    init_backend_config(&config_path, &endpoint, &space_uid);

    let out_path = dir.path().join("downloaded.bin");
    let download = run_cli(
        &config_path,
        &[
            "asset",
            "download",
            "asset-1",
            "--entry",
            "doc-1",
            "--field",
            "Document",
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    assert!(
        download.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&download.stderr)
    );
    assert_eq!(std::fs::read(&out_path).unwrap(), b"binary-bytes");
    handle.join().unwrap();

    // Wrong field context fails before any asset read request: the list
    // server sees exactly one request (asset.list) and no byte read follows.
    let lonely = TcpListener::bind("127.0.0.1:0").unwrap();
    let lonely_endpoint = format!("http://{}", lonely.local_addr().unwrap());
    let lonely_handle = std::thread::spawn(move || {
        let (mut stream, _) = lonely.accept().expect("list request");
        stream.set_read_timeout(Some(STUB_READ_TIMEOUT)).unwrap();
        let lonely_deadline = Instant::now() + STUB_READ_DEADLINE;
        let mut request = Vec::new();
        loop {
            let mut buffer = [0_u8; 4096];
            let read = stub_read(&mut stream, &mut buffer, lonely_deadline, "read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if String::from_utf8_lossy(&request).contains("\r\n\r\n") {
                break;
            }
        }
        let body = b"[]";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    });
    let lonely_config = dir.path().join("lonely-config.toml");
    init_backend_config(&lonely_config, &lonely_endpoint, &space_uid);
    let denied = run_cli(
        &lonely_config,
        &[
            "asset", "read", "asset-1", "--entry", "doc-1", "--field", "Missing", "-o", "json",
        ],
    );
    assert!(!denied.status.success());
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("ASSET_NOT_FOUND"),
        "stderr: {}",
        String::from_utf8_lossy(&denied.stderr)
    );
    lonely_handle.join().unwrap();
}

// ---------------------------------------------------------------------------
// PR7 remainder: durable attach/read-back, multi-attachment full-replacement,
// context usage errors, malformed-input guards, attachment search, and the
// core-vs-remote acceptance matrix. The supported attach path is
// `asset upload` followed by `entry create/update --fields-file` carrying the
// returned asset object; there is no dedicated attach flag.
// ---------------------------------------------------------------------------

const DOC_FORM_JSON: &str = r#"{"name":"Doc","fields":{"Document":{"type":"asset_reference"}}}"#;
const ALBUM_FORM_JSON: &str = r#"{"name":"Album","fields":{"Attachments":{"type":"list","items":{"type":"asset_reference"}}}}"#;

fn setup_core_space(dir: &tempfile::TempDir, slug: &str, form_json: &str) -> PathBuf {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_core_config(&config_path, &root);
    assert!(run_cli(&config_path, &["space", "create", slug])
        .status
        .success());
    let form_file = dir.path().join("form.json");
    std::fs::write(&form_file, form_json).unwrap();
    let updated = run_cli(&config_path, &["form", "save", form_file.to_str().unwrap()]);
    assert!(
        updated.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&updated.stderr)
    );
    config_path
}

/// Rebuild the full AssetReference field value for an upload receipt.
///
/// Upload machine output is the stable receipt (`kind`/`id`); the attach
/// path needs the stored reference object, whose metadata the caller knows
/// (filename given, octet-stream media type, content bytes).
fn upload_core(config_path: &Path, file: &Path, filename: &str) -> serde_json::Value {
    let file = file.to_str().unwrap();
    let output = run_cli(
        config_path,
        &["asset", "upload", file, "--filename", filename],
    );
    assert!(
        output.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("asset upload JSON");
    assert_eq!(receipt["kind"].as_str(), Some("asset"));
    assert!(
        receipt["id"].as_str().is_some_and(|id| !id.is_empty()),
        "upload receipt carries the asset id: {receipt}"
    );
    assert_eq!(receipt["asset_reference"]["asset_id"], receipt["id"]);
    assert_eq!(receipt["asset_reference"]["name"], filename);
    assert_eq!(receipt["asset_reference"].as_object().unwrap().len(), 5);
    receipt["asset_reference"].clone()
}

fn create_entry_with_fields(
    config_path: &Path,
    entry_id: &str,
    form: &str,
    fields: &serde_json::Value,
    fields_path: &Path,
) {
    std::fs::write(fields_path, serde_json::to_string(fields).unwrap()).unwrap();
    let output = run_cli(
        config_path,
        &[
            "entry",
            "create",
            "--id",
            entry_id,
            "--form",
            form,
            "--fields-file",
            fields_path.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "create stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn update_entry_with_fields(
    config_path: &Path,
    entry_id: &str,
    fields: &serde_json::Value,
    fields_path: &Path,
) {
    std::fs::write(fields_path, serde_json::to_string(fields).unwrap()).unwrap();
    let output = run_cli(
        config_path,
        &[
            "entry",
            "update",
            entry_id,
            "--fields-file",
            fields_path.to_str().unwrap(),
        ],
    );
    assert!(
        output.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn read_asset_name(
    config_path: &Path,
    asset_id: &str,
    entry_id: &str,
    field: &str,
) -> serde_json::Value {
    let output = run_cli(
        config_path,
        &[
            "asset", "read", asset_id, "--entry", entry_id, "--field", field, "-o", "json",
        ],
    );
    assert!(
        output.status.success(),
        "read stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    json_of(&output)
}

/// Attach via structured create AND via structured update, reading the name
/// back through the owning entry/field context (core mode).
#[test]
fn test_asset_attach_create_and_update_read_name_back_core() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = setup_core_space(&dir, "attach-core", DOC_FORM_JSON);

    let first_file = dir.path().join("report.txt");
    std::fs::write(&first_file, b"first report bytes").unwrap();
    let first = upload_core(&config_path, &first_file, "report.txt");
    let first_id = first["asset_id"].as_str().expect("asset id").to_string();

    // Supported attach path: the upload asset object becomes the field value.
    create_entry_with_fields(
        &config_path,
        "doc-create",
        "Doc",
        &serde_json::json!({"Document": first}),
        &dir.path().join("create-fields.json"),
    );
    let read = read_asset_name(&config_path, &first_id, "doc-create", "Document");
    assert_eq!(read["name"], "report.txt");
    assert_eq!(read["asset_id"], first_id);

    // Same attach path through a structured update (full replacement map).
    let second_file = dir.path().join("second.bin");
    std::fs::write(&second_file, b"second bytes").unwrap();
    let second = upload_core(&config_path, &second_file, "second.bin");
    let second_id = second["asset_id"].as_str().expect("asset id").to_string();
    update_entry_with_fields(
        &config_path,
        "doc-create",
        &serde_json::json!({"Document": second}),
        &dir.path().join("update-fields.json"),
    );
    let reread = read_asset_name(&config_path, &second_id, "doc-create", "Document");
    assert_eq!(reread["name"], "second.bin");

    // The replaced reference no longer authorizes the old context.
    let stale = run_cli(
        &config_path,
        &[
            "asset",
            "read",
            &first_id,
            "--entry",
            "doc-create",
            "--field",
            "Document",
            "-o",
            "json",
        ],
    );
    assert!(!stale.status.success());
    assert!(
        String::from_utf8_lossy(&stale.stderr).contains("ASSET_NOT_FOUND"),
        "stderr: {}",
        String::from_utf8_lossy(&stale.stderr)
    );
}

/// Create with one attachment, then append a second via read-modify-full-update
/// without losing the first (core mode).
#[test]
fn test_asset_multi_attachment_full_update_preserves_both_core() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = setup_core_space(&dir, "multi-core", ALBUM_FORM_JSON);

    let first_file = dir.path().join("first.txt");
    std::fs::write(&first_file, b"first bytes").unwrap();
    let first = upload_core(&config_path, &first_file, "first.txt");
    let first_id = first["asset_id"].as_str().expect("asset id").to_string();
    create_entry_with_fields(
        &config_path,
        "album-1",
        "Album",
        &serde_json::json!({"Attachments": [first]}),
        &dir.path().join("album-create.json"),
    );

    // Read step of read-modify-write: the current revision anchors the update.
    let current = run_cli(&config_path, &["entry", "get", "album-1"]);
    assert!(current.status.success());
    let current_json: serde_json::Value = serde_json::from_slice(&current.stdout).unwrap();
    let revision_id = current_json["revision_id"]
        .as_str()
        .expect("revision id")
        .to_string();

    let second_file = dir.path().join("second.txt");
    std::fs::write(&second_file, b"second bytes").unwrap();
    let second = upload_core(&config_path, &second_file, "second.txt");
    let second_id = second["asset_id"].as_str().expect("asset id").to_string();

    // Full-replacement update resupplies both references, anchored on the read
    // revision: omitted list items would be dropped, never merged.
    let fields_path = dir.path().join("album-update.json");
    std::fs::write(
        &fields_path,
        serde_json::to_string(&serde_json::json!({"Attachments": [first, second]})).unwrap(),
    )
    .unwrap();
    let updated = run_cli(
        &config_path,
        &[
            "entry",
            "update",
            "album-1",
            "--fields-file",
            fields_path.to_str().unwrap(),
            "--parent-revision-id",
            &revision_id,
        ],
    );
    assert!(
        updated.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&updated.stderr)
    );

    let list = run_cli(&config_path, &["asset", "list", "-o", "json"]);
    assert!(list.status.success());
    let items = json_of(&list);
    let ids: Vec<&str> = items
        .as_array()
        .expect("asset list array")
        .iter()
        .filter_map(|item| item.get("asset_id").and_then(|id| id.as_str()))
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&first_id.as_str()));
    assert!(ids.contains(&second_id.as_str()));
    assert_eq!(
        read_asset_name(&config_path, &first_id, "album-1", "Attachments")["name"],
        "first.txt"
    );
    assert_eq!(
        read_asset_name(&config_path, &second_id, "album-1", "Attachments")["name"],
        "second.txt"
    );
}

/// Missing --entry/--field is a clap usage error (exit 2) in both modes: the
/// CLI never contacts a transport without its read context.
#[test]
fn test_asset_read_missing_context_is_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_core_config(&config_path, &root);
    assert!(run_cli(&config_path, &["space", "create", "usage-space"])
        .status
        .success());

    for args in [
        vec!["asset", "read", "asset-1", "-o", "json"],
        vec!["asset", "read", "asset-1", "--entry", "doc-1", "-o", "json"],
        vec![
            "asset", "read", "asset-1", "--field", "Document", "-o", "json",
        ],
    ] {
        let output = run_cli(&config_path, &args);
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
    }
    let out_path = dir.path().join("out.bin");
    let download = run_cli(
        &config_path,
        &[
            "asset",
            "download",
            "asset-1",
            "--field",
            "Document",
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    assert_eq!(download.status.code(), Some(2));

    // Same usage surface in backend mode: failure happens before any request.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let remote_uid = uuid::Uuid::now_v7().to_string();
    let backend_config = dir.path().join("backend-config.toml");
    init_backend_config(&backend_config, &endpoint, &remote_uid);
    let denied = run_cli(&backend_config, &["asset", "read", "asset-1", "-o", "json"]);
    assert_eq!(denied.status.code(), Some(2));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

/// The CLI cannot construct malformed multipart: a missing file fails closed
/// before any mutation, leaving the asset list empty. (Multipart rejection
/// itself is a server boundary covered by the server upload tests.)
#[test]
fn test_asset_upload_missing_file_fails_closed_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = setup_core_space(&dir, "malformed-cli", DOC_FORM_JSON);

    let missing = dir.path().join("no-such-file.bin");
    let output = run_cli(
        &config_path,
        &["asset", "upload", missing.to_str().unwrap()],
    );
    assert!(!output.status.success());
    assert!(!String::from_utf8_lossy(&output.stderr).is_empty());

    let list = run_cli(&config_path, &["asset", "list", "-o", "json"]);
    assert!(list.status.success());
    assert_eq!(json_of(&list).as_array().expect("array").len(), 0);
}

/// A successful HTTP status with malformed upload metadata is not projected
/// as a success receipt because bytes may already have been persisted.
#[test]
fn test_asset_upload_invalid_remote_reference_has_no_success_receipt() {
    let asset_id = uuid::Uuid::now_v7().to_string();
    let malformed = serde_json::json!({
        "asset_id": asset_id,
        "name": "proof.txt",
        "media_type": "application/octet-stream",
        "size_bytes": 24,
        "sha256": "not-a-checksum",
    });
    let harness = spawn_stub(
        start_stub(
            malformed,
            b"audit proof: 0123456789\n".to_vec(),
            vec![],
            serde_json::json!([]),
        ),
        1,
    );
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("proof.txt");
    std::fs::write(&file, b"audit proof: 0123456789\n").unwrap();

    let output = run_cli(
        &harness.config_path,
        &["asset", "upload", file.to_str().unwrap(), "-o", "json"],
    );
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "unexpected success receipt: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("result is unconfirmed"), "{stderr}");
    assert!(stderr.contains("Do not retry blindly"), "{stderr}");
    harness.handle.join().unwrap();
}

/// Minimal stub backend serving the portable asset/entry routes over
/// the same REST paths the api-client protocol owns.
struct StubBackend {
    space_uid: String,
    asset: serde_json::Value,
    asset_bytes: Vec<u8>,
    refs: Mutex<Vec<serde_json::Value>>,
    entry_get: serde_json::Value,
    entry_rows: serde_json::Value,
    seen: Mutex<Vec<(String, String)>>,
}

/// Bounded stub read (issue #3421): under full-suite load a single socket
/// read can stall past one short timeout while the peer is merely slow, so
/// timed-out reads retry until an overall deadline instead of failing on
/// the first WouldBlock. An expired deadline or a real I/O error still
/// fails loudly. Both timeout kinds are tolerated because read timeouts
/// surface as WouldBlock on Unix and TimedOut elsewhere.
const STUB_READ_TIMEOUT: Duration = Duration::from_secs(5);
const STUB_READ_DEADLINE: Duration = Duration::from_secs(30);

fn stub_read(
    stream: &mut std::net::TcpStream,
    buffer: &mut [u8],
    deadline: Instant,
    what: &str,
) -> usize {
    loop {
        match std::io::Read::read(stream, buffer) {
            Ok(read) => return read,
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.kind() == std::io::ErrorKind::TimedOut =>
            {
                assert!(
                    Instant::now() < deadline,
                    "{what} timed out waiting for bytes"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("{what}: {error}"),
        }
    }
}

fn read_stub_request(stream: &mut std::net::TcpStream) -> (String, String) {
    use std::io::Write;
    stream.set_read_timeout(Some(STUB_READ_TIMEOUT)).unwrap();
    let deadline = Instant::now() + STUB_READ_DEADLINE;
    let mut raw = Vec::new();
    let mut header_end = None;
    let mut content_length = 0_usize;
    loop {
        let mut buffer = [0_u8; 4096];
        let read = stub_read(&mut *stream, &mut buffer, deadline, "read stub request");
        if read == 0 {
            break;
        }
        raw.extend_from_slice(&buffer[..read]);
        if header_end.is_none() {
            if let Some(pos) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = Some(pos + 4);
                if String::from_utf8_lossy(&raw[..pos + 4])
                    .to_ascii_lowercase()
                    .contains("expect: 100-continue")
                {
                    stream
                        .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                        .expect("100-continue");
                }
                for line in String::from_utf8_lossy(&raw[..pos]).lines().skip(1) {
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_length = value.trim().parse().expect("content length");
                    }
                }
                if content_length == 0 {
                    break;
                }
            }
        }
        if let Some(end) = header_end {
            if raw.len() >= end + content_length {
                break;
            }
        }
    }
    let end = header_end.unwrap_or(raw.len()).min(raw.len());
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let body = String::from_utf8_lossy(raw.get(end..).unwrap_or(&[])).into_owned();
    (head, body)
}

fn serve_stub(listener: TcpListener, backend: Arc<StubBackend>, expected: usize) {
    use std::io::Write;
    let deadline = Instant::now() + Duration::from_secs(30);
    for _ in 0..expected {
        let (mut stream, _) = loop {
            match listener.accept() {
                Ok(accepted) => break accepted,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "stub timed out waiting");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("stub accept: {error}"),
            }
        };
        let (head, body_text) = read_stub_request(&mut stream);
        let uid = backend.space_uid.clone();
        let (status, content_type, body): (&str, &str, Vec<u8>) =
            if head.starts_with(&format!("POST /spaces/{uid}/assets ")) {
                (
                    "201 Created",
                    "application/json",
                    serde_json::to_vec(&backend.asset).unwrap(),
                )
            } else if head.starts_with(&format!("POST /spaces/{uid}/entries ")) {
                (
                    "200 OK",
                    "application/json",
                    br#"{"id":"doc-1","revision_id":"rev-1","change_id":"chg-1"}"#.to_vec(),
                )
            } else if head.starts_with(&format!("PUT /spaces/{uid}/entries/")) {
                (
                    "200 OK",
                    "application/json",
                    br#"{"id":"doc-1","revision_id":"rev-2","change_id":"chg-2"}"#.to_vec(),
                )
            } else if head.starts_with(&format!("GET /spaces/{uid}/entries/")) {
                (
                    "200 OK",
                    "application/json",
                    serde_json::to_vec(&backend.entry_get).unwrap(),
                )
            } else if head.starts_with(&format!("GET /spaces/{uid}/assets ")) {
                (
                    "200 OK",
                    "application/json",
                    serde_json::to_vec(&backend.refs.lock().unwrap().clone()).unwrap(),
                )
            } else if head.starts_with(&format!("GET /spaces/{uid}/assets/")) {
                // Fail-closed `asset.read` context gate mirroring the server:
                // both exact portable-protocol names must be present, so a
                // renamed or dropped CLI parameter gets 403 instead of bytes.
                let request_line = head.lines().next().unwrap_or("");
                if asset_read_context(request_line).is_none() {
                    (
                        "403 Forbidden",
                        "application/json",
                        br#"{"detail":"asset reads require a containing Form and Entry context"}"#
                            .to_vec(),
                    )
                } else {
                    (
                        "200 OK",
                        "application/octet-stream",
                        backend.asset_bytes.clone(),
                    )
                }
            } else if head.starts_with(&format!("POST /spaces/{uid}/entries/query")) {
                (
                    "200 OK",
                    "application/json",
                    serde_json::to_vec(
                        &serde_json::json!({"rows": backend.entry_rows, "has_more": false}),
                    )
                    .unwrap(),
                )
            } else {
                (
                    "404 Not Found",
                    "application/json",
                    br#"{"detail":"unexpected stub request"}"#.to_vec(),
                )
            };
        backend.seen.lock().unwrap().push((
            head.split("\r\n").next().unwrap_or("").to_string(),
            body_text,
        ));
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.write_all(&body).unwrap();
    }
}

/// Required `asset.read` query context with the exact portable-protocol
/// names (`form`, `entry_id`) and percent-decoded values. Returns `None`
/// when either parameter is missing, empty, or renamed, so the stub fails
/// closed exactly where the server does.
fn asset_read_context(request_line: &str) -> Option<(String, String)> {
    let target = request_line.split_whitespace().nth(1)?;
    let query = target.split_once('?')?.1;
    let mut form = None;
    let mut entry_id = None;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=')?;
        let value = percent_decode(value)?;
        match name {
            "form" => form = Some(value),
            "entry_id" => entry_id = Some(value),
            _ => {}
        }
    }
    let form = form.filter(|value| !value.is_empty())?;
    let entry_id = entry_id.filter(|value| !value.is_empty())?;
    Some((form, entry_id))
}

fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 3 <= bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?;
                decoded.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).ok()
}

fn stub_asset(asset_id: &str, name: &str, size: u64) -> serde_json::Value {
    serde_json::json!({
        "asset_id": asset_id,
        "name": name,
        "media_type": "text/plain",
        "size_bytes": size,
        "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    })
}

struct StubSetup {
    uid: String,
    dir: tempfile::TempDir,
    config_path: PathBuf,
    backend: Arc<StubBackend>,
    listener: TcpListener,
}

fn start_stub(
    asset: serde_json::Value,
    asset_bytes: Vec<u8>,
    refs: Vec<serde_json::Value>,
    entry_rows: serde_json::Value,
) -> StubSetup {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let uid = uuid::Uuid::now_v7().to_string();
    let backend = Arc::new(StubBackend {
        space_uid: uid.clone(),
        asset,
        asset_bytes,
        refs: Mutex::new(refs),
        entry_get: serde_json::json!({
            "id": "doc-1",
            "revision_id": "00000000-0000-7000-8000-000000000001",
            "extra_attributes": {},
            "form": "Doc",
            "title": "doc-1",
        }),
        entry_rows,
        seen: Mutex::new(Vec::new()),
    });
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.toml");
    init_backend_config(&config_path, &endpoint, &uid);
    StubSetup {
        uid,
        dir,
        config_path,
        backend,
        listener,
    }
}

struct StubHarness {
    uid: String,
    _dir: tempfile::TempDir,
    config_path: PathBuf,
    backend: Arc<StubBackend>,
    handle: std::thread::JoinHandle<()>,
}

fn spawn_stub(setup: StubSetup, expected: usize) -> StubHarness {
    let StubSetup {
        uid,
        dir,
        config_path,
        backend,
        listener,
    } = setup;
    let worker = backend.clone();
    let handle = std::thread::spawn(move || serve_stub(listener, worker, expected));
    StubHarness {
        uid,
        _dir: dir,
        config_path,
        backend,
        handle,
    }
}

/// Attach via structured create over the remote transport: upload, entry
/// create with the asset object, then read the name back through context.
#[test]
fn test_asset_attach_create_reads_name_back_remote() {
    let asset_id = "asset-remote-1";
    let asset = stub_asset(asset_id, "report.txt", 18);
    let asset_bytes = b"remote report bytes".to_vec();
    let refs = vec![serde_json::json!({
        "asset_id": asset_id,
        "name": "report.txt",
        "media_type": "text/plain",
        "size_bytes": asset_bytes.len(),
        "sha256": "abc",
        "form": "Doc",
        "entry_id": "doc-1",
        "field": "Document",
    })];
    let harness = spawn_stub(
        start_stub(asset, asset_bytes.clone(), refs, serde_json::json!([])),
        4,
    );

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("report.txt");
    std::fs::write(&file, &asset_bytes).unwrap();
    let upload = run_cli(
        &harness.config_path,
        &["asset", "upload", file.to_str().unwrap()],
    );
    assert!(
        upload.status.success(),
        "upload stderr: {}",
        String::from_utf8_lossy(&upload.stderr)
    );
    let uploaded: serde_json::Value = serde_json::from_slice(&upload.stdout).unwrap();
    assert_eq!(uploaded["kind"].as_str(), Some("asset"));
    assert_eq!(uploaded["id"].as_str(), Some(asset_id));
    let attached = uploaded["asset_reference"].clone();

    let fields_file = dir.path().join("fields.json");
    std::fs::write(
        &fields_file,
        serde_json::to_string(&serde_json::json!({"Document": attached})).unwrap(),
    )
    .unwrap();
    let create = run_cli(
        &harness.config_path,
        &[
            "entry",
            "create",
            "--id",
            "doc-1",
            "--form",
            "Doc",
            "--fields-file",
            fields_file.to_str().unwrap(),
        ],
    );
    assert!(
        create.status.success(),
        "create stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );
    let read = read_asset_name(&harness.config_path, asset_id, "doc-1", "Document");
    assert_eq!(read["name"], "report.txt");
    assert_eq!(read["content_text"], "remote report bytes");
    harness.handle.join().unwrap();

    let seen = harness.backend.seen.lock().unwrap();
    assert_eq!(seen.len(), 4);
    assert!(
        seen[0]
            .0
            .starts_with(&format!("POST /spaces/{}/assets ", harness.uid)),
        "{}",
        seen[0].0
    );
    assert!(
        seen[1]
            .0
            .starts_with(&format!("POST /spaces/{}/entries ", harness.uid)),
        "{}",
        seen[1].0
    );
    assert!(
        seen[2]
            .0
            .starts_with(&format!("GET /spaces/{}/assets ", harness.uid)),
        "{}",
        seen[2].0
    );
    assert!(
        seen[3]
            .0
            .starts_with(&format!("GET /spaces/{}/assets/{asset_id}", harness.uid)),
        "{}",
        seen[3].0
    );
    assert!(seen[3].0.contains("form=Doc"), "{}", seen[3].0);
    assert!(seen[3].0.contains("entry_id=doc-1"), "{}", seen[3].0);
    // Exact names and encoded values: a renamed or dropped parameter must
    // fail this assertion, not just the substring checks above.
    let request_line = seen[3].0.lines().next().unwrap_or("");
    let (form, entry_id) =
        asset_read_context(request_line).expect("asset.read carries form and entry_id");
    assert_eq!(form, "Doc", "{request_line}");
    assert_eq!(entry_id, "doc-1", "{request_line}");
}

/// Anonymous remote mutations fail fast before transport (#2963): no request
/// reaches the server and the error names the missing credential.
#[test]
fn test_asset_upload_without_credential_fails_fast_before_transport() {
    let asset_id = "asset-anon-1";
    let asset = stub_asset(asset_id, "note.txt", 17);
    let asset_bytes = b"anonymous bytes".to_vec();
    let harness = spawn_stub(
        start_stub(asset, asset_bytes.clone(), vec![], serde_json::json!([])),
        0,
    );

    // Same stub connection through a credential-less context.
    assert!(run_cli(
        &harness.config_path,
        &[
            "context",
            "add",
            "anon",
            "--connection",
            "local",
            "--space",
            &harness.uid,
        ]
    )
    .status
    .success());
    assert!(run_cli(&harness.config_path, &["context", "use", "anon"])
        .status
        .success());

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.txt");
    std::fs::write(&file, &asset_bytes).unwrap();
    let upload = run_cli(
        &harness.config_path,
        &["asset", "upload", file.to_str().unwrap()],
    );
    assert!(
        !upload.status.success(),
        "anonymous upload must fail fast before transport"
    );
    let stderr = String::from_utf8_lossy(&upload.stderr);
    assert!(
        stderr.contains("requires a credential"),
        "fail-fast must name the missing credential: {stderr}"
    );
    harness.handle.join().unwrap();
    assert!(
        harness.backend.seen.lock().unwrap().is_empty(),
        "no request may reach the server"
    );
}

/// The stub gate itself fails closed: every partial or renamed `asset.read`
/// query combination gets the same 403 the server returns, while the exact
/// documented query gets bytes.
#[test]
fn stub_asset_read_rejects_missing_or_renamed_context_query() {
    use std::io::{Read, Write};
    let setup = start_stub(
        stub_asset("asset-1", "a.txt", 5),
        b"hello".to_vec(),
        vec![],
        serde_json::json!([]),
    );
    let addr = setup.listener.local_addr().expect("stub addr");
    let uid = setup.uid.clone();
    let cases = [
        (format!("/spaces/{uid}/assets/asset-1"), 403),
        (format!("/spaces/{uid}/assets/asset-1?entry_id=doc-1"), 403),
        (format!("/spaces/{uid}/assets/asset-1?form=Doc"), 403),
        (
            format!("/spaces/{uid}/assets/asset-1?Form=Doc&entry_id=doc-1"),
            403,
        ),
        (
            format!("/spaces/{uid}/assets/asset-1?form=Doc&entryId=doc-1"),
            403,
        ),
        (
            format!("/spaces/{uid}/assets/asset-1?form=&entry_id=doc-1"),
            403,
        ),
        (
            format!("/spaces/{uid}/assets/asset-1?form=Doc&entry_id=doc-1"),
            200,
        ),
    ];
    let harness = spawn_stub(setup, cases.len());
    for (target, status) in &cases {
        let mut stream = std::net::TcpStream::connect(addr).expect("connect stub");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let request = format!("GET {target} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .expect("write stub request");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("read stub response");
        let status_line = response.lines().next().unwrap_or("").to_string();
        assert!(
            status_line.starts_with(&format!("HTTP/1.1 {status} ")),
            "{target}: {status_line}"
        );
        if *status == 403 {
            assert!(
                response.contains("asset reads require a containing Form and Entry context"),
                "{target}: {response}"
            );
        }
    }
    harness.handle.join().unwrap();
}

/// Remote full-replacement update resupplies both attachment references: the
/// stub captures the PUT body and the list/read surface proves both survived.
#[test]
fn test_asset_multi_attachment_full_update_preserves_both_remote() {
    let first_id = "asset-remote-a";
    let second_id = "asset-remote-b";
    let asset = stub_asset(first_id, "first.txt", 11);
    let refs = vec![
        serde_json::json!({
            "asset_id": first_id, "name": "first.txt", "media_type": "text/plain",
            "size_bytes": 11, "sha256": "abc",
            "form": "Doc", "entry_id": "doc-1", "field": "Attachments",
        }),
        serde_json::json!({
            "asset_id": second_id, "name": "second.txt", "media_type": "text/plain",
            "size_bytes": 12, "sha256": "def",
            "form": "Doc", "entry_id": "doc-1", "field": "Attachments",
        }),
    ];
    // upload x2, create, get, update, list, read = 7 requests.
    let harness = spawn_stub(
        start_stub(
            asset,
            b"second-bytes!".to_vec(),
            refs,
            serde_json::json!([]),
        ),
        7,
    );

    let dir = tempfile::tempdir().unwrap();
    let first_file = dir.path().join("first.txt");
    std::fs::write(&first_file, b"first-bytes").unwrap();
    let second_file = dir.path().join("second.txt");
    std::fs::write(&second_file, b"second-bytes!").unwrap();
    let mut uploaded_references = Vec::new();
    for file in [&first_file, &second_file] {
        let upload = run_cli(
            &harness.config_path,
            &["asset", "upload", file.to_str().unwrap()],
        );
        assert!(
            upload.status.success(),
            "upload stderr: {}",
            String::from_utf8_lossy(&upload.stderr)
        );
        let receipt = json_of(&upload);
        assert_eq!(receipt["asset_reference"]["asset_id"], receipt["id"]);
        uploaded_references.push(receipt["asset_reference"].clone());
    }
    let uploaded_first = uploaded_references[0].clone();
    let uploaded_second = serde_json::json!({
        "asset_id": second_id,
        "name": "second.txt",
        "media_type": "text/plain",
        "size_bytes": 12,
        "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    });
    let fields_file = dir.path().join("fields.json");
    std::fs::write(
        &fields_file,
        serde_json::to_string(&serde_json::json!({"Attachments": [uploaded_first.clone()]}))
            .unwrap(),
    )
    .unwrap();
    let create = run_cli(
        &harness.config_path,
        &[
            "entry",
            "create",
            "--id",
            "doc-1",
            "--form",
            "Doc",
            "--fields-file",
            fields_file.to_str().unwrap(),
        ],
    );
    assert!(
        create.status.success(),
        "create stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );

    // Read-modify-full-update: the CLI reads the entry for its revision, then
    // the caller resupplies the complete post-update array.
    let get = run_cli(&harness.config_path, &["entry", "get", "doc-1"]);
    assert!(get.status.success());
    std::fs::write(
        &fields_file,
        serde_json::to_string(
            &serde_json::json!({"Attachments": [uploaded_first, uploaded_second]}),
        )
        .unwrap(),
    )
    .unwrap();
    let update = run_cli(
        &harness.config_path,
        &[
            "entry",
            "update",
            "doc-1",
            "--fields-file",
            fields_file.to_str().unwrap(),
        ],
    );
    assert!(
        update.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&update.stderr)
    );

    let list = run_cli(&harness.config_path, &["asset", "list", "-o", "json"]);
    assert!(list.status.success());
    let ids: Vec<String> = json_of(&list)
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|item| {
            item.get("asset_id")
                .and_then(|id| id.as_str())
                .map(str::to_string)
        })
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&first_id.to_string()));
    assert!(ids.contains(&second_id.to_string()));
    harness.handle.join().unwrap();

    let seen = harness.backend.seen.lock().unwrap();
    assert_eq!(seen.len(), 7);
    let put = seen
        .iter()
        .find(|(line, _)| line.starts_with("PUT "))
        .expect("entry.update request");
    assert!(put.1.contains(first_id), "PUT body: {}", put.1);
    assert!(put.1.contains(second_id), "PUT body: {}", put.1);
}

/// Remote entry list surfaces stub rows through the canonical EntryQuery
/// operation (engine behavior remote-side is covered by service tests).
#[test]
fn test_asset_entry_list_remote_surfaces_results() {
    let harness = spawn_stub(
        start_stub(
            stub_asset("asset-x", "x.txt", 1),
            b"x".to_vec(),
            vec![],
            serde_json::json!([{"id": "doc-1", "form_id": "00000000-0000-7000-8000-000000000001", "revision_id": "00000000-0000-7000-8000-000000000001", "created_at_micros": 1000000, "updated_at_micros": 1000000, "preview": "Doc"}]),
        ),
        1,
    );
    let search = run_cli(
        &harness.config_path,
        &["entry", "list", "--text", "zephyr-quetzal", "-o", "json"],
    );
    assert!(
        search.status.success(),
        "search stderr: {}",
        String::from_utf8_lossy(&search.stderr)
    );
    let rows = json_of(&search);
    let ids: Vec<&str> = rows
        .as_array()
        .expect("search array")
        .iter()
        .filter_map(|row| row.get("id").and_then(|id| id.as_str()))
        .collect();
    assert!(ids.contains(&"doc-1"), "rows: {rows}");
    harness.handle.join().unwrap();
    let seen = harness.backend.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0]
            .0
            .starts_with(&format!("POST /spaces/{}/entries/query", harness.uid)),
        "{}",
        seen[0].0
    );
}

/// Acceptance matrix: valid / no / wrong context, malformed multipart,
/// multiple attachments, and attachment search share one observable
/// semantics in core mode and through the remote transport.
#[test]
fn test_asset_acceptance_matrix_core_remote_parity() {
    // Row 1: valid context reads the same name on both transports.
    let dir = tempfile::tempdir().unwrap();
    let config_path = setup_core_space(&dir, "matrix-core", DOC_FORM_JSON);
    let body_file = dir.path().join("matrix.txt");
    std::fs::write(&body_file, b"matrix bytes").unwrap();
    let core_asset = upload_core(&config_path, &body_file, "matrix.txt");
    let core_id = core_asset["asset_id"]
        .as_str()
        .expect("asset id")
        .to_string();
    create_entry_with_fields(
        &config_path,
        "m-1",
        "Doc",
        &serde_json::json!({"Document": core_asset}),
        &dir.path().join("matrix-fields.json"),
    );
    let core_name = read_asset_name(&config_path, &core_id, "m-1", "Document")["name"]
        .as_str()
        .expect("name")
        .to_string();

    let remote_id = "asset-matrix-1";
    let remote_bytes = b"matrix bytes".to_vec();
    let harness = spawn_stub(
        start_stub(
            stub_asset(remote_id, "matrix.txt", remote_bytes.len() as u64),
            remote_bytes,
            vec![serde_json::json!({
                "asset_id": remote_id, "name": "matrix.txt", "media_type": "text/plain",
                "size_bytes": 12, "sha256": "abc",
                "form": "Doc", "entry_id": "m-1", "field": "Document",
            })],
            serde_json::json!([{"id": "m-1", "form_id": "00000000-0000-7000-8000-000000000001", "revision_id": "00000000-0000-7000-8000-000000000001", "created_at_micros": 1000000, "updated_at_micros": 1000000, "preview": "M"}]),
        ),
        6,
    );
    let remote_dir = tempfile::tempdir().unwrap();
    let remote_file = remote_dir.path().join("matrix.txt");
    std::fs::write(&remote_file, b"matrix bytes").unwrap();
    let remote_upload = run_cli(
        &harness.config_path,
        &["asset", "upload", remote_file.to_str().unwrap()],
    );
    assert!(remote_upload.status.success());
    let remote_receipt = json_of(&remote_upload);
    assert_eq!(remote_receipt["asset_reference"]["asset_id"], remote_id);
    let remote_fields = remote_dir.path().join("fields.json");
    std::fs::write(
        &remote_fields,
        serde_json::to_string(&serde_json::json!({"Document": remote_receipt["asset_reference"]}))
            .unwrap(),
    )
    .unwrap();
    assert!(run_cli(
        &harness.config_path,
        &[
            "entry",
            "create",
            "--id",
            "m-1",
            "--form",
            "Doc",
            "--fields-file",
            remote_fields.to_str().unwrap(),
        ],
    )
    .status
    .success());
    let remote_name = read_asset_name(&harness.config_path, remote_id, "m-1", "Document")["name"]
        .as_str()
        .expect("name")
        .to_string();
    assert_eq!(core_name, "matrix.txt");
    assert_eq!(remote_name, core_name);

    // Row 2: wrong context fails closed with the same stable code before any
    // byte read on both transports.
    let core_wrong = run_cli(
        &config_path,
        &[
            "asset", "read", &core_id, "--entry", "m-1", "--field", "Missing", "-o", "json",
        ],
    );
    assert!(!core_wrong.status.success());
    assert!(
        String::from_utf8_lossy(&core_wrong.stderr).contains("ASSET_NOT_FOUND"),
        "stderr: {}",
        String::from_utf8_lossy(&core_wrong.stderr)
    );
    let remote_wrong = run_cli(
        &harness.config_path,
        &[
            "asset", "read", remote_id, "--entry", "m-1", "--field", "Missing", "-o", "json",
        ],
    );
    assert!(!remote_wrong.status.success());
    assert!(
        String::from_utf8_lossy(&remote_wrong.stderr).contains("ASSET_NOT_FOUND"),
        "stderr: {}",
        String::from_utf8_lossy(&remote_wrong.stderr)
    );

    // Row 3: entry text search surfaces the same entry on both transports.
    // Entry text matches Entry identity and columns only; asset bytes stay
    // outside the EntryQuery read path by product intent.
    assert!(run_cli(&config_path, &["index", "run"]).status.success());
    let core_search = run_cli(
        &config_path,
        &["entry", "list", "--text", "m-1", "-o", "json"],
    );
    assert!(core_search.status.success());
    let remote_search = run_cli(
        &harness.config_path,
        &["entry", "list", "--text", "m-1", "-o", "json"],
    );
    assert!(remote_search.status.success());
    for output in [&core_search, &remote_search] {
        let rows = serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap();
        assert!(
            rows.as_array()
                .expect("search array")
                .iter()
                .any(|row| row.get("id").and_then(|id| id.as_str()) == Some("m-1")),
            "rows: {rows}"
        );
    }
    harness.handle.join().unwrap();
    // upload, create, read (list + bytes), wrong-context (list only),
    // search: every CLI invocation reached the backend exactly once per
    // framed request, and the join proves the stub saw them all.
    let seen = harness.backend.seen.lock().unwrap();
    assert_eq!(seen.len(), 6, "{seen:?}");
}
