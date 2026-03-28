//! Integration tests for CLI endpoint routing configuration.
//! REQ-API-001, REQ-STO-001, REQ-STO-004, REQ-SEC-003

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
}

fn spawn_json_server(body: &'static str) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buffer = [0_u8; 1024];
                    let _ = stream.read(&mut buffer);
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for CLI backend request"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("failed to accept test request: {error}"),
            }
        }
    });
    (format!("http://{}", addr), handle)
}

fn spawn_recording_server(
    status_line: &'static str,
    body: &'static str,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut content_length = 0_usize;
                    let mut header_end: Option<usize> = None;

                    loop {
                        let mut buffer = [0_u8; 1024];
                        let read = match stream.read(&mut buffer) {
                            Ok(read) => read,
                            Err(error)
                                if error.kind() == std::io::ErrorKind::WouldBlock
                                    || error.kind() == std::io::ErrorKind::TimedOut
                                    || error.kind() == std::io::ErrorKind::Interrupted =>
                            {
                                assert!(
                                    Instant::now() < deadline,
                                    "timed out waiting for CLI backend request body"
                                );
                                thread::sleep(Duration::from_millis(10));
                                continue;
                            }
                            Err(error) => panic!("failed to read test request body: {error}"),
                        };
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if header_end.is_none() {
                            if let Some(pos) =
                                request.windows(4).position(|window| window == b"\r\n\r\n")
                            {
                                let end = pos + 4;
                                header_end = Some(end);
                                let headers = String::from_utf8_lossy(&request[..end]);
                                for line in headers.lines() {
                                    let mut parts = line.splitn(2, ':');
                                    if let (Some(name), Some(value)) = (parts.next(), parts.next())
                                    {
                                        if name.eq_ignore_ascii_case("Content-Length") {
                                            content_length = value.trim().parse().unwrap_or(0);
                                        }
                                    }
                                }
                            }
                        }
                        if let Some(end) = header_end {
                            if request.len() >= end + content_length {
                                break;
                            }
                        }
                    }

                    tx.send(String::from_utf8_lossy(&request).into_owned())
                        .unwrap();
                    let response = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    return;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for CLI backend request"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("failed to accept test request: {error}"),
            }
        }
    });
    (format!("http://{}", addr), rx, handle)
}

/// REQ-STO-001: Config set/show round-trips correctly.
#[test]
fn test_cli_config_set_and_show() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://localhost:9000",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let show_output = Command::new(ugoite_bin())
        .args(["config", "show"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(show_output.status.success());

    let stdout = String::from_utf8_lossy(&show_output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(v["mode"].as_str(), Some("backend"));
    assert_eq!(v["backend_url"].as_str(), Some("http://localhost:9000"));
}

/// REQ-STO-004: Space list uses remote endpoint when backend mode is configured.
#[test]
fn test_space_list_uses_remote_endpoint_when_backend_mode() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    // Set to backend mode with an unreachable URL
    Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://127.0.0.1:19999",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Space list should attempt to contact backend (and fail since it's unreachable)
    let output = Command::new(ugoite_bin())
        .args(["space", "list"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should fail (backend unreachable), confirming routing to backend
    assert!(
        !output.status.success(),
        "Expected failure connecting to unreachable backend"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Cannot drop a runtime"),
        "CLI should return a normal connection error instead of panicking: {stderr}"
    );
}

/// REQ-API-001: create-space routes to POST /spaces in backend mode.
#[test]
fn test_create_space_req_api_001_routes_to_backend_post_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"id":"my-space","name":"my-space"}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            &base_url,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["create-space", "my-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    server_handle.join().unwrap();
    let request = request_rx.recv().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with("POST /spaces HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(
        !request.contains("POST /spaces/my-space HTTP/1.1"),
        "{request}"
    );
    assert!(request.contains(r#"{"name":"my-space"}"#), "{request}");
}

/// REQ-API-001: create-space routes to POST /spaces in api mode.
#[test]
fn test_create_space_req_api_001_routes_to_api_post_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"id":"api-space","name":"api-space"}"#,
    );

    let set_output = Command::new(ugoite_bin())
        .args(["config", "set", "--mode", "api", "--api-url", &base_url])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["create-space", "api-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    server_handle.join().unwrap();
    let request = request_rx.recv().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with("POST /spaces HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(
        !request.contains("POST /spaces/api-space HTTP/1.1"),
        "{request}"
    );
    assert!(request.contains(r#"{"name":"api-space"}"#), "{request}");
}

/// REQ-STO-004: Backend mode returns remote space JSON without Tokio runtime panic.
#[test]
fn test_space_list_req_sto_004_returns_remote_json_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, server_handle) =
        spawn_json_server(r#"[{"id":"remote-space","name":"Remote Space"}]"#);

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            &base_url,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["space", "list"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    server_handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Cannot drop a runtime"),
        "CLI should not panic in backend mode: {stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(value[0]["id"].as_str(), Some("remote-space"));
}

/// REQ-STO-010: Core-mode commands require an explicit local root, while backend mode does not.
#[test]
fn test_create_space_req_sto_010_requires_root_only_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let core_output = Command::new(ugoite_bin())
        .args(["space", "create", "local-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        !core_output.status.success(),
        "space create should fail in core mode without SPACE_ID_OR_PATH"
    );
    let stderr = String::from_utf8_lossy(&core_output.stderr);
    assert!(
        stderr.contains(
            "space create requires SPACE_ID_OR_PATH as /path/to/root/spaces/<id> in core mode"
        ),
        "{stderr}"
    );

    let legacy_output = Command::new(ugoite_bin())
        .args(["create-space", "local-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        !legacy_output.status.success(),
        "create-space should still fail in core mode without --root"
    );
    let legacy_stderr = String::from_utf8_lossy(&legacy_output.stderr);
    assert!(
        legacy_stderr.contains("create-space requires --root <LOCAL_ROOT> in core mode"),
        "{legacy_stderr}"
    );
}

/// REQ-STO-010: Space list accepts backend mode without a local root argument.
#[test]
fn test_space_list_req_sto_010_accepts_backend_mode_without_local_root() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, server_handle) =
        spawn_json_server(r#"[{"id":"remote-space","name":"Remote Space"}]"#);

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            &base_url,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(set_output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["space", "list"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    server_handle.join().unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(value[0]["id"].as_str(), Some("remote-space"));
}

/// REQ-STO-010: space help describes the shared core-mode path convention with concrete examples.
#[test]
fn test_entry_and_form_list_req_sto_010_use_space_id_or_path_help() {
    for args in [
        ["entry", "list", "--help"],
        ["form", "list", "--help"],
        ["asset", "list", "--help"],
        ["index", "run", "--help"],
        ["search", "keyword", "--help"],
        ["space", "create", "--help"],
        ["space", "get", "--help"],
        ["space", "patch", "--help"],
    ] {
        let help = Command::new(ugoite_bin())
            .args(args)
            .output()
            .expect("failed to execute");
        assert!(help.status.success());
        let stdout = String::from_utf8_lossy(&help.stdout);
        assert!(stdout.contains("SPACE_ID_OR_PATH"), "{stdout}");
        assert!(!stdout.contains("SPACE_PATH"), "{stdout}");
        assert!(stdout.contains("/root/spaces/"), "{stdout}");
    }

    let list_help = Command::new(ugoite_bin())
        .args(["space", "list", "--help"])
        .output()
        .expect("failed to execute");
    assert!(list_help.status.success());
    let list_stdout = String::from_utf8_lossy(&list_help.stdout);
    assert!(list_stdout.contains("ROOT_PATH"), "{list_stdout}");
    assert!(list_stdout.contains("/root/spaces"), "{list_stdout}");
}

/// REQ-SEC-003: Service account create routes to backend when in backend mode.
#[test]
fn test_space_service_account_create_routes_to_backend() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    // Set to backend mode with an unreachable URL
    Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://127.0.0.1:19999",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Service account creation should attempt to route to backend
    let output = Command::new(ugoite_bin())
        .args([
            "space",
            "service-account-create",
            "my-space",
            "--display-name",
            "sa-name",
            "--scopes",
            "spaces:read",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should fail (backend unreachable or not in core mode), confirming routing to backend
    assert!(
        !output.status.success(),
        "Expected failure - service account requires backend mode"
    );
}
