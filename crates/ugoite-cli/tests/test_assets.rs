//! Integration tests for asset lifecycle management.
//! REQ-ASSET-001

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

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

/// Oversize remote CLI upload is rejected by the client-side size guard
/// before opening a transport connection.
#[test]
fn test_asset_remote_upload_rejects_oversize_without_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
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
    std::fs::write(
        &config_path,
        serde_json::json!({
            "mode": "backend",
            "backend_url": endpoint,
            "api_url": "http://127.0.0.1:3000/api"
        })
        .to_string(),
    )
    .unwrap();

    let remote_space_uid = uuid::Uuid::now_v7().to_string();
    let output = Command::new(ugoite_bin())
        .args([
            "asset",
            "upload",
            &remote_space_uid,
            asset_file.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("run remote asset upload");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("size limit"));
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

fn run_cli(config_path: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(ugoite_bin())
        .args(args)
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("run CLI")
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
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/asset-space");

    assert!(run_cli(
        &config_path,
        &["create-space", "--root", &root, "asset-space"]
    )
    .status
    .success());
    let form_file = dir.path().join("doc-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Doc","fields":{"Document":{"type":"asset_reference"}}}"#,
    )
    .unwrap();
    assert!(run_cli(
        &config_path,
        &["form", "update", &space_path, form_file.to_str().unwrap()]
    )
    .status
    .success());

    let asset_file = dir.path().join("note.bin");
    std::fs::write(&asset_file, b"binary-bytes").unwrap();
    let upload = run_cli(
        &config_path,
        &["asset", "upload", &space_path, asset_file.to_str().unwrap()],
    );
    assert!(
        upload.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&upload.stderr)
    );
    let asset = json_of(&upload);
    let asset_id = asset["asset_id"].as_str().expect("asset id").to_string();

    let fields_file = dir.path().join("fields.json");
    std::fs::write(
        &fields_file,
        serde_json::to_string(&serde_json::json!({"Document": asset})).unwrap(),
    )
    .unwrap();
    let create = run_cli(
        &config_path,
        &[
            "entry",
            "create",
            &space_path,
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
    let list = run_cli(&config_path, &["asset", "list", &space_path, "-o", "json"]);
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
            "asset",
            "read",
            &space_path,
            &asset_id,
            "--entry",
            "doc-1",
            "--field",
            "Document",
            "-o",
            "json",
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
            &space_path,
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
    assert_eq!(std::fs::read(&out_path).unwrap(), b"binary-bytes");

    // Wrong entry and wrong field contexts fail closed with a stable code.
    for args in [
        vec![
            "asset",
            "read",
            &space_path,
            &asset_id,
            "--entry",
            "missing-entry",
            "--field",
            "Document",
            "-o",
            "json",
        ],
        vec![
            "asset",
            "read",
            &space_path,
            &asset_id,
            "--entry",
            "doc-1",
            "--field",
            "Missing",
            "-o",
            "json",
        ],
        vec![
            "asset",
            "download",
            &space_path,
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
    let delete = run_cli(&config_path, &["asset", "delete", &space_path, &asset_id]);
    assert!(!delete.status.success());
}

/// Backend mode projects the same read-side surface through shared operations.
#[test]
fn test_asset_read_side_backend_uses_shared_operations() {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

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
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0_u8; 4096];
                let read = stream.read(&mut buffer).expect("read request");
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
    let config_path = dir.path().join("cli-config.json");
    std::fs::write(
        &config_path,
        serde_json::json!({
            "mode": "backend",
            "backend_url": endpoint,
            "api_url": "http://127.0.0.1:3000/api"
        })
        .to_string(),
    )
    .unwrap();

    let out_path = dir.path().join("downloaded.bin");
    let download = run_cli(
        &config_path,
        &[
            "asset",
            "download",
            &space_uid,
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
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        loop {
            let mut buffer = [0_u8; 4096];
            let read = stream.read(&mut buffer).expect("read request");
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
    std::fs::write(
        &config_path,
        serde_json::json!({
            "mode": "backend",
            "backend_url": lonely_endpoint,
            "api_url": "http://127.0.0.1:3000/api"
        })
        .to_string(),
    )
    .unwrap();
    let denied = run_cli(
        &config_path,
        &[
            "asset", "read", &space_uid, "asset-1", "--entry", "doc-1", "--field", "Missing", "-o",
            "json",
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
