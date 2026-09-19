//! Integration tests for CLI endpoint routing configuration.
//! REQ-API-001, REQ-API-002, REQ-STO-001, REQ-STO-004, REQ-SEC-003

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn ugoite_bin() -> std::path::PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ugoite") {
        return std::path::PathBuf::from(path);
    }

    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path
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

fn spawn_entry_update_server() -> (String, mpsc::Receiver<Vec<String>>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut requests = Vec::with_capacity(2);
        for body in [
            r#"{"id":"task-01","revision_id":"rev-1","extra_attributes":{}}"#,
            r#"{"id":"task-01","revision_id":"rev-2"}"#,
        ] {
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for CLI backend request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("failed to accept test request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut content_length = 0_usize;
            let mut header_end = None;
            loop {
                let mut buffer = [0_u8; 1024];
                let read = match stream.read(&mut buffer) {
                    Ok(read) => read,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::Interrupted
                                | std::io::ErrorKind::WouldBlock
                        ) =>
                    {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for CLI backend request body"
                        );
                        continue;
                    }
                    Err(error) => panic!("failed to read test request body: {error}"),
                };
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if header_end.is_none() {
                    if let Some(pos) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                        let end = pos + 4;
                        header_end = Some(end);
                        let headers = String::from_utf8_lossy(&request[..end]);
                        for line in headers.lines() {
                            let mut parts = line.splitn(2, ':');
                            if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
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
            requests.push(String::from_utf8_lossy(&request).into_owned());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
        tx.send(requests).unwrap();
    });
    (format!("http://{}", addr), rx, handle)
}

fn request_json_body(request: &str) -> serde_json::Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("request has an HTTP body");
    serde_json::from_str(body).expect("request body is JSON")
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
    let body = request_json_body(&request);
    assert_eq!(body["slug"].as_str(), Some("my-space"));
    assert_eq!(body["name"].as_str(), Some("my-space"));
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
    let body = request_json_body(&request);
    assert_eq!(body["slug"].as_str(), Some("api-space"));
    assert_eq!(body["name"].as_str(), Some("api-space"));
}

/// Space create keeps the positional slug as the lookup key while `--name`
/// carries an independent display name, for both `space create` and the
/// legacy `create-space` alias in backend mode.
#[test]
fn test_space_create_sends_independent_display_name() {
    for args in [
        vec!["space", "create", "team-notes", "--name", "Team Notes"],
        vec!["create-space", "team-notes", "--name", "Team Notes"],
    ] {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let (base_url, request_rx, server_handle) = spawn_recording_server(
            "HTTP/1.1 201 Created",
            r#"{"id":"team-notes","slug":"team-notes","name":"Team Notes"}"#,
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
            .args(&args)
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .output()
            .expect("failed to execute");
        server_handle.join().unwrap();
        let request = request_rx.recv().unwrap();

        assert!(
            output.status.success(),
            "args {args:?} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            request.starts_with("POST /spaces HTTP/1.1\r\n"),
            "{request}"
        );
        let body = request_json_body(&request);
        assert_eq!(body["slug"].as_str(), Some("team-notes"), "{args:?}");
        assert_eq!(body["name"].as_str(), Some("Team Notes"), "{args:?}");
    }
}

/// REQ-API-002: entry create routes to POST /spaces/{space_id}/entries in backend mode.
#[test]
fn test_entry_create_req_api_002_routes_to_backend_post_entries() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"id":"entry-1","revision_id":"rev-1"}"#,
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
        .args([
            "entry",
            "create",
            "019f1234-5678-7abc-8def-0123456789ab",
            "entry-1",
            "--content",
            "# Remote Entry",
        ])
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
        request
            .starts_with("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/entries HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(
        !request
            .contains("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/entries/entry-1 HTTP/1.1"),
        "{request}"
    );
    assert!(request.contains(r#""id":"entry-1""#), "{request}");
    assert!(
        request.contains("\"markdown\":\"# Remote Entry\""),
        "{request}"
    );
    assert!(!request.contains(r#""author":"#), "{request}");
}

/// REQ-API-006: remote saved SQL creation uses the server-generated ID contract.
#[test]
fn test_saved_sql_create_req_api_006_uses_server_generated_id() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"id":"remote-sql-1","revision_id":"rev-1"}"#,
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
        .args([
            "sql",
            "saved-create",
            "019f1234-5678-7abc-8def-0123456789ab",
            "--name",
            "Remote query",
            "--sql",
            "SELECT 1",
        ])
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
        request.starts_with("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/sql HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(request.contains(r#""name":"Remote query"#), "{request}");
    assert!(request.contains(r#""kind":"user-query"#), "{request}");
    assert!(request.contains(r#""sql":"SELECT 1"#), "{request}");
    assert!(request.contains(r#""variables":[]"#), "{request}");
    assert!(!request.contains(r#""id":"#), "{request}");
    assert!(!request.contains(r#""author":"#), "{request}");

    let response: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("CLI should print response JSON");
    assert_eq!(response["id"].as_str(), Some("remote-sql-1"));
}

/// REQ-API-006: backend saved SQL updates send the formal optimistic-concurrency field.
#[test]
fn test_saved_sql_update_req_api_006_sends_parent_revision_without_author() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"id":"remote-sql-1","revision_id":"rev-2"}"#,
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
        .args([
            "sql",
            "saved-update",
            "019f1234-5678-7abc-8def-0123456789ab",
            "remote-sql-1",
            "--name",
            "Remote query",
            "--sql",
            "SELECT 2",
            "--parent-revision-id",
            "rev-1",
        ])
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
        request.starts_with(
            "PUT /spaces/019f1234-5678-7abc-8def-0123456789ab/sql/remote-sql-1 HTTP/1.1\r\n"
        ),
        "{request}"
    );
    assert!(
        request.contains(r#""parent_revision_id":"rev-1""#),
        "{request}"
    );
    assert!(request.contains(r#""kind":"user-query""#), "{request}");
    assert!(!request.contains(r#""author":"#), "{request}");
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
        "space create should fail in core mode without SPACE_UID_OR_PATH"
    );
    let stderr = String::from_utf8_lossy(&core_output.stderr);
    assert!(
        stderr.contains(
            "space create requires SPACE_UID_OR_PATH as /path/to/root/spaces/<slug> in core mode"
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

/// `auth login --space-uid` accepts only UUIDv7 and fails during argument
/// validation, before any device-authorization request is sent.
#[test]
fn test_auth_login_space_uid_rejects_non_v7_before_request() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

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
        .expect("failed to execute");
    assert!(set_output.status.success());

    for invalid in [
        "00000000-0000-0000-0000-000000000000",
        "123e4567-e89b-42d3-a456-426614174000",
        "team-notes",
    ] {
        let output = Command::new(ugoite_bin())
            .args(["auth", "login", "--space-uid", invalid])
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .output()
            .expect("failed to execute");
        assert!(
            !output.status.success(),
            "--space-uid {invalid} must be rejected"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("UUIDv7"),
            "--space-uid {invalid} stderr must name the UUIDv7 requirement: {stderr}"
        );
    }

    let help = Command::new(ugoite_bin())
        .args(["auth", "login", "--help"])
        .output()
        .expect("failed to execute");
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(stdout.contains("SPACE_UID"), "{stdout}");
}

/// REQ-STO-010: CLI help must explain local Space paths versus immutable UIDs.
#[test]
fn test_cli_help_req_sto_010_describes_space_uid_or_path_routing() {
    for args in [
        ["entry", "list", "--help"],
        ["form", "list", "--help"],
        ["index", "run", "--help"],
        ["search", "keyword", "--help"],
        ["space", "get", "--help"],
        ["space", "patch", "--help"],
    ] {
        let help = Command::new(ugoite_bin())
            .args(args)
            .output()
            .expect("failed to execute");
        assert!(help.status.success());
        let stdout = String::from_utf8_lossy(&help.stdout);
        assert!(stdout.contains("SPACE_UID_OR_PATH"), "{stdout}");
        assert!(!stdout.contains("SPACE_PATH"), "{stdout}");
        assert!(stdout.contains("/root/spaces/"), "{stdout}");
    }

    let create_help = Command::new(ugoite_bin())
        .args(["space", "create", "--help"])
        .output()
        .expect("failed to execute");
    assert!(create_help.status.success());
    let create_stdout = String::from_utf8_lossy(&create_help.stdout);
    assert!(
        create_stdout.contains("SPACE_SLUG_OR_PATH"),
        "{create_stdout}"
    );
    assert!(!create_stdout.contains("SPACE_PATH"), "{create_stdout}");
    assert!(create_stdout.contains("/root/spaces/"), "{create_stdout}");
    assert!(
        create_stdout.contains("--name"),
        "space create help must document the independent display-name option: {create_stdout}"
    );

    for args in [
        &["space", "--help"][..],
        &["form", "--help"][..],
        &["search", "--help"][..],
    ] {
        let help = Command::new(ugoite_bin())
            .args(args)
            .output()
            .expect("failed to execute");
        assert!(help.status.success());
        let stdout = String::from_utf8_lossy(&help.stdout);
        assert!(stdout.contains("ugoite config current"), "{stdout}");
    }

    // PR-04 help contract: Space-bound subcommand helps lead with the
    // selected context, show the --context override second, and label the
    // legacy explicit Space as 0.1.x compatibility third. (`space create`
    // and `space list` are not selected-context commands: create registers
    // a context, list takes a workspace root or nothing.)
    for args in [
        &["space", "get", "--help"][..],
        &["space", "patch", "--help"][..],
        &["form", "list", "--help"][..],
        &["form", "get", "--help"][..],
        &["form", "update", "--help"][..],
        &["search", "keyword", "--help"][..],
    ] {
        let help = Command::new(ugoite_bin())
            .args(args)
            .output()
            .expect("failed to execute");
        assert!(help.status.success());
        let stdout = String::from_utf8_lossy(&help.stdout);
        for needle in [
            "# Selected context",
            "--context NAME",
            "# 0.1.x compatibility",
        ] {
            assert!(
                stdout.contains(needle),
                "{args:?} help must carry the PR-04 tiers: {stdout}"
            );
        }
    }

    for args in [
        &["space", "--help"][..],
        &["form", "--help"][..],
        &["search", "--help"][..],
    ] {
        let help = Command::new(ugoite_bin())
            .args(args)
            .output()
            .expect("failed to execute");
        assert!(help.status.success());
        let stdout = String::from_utf8_lossy(&help.stdout);
        for needle in ["/root/spaces/<slug>", "SPACE_UID"] {
            assert!(stdout.contains(needle), "{stdout}");
        }
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

/// REQ-OPS-006: entry update help must describe its required IDs and Markdown payload flags.
#[test]
fn test_entry_update_req_ops_006_help_describes_required_inputs() {
    let help = Command::new(ugoite_bin())
        .args(["entry", "update", "--help"])
        .output()
        .expect("failed to execute");
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    for needle in [
        "ENTRY_ID",
        "Entry slug/ID",
        "--markdown <MARKDOWN>",
        "Updated entry content as a Markdown string",
        "--parent-revision-id <PARENT_REVISION_ID>",
        "optimistic concurrency checks",
        "--author <AUTHOR>",
        "Author name to record in the revision history (local only)",
    ] {
        assert!(stdout.contains(needle), "{stdout}");
    }
}

/// REQ-OPS-006: entry create help leads with structured authoring; raw
/// Markdown is the labeled 0.1.x compatibility surface (PR-04 help contract).
#[test]
fn test_entry_create_req_ops_006_help_leads_with_plain_markdown_example() {
    let help = Command::new(ugoite_bin())
        .args(["entry", "create", "--help"])
        .output()
        .expect("failed to execute");
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);

    let structured_example = "ugoite entry create task-01 --form Task --field status=open";
    let compat_example = "ugoite entry create /root/spaces/my-space my-note --content '# My Note'";

    for needle in [
        "Frontmatter is optional",
        structured_example,
        compat_example,
        "0.1.x compatibility",
        "Selected context",
        "--context NAME",
    ] {
        assert!(stdout.contains(needle), "{stdout}");
    }

    let structured_index = stdout.find(structured_example).expect("structured example");
    let compat_index = stdout.find(compat_example).expect("compat example");
    assert!(structured_index < compat_index, "{stdout}");
}

/// REQ-OPS-006: form and search help must describe required positional inputs before execution.
#[test]
fn test_form_and_search_req_ops_006_help_describes_required_inputs() {
    let form_get_help = Command::new(ugoite_bin())
        .args(["form", "get", "--help"])
        .output()
        .expect("failed to execute");
    assert!(form_get_help.status.success());
    let form_get_stdout = String::from_utf8_lossy(&form_get_help.stdout);
    for needle in [
        "FORM_NAME",
        "Form name from the form definition",
        "Selected context",
    ] {
        assert!(form_get_stdout.contains(needle), "{form_get_stdout}");
    }

    let form_update_help = Command::new(ugoite_bin())
        .args(["form", "update", "--help"])
        .output()
        .expect("failed to execute");
    assert!(form_update_help.status.success());
    let form_update_stdout = String::from_utf8_lossy(&form_update_help.stdout);
    for needle in [
        "FORM_FILE",
        "Path to a JSON form definition file",
        "Selected context",
    ] {
        assert!(form_update_stdout.contains(needle), "{form_update_stdout}");
    }

    let search_help = Command::new(ugoite_bin())
        .args(["search", "keyword", "--help"])
        .output()
        .expect("failed to execute");
    assert!(search_help.status.success());
    let search_stdout = String::from_utf8_lossy(&search_help.stdout);
    for needle in [
        "QUERY",
        "Plain-text query string to match against Entry content",
        "Selected context",
    ] {
        assert!(search_stdout.contains(needle), "{search_stdout}");
    }
}

/// REQ-API-009: sample-data help must describe the local root, target space, scenario, and seed inputs.
#[test]
fn test_space_sample_data_req_api_009_help_describes_inputs() {
    let help = Command::new(ugoite_bin())
        .args(["space", "sample-data", "--help"])
        .output()
        .expect("failed to execute");
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    for needle in [
        "LOCAL_ROOT",
        "Local workspace root",
        "SPACE_SLUG",
        "Space slug for the generated sample-data space",
        "--scenario <SCENARIO>",
        "Sample-data scenario ID",
        "--entry-count <ENTRY_COUNT>",
        "Approximate number of generated entries",
        "--seed <SEED>",
        "Deterministic random seed for reproducible sample data",
    ] {
        assert!(stdout.contains(needle), "{stdout}");
    }
}

/// Lane1 PR7: structured entry create routes the same normalized payload shape.
#[test]
fn test_entry_create_structured_routes_form_fields_without_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"id":"task-01","revision_id":"rev-1"}"#,
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
        .args([
            "entry",
            "create",
            "019f1234-5678-7abc-8def-0123456789ab",
            "task-01",
            "--form",
            "Task",
            "--title",
            "Ship 0.1.x",
            "--field",
            "status=open",
        ])
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
        request
            .starts_with("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/entries HTTP/1.1\r\n"),
        "{request}"
    );
    assert!(request.contains(r#""form":"Task""#), "{request}");
    assert!(request.contains(r#""status":"open""#), "{request}");
    assert!(!request.contains(r#""markdown""#), "{request}");
}

/// Lane1 PR8: structured entry update routes fields/title without Markdown.
#[test]
fn test_entry_update_structured_routes_fields_without_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_entry_update_server();

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
        .args([
            "entry",
            "update",
            "019f1234-5678-7abc-8def-0123456789ab",
            "task-01",
            "--title",
            "New title",
            "--field",
            "status=done",
            "--parent-revision-id",
            "rev-1",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let requests = request_rx.recv().unwrap();
    server_handle.join().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].starts_with(
            "GET /spaces/019f1234-5678-7abc-8def-0123456789ab/entries/task-01 HTTP/1.1\r\n"
        ),
        "{}",
        requests[0]
    );
    let request = &requests[1];
    assert!(
        request.starts_with(
            "PUT /spaces/019f1234-5678-7abc-8def-0123456789ab/entries/task-01 HTTP/1.1\r\n"
        ),
        "{request}"
    );
    assert!(request.contains(r#""status":"done""#), "{request}");
    assert!(request.contains(r#""title":"New title""#), "{request}");
    assert!(
        request.contains(r#""parent_revision_id":"rev-1""#),
        "{request}"
    );
    assert!(!request.contains(r#""markdown""#), "{request}");
}

/// PR5: remote entry updates default to the revision read immediately before
/// the write when no parent revision is supplied.
#[test]
fn test_entry_update_default_parent_reads_current_entry_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let (base_url, request_rx, server_handle) = spawn_entry_update_server();

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
        .args([
            "entry",
            "update",
            "019f1234-5678-7abc-8def-0123456789ab",
            "task-01",
            "--markdown",
            "# Updated",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let requests = request_rx.recv().unwrap();
    server_handle.join().unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].starts_with(
            "GET /spaces/019f1234-5678-7abc-8def-0123456789ab/entries/task-01 HTTP/1.1\r\n"
        ),
        "{}",
        requests[0]
    );
    assert!(
        requests[1].starts_with(
            "PUT /spaces/019f1234-5678-7abc-8def-0123456789ab/entries/task-01 HTTP/1.1\r\n"
        ),
        "{}",
        requests[1]
    );
    assert_eq!(
        request_json_body(&requests[1])["parent_revision_id"],
        "rev-1"
    );
}
