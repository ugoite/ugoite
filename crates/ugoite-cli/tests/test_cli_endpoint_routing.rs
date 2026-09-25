//! Canonical CLI routing and help contracts.
//! These tests exercise named connections/contexts only.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

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

fn run(config: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(ugoite_bin());
    command.args(["--config", config.to_string_lossy().as_ref()]);
    command.args(args);
    command.output().expect("failed to execute CLI")
}

fn init_config(config: &Path) {
    let output = run(config, &["config", "init"]);
    assert!(
        output.status.success(),
        "config init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn set_connection(config: &Path, kind: &str, value: &str) {
    let mut args = vec!["config", "connection", "set", "local", "--type", kind];
    if kind == "core" {
        args.extend(["--root", value]);
    } else {
        args.extend(["--url", value]);
    }
    let output = run(config, &args);
    assert!(
        output.status.success(),
        "connection set failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn add_context(config: &Path, uid: &str) {
    let output = run(
        config,
        &[
            "context",
            "add",
            "test",
            "--connection",
            "local",
            "--space",
            uid,
        ],
    );
    assert!(
        output.status.success(),
        "context add failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = run(config, &["context", "use", "test"]);
    assert!(
        output.status.success(),
        "context use failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn spawn_recording_server(
    status_line: &'static str,
    body: &'static str,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    spawn_recording_server_responses(vec![(status_line, body)])
}

fn spawn_recording_server_responses(
    responses: Vec<(&'static str, &'static str)>,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        for (status_line, body) in responses {
            let deadline = Instant::now() + Duration::from_secs(10);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "timed out waiting for CLI request"
                        );
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("failed to accept test request: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut content_length = 0usize;
            let mut header_end = None;
            loop {
                let mut buffer = [0u8; 4096];
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
                        assert!(Instant::now() < deadline, "timed out reading CLI request");
                        continue;
                    }
                    Err(error) => panic!("failed to read test request: {error}"),
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
                            if let Some((name, value)) = line.split_once(':') {
                                if name.eq_ignore_ascii_case("content-length") {
                                    content_length = value.trim().parse().unwrap_or_default();
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
                "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
        }
    });
    (format!("http://{addr}"), rx, handle)
}

fn request_json_body(request: &str) -> serde_json::Value {
    let body = request
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("request has an HTTP body");
    serde_json::from_str(body).expect("request body is JSON")
}

#[test]
fn test_cli_config_current_reports_canonical_connection() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    init_config(&config);
    set_connection(&config, "backend", "http://localhost:9000");

    let output = run(&config, &["config", "connection", "list"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("backend"), "{stdout}");
    assert!(stdout.contains("http://localhost:9000"), "{stdout}");
    assert!(!stdout.contains("backend_url"), "{stdout}");
}

#[test]
fn test_create_space_req_api_001_routes_to_backend_post_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"space_uid":"019f1234-5678-7abc-8def-0123456789ab","slug":"my-space","name":"My Space"}"#,
    );
    init_config(&config);
    set_connection(&config, "backend", &base_url);

    let output = run(
        &config,
        &["space", "create", "my-space", "--name", "My Space"],
    );
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with("POST /spaces HTTP/1.1\r\n"),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(body["slug"], "my-space");
    assert_eq!(body["name"], "My Space");
}

#[test]
fn test_create_space_req_api_001_routes_to_api_post_spaces() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"space_uid":"019f1234-5678-7abc-8def-0123456789ab","slug":"api-space","name":"API Space"}"#,
    );
    init_config(&config);
    set_connection(&config, "api", &base_url);

    let output = run(
        &config,
        &["space", "create", "api-space", "--name", "API Space"],
    );
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with("POST /spaces HTTP/1.1\r\n"),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(body["slug"], "api-space");
    assert_eq!(body["name"], "API Space");
}

#[test]
fn test_space_create_sends_independent_display_name() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"space_uid":"019f1234-5678-7abc-8def-0123456789ab","slug":"team-notes","name":"Team Notes"}"#,
    );
    init_config(&config);
    set_connection(&config, "backend", &base_url);

    let output = run(
        &config,
        &["space", "create", "team-notes", "--name", "Team Notes"],
    );
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body = request_json_body(&request);
    assert_eq!(body["slug"], "team-notes");
    assert_eq!(body["name"], "Team Notes");
}

#[test]
fn test_entry_create_req_api_002_routes_to_backend_post_entries() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"id":"entry-1","revision_id":"rev-1"}"#,
    );
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(
        &config,
        &[
            "entry",
            "create",
            "--id",
            "entry-1",
            "--form",
            "Task",
            "--field",
            "status=open",
        ],
    );
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request
            .starts_with("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/entries HTTP/1.1\r\n"),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(body["form"], "Task");
    assert_eq!(body["fields"]["status"], "open");
}

#[test]
fn test_entry_list_uses_canonical_entry_query_route_and_dto() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"rows":[],"has_more":false}"#);
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(
        &config,
        &[
            "entry",
            "list",
            "--text",
            "planning",
            "--sort",
            "updated_at:desc",
            "--columns",
            "form",
            "--limit",
            "2",
        ],
    );
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with(
            "POST /spaces/019f1234-5678-7abc-8def-0123456789ab/entries/query HTTP/1.1\r\n"
        ),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(body["query"]["scope"]["kind"], "all");
    assert_eq!(body["query"]["text"], "planning");
    assert_eq!(body["query"]["sort"][0]["field"]["kind"], "updated_at");
    assert_eq!(body["query"]["sort"][0]["direction"], "desc");
    assert_eq!(body["projection"]["kind"], "fields");
    assert_eq!(body["projection"]["fields"][0]["kind"], "form");
    assert_eq!(body["limit"], 2);
    assert!(!body.as_object().unwrap().contains_key("after"));
}

#[test]
fn test_sql_query_uses_stateless_route_and_dto() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"{"columns":["status"],"rows":[{"status":"open"}],"has_more":true,"next":"continuation-1"}"#,
    );
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(
        &config,
        &[
            "sql",
            "query",
            "SELECT status FROM tasks WHERE status = $status ORDER BY status",
            "--param",
            "status=open",
            "--limit",
            "2",
        ],
    );
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with(
            "POST /spaces/019f1234-5678-7abc-8def-0123456789ab/sql/query HTTP/1.1\r\n"
        ),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(
        body["sql"],
        "SELECT status FROM tasks WHERE status = $status ORDER BY status"
    );
    assert_eq!(body["parameters"]["status"], "open");
    assert_eq!(body["parameter_types"]["status"], "string");
    assert_eq!(body["limit"], 2);
    assert!(body["continuation"].is_null());
}

#[test]
fn test_sql_export_reuses_stateless_route_and_continuation() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let first =
        r#"{"columns":["id"],"rows":[{"id":"a"},{"id":"b"}],"has_more":true,"next":"opaque-1"}"#;
    let last = r#"{"columns":["id"],"rows":[{"id":"c"}],"has_more":false,"next":null}"#;
    let (base_url, request_rx, server) = spawn_recording_server_responses(vec![
        ("HTTP/1.1 200 OK", first),
        ("HTTP/1.1 200 OK", last),
    ]);
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");
    let path = dir.path().join("remote.ndjson");

    let output = run(
        &config,
        &[
            "sql",
            "export",
            "SELECT id FROM things ORDER BY id",
            "--max-rows",
            "3",
            "--page-size",
            "2",
            "--output",
            path.to_str().unwrap(),
        ],
    );
    let requests = [request_rx.recv().unwrap(), request_rx.recv().unwrap()];
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(requests.iter().all(|request| request
        .starts_with("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/sql/query HTTP/1.1\r\n")));
    let first_body = request_json_body(&requests[0]);
    let second_body = request_json_body(&requests[1]);
    assert_eq!(first_body["limit"], 2);
    assert!(first_body["continuation"].is_null());
    assert_eq!(second_body["limit"], 1);
    assert_eq!(second_body["continuation"], "opaque-1");
    assert_eq!(std::fs::read_to_string(path).unwrap().lines().count(), 3);
}

#[test]
fn test_sql_export_discards_file_after_remote_failure_without_leaking_token() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let first = r#"{"columns":["id"],"rows":[{"id":"a"}],"has_more":true,"next":"secret-token"}"#;
    let denied = r#"{"code":"FORBIDDEN","kind":"forbidden","message":"Access revoked"}"#;
    let (base_url, request_rx, server) = spawn_recording_server_responses(vec![
        ("HTTP/1.1 200 OK", first),
        ("HTTP/1.1 403 Forbidden", denied),
    ]);
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");
    let path = dir.path().join("revoked.ndjson");

    let output = run(
        &config,
        &[
            "sql",
            "export",
            "SELECT id FROM things ORDER BY id",
            "--max-rows",
            "3",
            "--page-size",
            "1",
            "--output",
            path.to_str().unwrap(),
        ],
    );
    let _ = request_rx.recv().unwrap();
    let second_request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(!output.status.success());
    assert!(second_request.contains("secret-token"));
    assert!(!path.exists());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("FORBIDDEN"), "{stderr}");
    assert!(stderr.contains("rows_exported"), "{stderr}");
    assert!(!stderr.contains("secret-token"), "{stderr}");
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".ugoite-export-")
    }));
}

#[test]
fn test_sql_query_count_uses_separate_stateless_route() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) =
        spawn_recording_server("HTTP/1.1 200 OK", r#"{"count":7}"#);
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(&config, &["sql", "count", "SELECT 1"]);
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with(
            "POST /spaces/019f1234-5678-7abc-8def-0123456789ab/sql/query/count HTTP/1.1\r\n"
        ),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(body["sql"], "SELECT 1");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["count"],
        7
    );
}

#[test]
fn test_saved_sql_create_req_api_006_uses_server_generated_id() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server_responses(vec![
        (
            "HTTP/1.1 201 Created",
            r#"{"id":"remote-sql-1","revision_id":"rev-1"}"#,
        ),
        (
            "HTTP/1.1 201 Created",
            r#"{"id":"remote-sql-2","revision_id":"rev-2"}"#,
        ),
    ]);
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(&config, &["sql", "saved", "create", "--sql", "SELECT 1"]);
    let unnamed_request = request_rx.recv().unwrap();
    let named_output = run(
        &config,
        &[
            "sql",
            "saved",
            "create",
            "--name",
            "Remote query",
            "--sql",
            "SELECT 2",
        ],
    );
    let named_request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        unnamed_request
            .starts_with("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/sql HTTP/1.1\r\n"),
        "{unnamed_request}"
    );
    assert!(
        named_output.status.success(),
        "{}",
        String::from_utf8_lossy(&named_output.stderr)
    );
    let unnamed_body = request_json_body(&unnamed_request);
    assert!(unnamed_body["name"].is_null());
    assert_eq!(unnamed_body["metadata"]["generatedName"], "untitled");
    assert_eq!(unnamed_body["sql"], "SELECT 1");
    assert!(!unnamed_body.as_object().unwrap().contains_key("id"));

    let named_body = request_json_body(&named_request);
    assert_eq!(named_body["name"], "Remote query");
    assert_eq!(named_body["sql"], "SELECT 2");
    assert!(named_body["metadata"].is_null());
    assert!(!named_body.as_object().unwrap().contains_key("id"));
}

#[test]
fn test_saved_sql_update_req_api_006_sends_parent_revision_without_author() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server_responses(vec![
        (
            "HTTP/1.1 200 OK",
            r#"{"id":"remote-sql-1","name":"Remote query","kind":"user-query","metadata":null,"sql":"SELECT 1","variables":[],"revision_id":"rev-current"}"#,
        ),
        (
            "HTTP/1.1 200 OK",
            r#"{"id":"remote-sql-1","revision_id":"rev-2"}"#,
        ),
    ]);
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(
        &config,
        &[
            "sql",
            "saved",
            "update",
            "remote-sql-1",
            "--sql",
            "SELECT 2",
            "--parent-revision-id",
            "rev-1",
        ],
    );
    let get_request = request_rx.recv().unwrap();
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        get_request.starts_with(
            "GET /spaces/019f1234-5678-7abc-8def-0123456789ab/sql/remote-sql-1 HTTP/1.1\r\n"
        ),
        "{get_request}"
    );
    assert!(
        request.starts_with(
            "PUT /spaces/019f1234-5678-7abc-8def-0123456789ab/sql/remote-sql-1 HTTP/1.1\r\n"
        ),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(body["parent_revision_id"], "rev-1");
    assert_eq!(body["name"], "Remote query");
    assert_eq!(body["sql"], "SELECT 2");
    assert_eq!(body["variables"], serde_json::json!([]));
    assert!(!body.as_object().unwrap().contains_key("author"));
}

#[test]
fn test_saved_sql_update_defaults_to_current_revision() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server_responses(vec![
        (
            "HTTP/1.1 200 OK",
            r#"{"id":"remote-sql-1","name":"Remote query","kind":"user-query","metadata":null,"sql":"SELECT 1","variables":[],"revision_id":"rev-current"}"#,
        ),
        (
            "HTTP/1.1 200 OK",
            r#"{"id":"remote-sql-1","revision_id":"rev-2"}"#,
        ),
    ]);
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(
        &config,
        &[
            "sql",
            "saved",
            "update",
            "remote-sql-1",
            "--sql",
            "SELECT 3",
        ],
    );
    let get_request = request_rx.recv().unwrap();
    let update_request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(get_request.starts_with("GET /spaces/"), "{get_request}");
    assert!(
        update_request.starts_with("PUT /spaces/"),
        "{update_request}"
    );
    let body = request_json_body(&update_request);
    assert_eq!(body["parent_revision_id"], "rev-current");
    assert_eq!(body["name"], "Remote query");
    assert_eq!(body["sql"], "SELECT 3");
}

#[test]
fn test_entry_create_structured_routes_form_fields_without_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 201 Created",
        r#"{"id":"entry-1","revision_id":"rev-1"}"#,
    );
    init_config(&config);
    set_connection(&config, "backend", &base_url);
    add_context(&config, "019f1234-5678-7abc-8def-0123456789ab");

    let output = run(
        &config,
        &[
            "--context",
            "test",
            "entry",
            "create",
            "--id",
            "entry-1",
            "--form",
            "Task",
            "--field",
            "status=open",
        ],
    );
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request
            .starts_with("POST /spaces/019f1234-5678-7abc-8def-0123456789ab/entries HTTP/1.1\r\n"),
        "{request}"
    );
    let body = request_json_body(&request);
    assert_eq!(body["form"], "Task");
    assert_eq!(body["fields"]["status"], "open");
    assert!(!body.as_object().unwrap().contains_key("markdown"));
}

#[test]
fn test_create_space_req_sto_010_requires_root_only_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let root = dir.path().join("workspace");
    std::fs::create_dir_all(&root).unwrap();
    init_config(&config);
    set_connection(&config, "core", root.to_str().unwrap());

    let output = run(&config, &["space", "create", "local-space"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("space_uid"), "{stdout}");
    assert!(stdout.contains("local-space"), "{stdout}");
}

#[test]
fn test_space_list_req_sto_004_returns_remote_json_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"[{"space_uid":"019f1234-5678-7abc-8def-0123456789ab","slug":"remote-space"}]"#,
    );
    init_config(&config);
    set_connection(&config, "backend", &base_url);

    let output = run(&config, &["space", "list"]);
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(request.starts_with("GET /spaces HTTP/1.1\r\n"), "{request}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("remote-space"), "{stdout}");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Cannot drop a runtime"));
}

#[test]
fn test_space_list_req_sto_010_accepts_backend_mode_without_local_root() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let (base_url, request_rx, server) = spawn_recording_server(
        "HTTP/1.1 200 OK",
        r#"[{"space_uid":"019f1234-5678-7abc-8def-0123456789ab","slug":"remote-space"}]"#,
    );
    init_config(&config);
    set_connection(&config, "backend", &base_url);

    let output = run(&config, &["space", "list"]);
    let request = request_rx.recv().unwrap();
    server.join().unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(request.starts_with("GET /spaces HTTP/1.1\r\n"), "{request}");
}

#[test]
fn test_cli_help_req_sto_010_describes_context_routing() {
    for args in [
        ["entry", "list", "--help"],
        ["form", "list", "--help"],
        ["index", "run", "--help"],
        ["entry", "list", "--help"],
        ["space", "get", "--help"],
        ["space", "patch", "--help"],
    ] {
        let output = Command::new(ugoite_bin()).args(args).output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("selected context"), "{stdout}");
        assert!(stdout.contains("--context <NAME>"), "{stdout}");
        assert!(!stdout.contains("SPACE_UID_OR_PATH"), "{stdout}");
        assert!(!stdout.contains("SPACE_PATH"), "{stdout}");
        assert!(!stdout.contains("0.1.x compatibility"), "{stdout}");
    }
}

#[test]
fn test_entry_update_req_ops_006_help_describes_required_inputs() {
    let output = Command::new(ugoite_bin())
        .args(["entry", "update", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "ENTRY_ID",
        "--form <FORM>",
        "--field <KEY=VALUE>",
        "--fields-file <PATH>",
        "--parent-revision-id <PARENT_REVISION_ID>",
        "--context <NAME>",
    ] {
        assert!(stdout.contains(needle), "{stdout}");
    }
}

#[test]
fn test_entry_create_req_ops_006_help_leads_with_structured_fields() {
    let output = Command::new(ugoite_bin())
        .args(["entry", "create", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for needle in [
        "--id <ENTRY_ID>",
        "--form <FORM>",
        "--field <KEY=VALUE>",
        "--fields-file <PATH>",
        "--context <NAME>",
    ] {
        assert!(stdout.contains(needle), "{stdout}");
    }
}

#[test]
fn test_form_and_entry_list_req_ops_006_help_describes_required_inputs() {
    for args in [["form", "get", "--help"], ["entry", "list", "--help"]] {
        let output = Command::new(ugoite_bin()).args(args).output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--context <NAME>"), "{stdout}");
        assert!(!stdout.contains("SPACE_UID_OR_PATH"), "{stdout}");
    }
}
