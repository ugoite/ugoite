//! CLI SQL session commands (`sql saved-execute`, `sql session-*`).
//!
//! Evidence identity: surface=cli, transports=core/local and remote.
//! Core mode runs shared read-only admission and paged execution; remote mode
//! sends the identical DTO through `sql.get` + `sql_session.*`. CLI stdout
//! wording is never compared; only exit status, error codes, and the stable
//! result/count/offset/limit envelope matter.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
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

fn run_cli(config: &std::path::Path, args: &[&str]) -> Output {
    Command::new(ugoite_bin())
        .args(args)
        .env("UGOITE_CLI_CONFIG_PATH", config)
        .output()
        .expect("run ugoite")
}

fn stdout_json(output: &Output, what: &str) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{what} stdout is not JSON: {stdout}"))
}

fn setup_sql_space(config_path: &std::path::Path, root: &str, slug: &str) -> (String, String) {
    let space_path = format!("{root}/spaces/{slug}");
    let output = run_cli(config_path, &["create-space", "--root", root, slug]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form_file = format!("{root}/{slug}-form.json");
    std::fs::write(
        &form_file,
        "{\"name\":\"Task\",\"version\":1,\"template\":\"# Task\\n\\n## status\\n\",\"fields\":{\"status\":{\"type\":\"string\",\"required\":true}}}",
    )
    .expect("write task form");
    let output = run_cli(config_path, &["form", "update", &space_path, &form_file]);
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (entry_id, title, status) in [
        ("task-a", "Alpha", "open"),
        ("task-b", "Beta", "open"),
        ("task-c", "Gamma", "closed"),
    ] {
        let content = format!("---\nform: Task\n---\n# {title}\n\n## status\n{status}\n");
        let output = run_cli(
            config_path,
            &[
                "entry",
                "create",
                "--content",
                &content,
                &space_path,
                entry_id,
            ],
        );
        assert!(
            output.status.success(),
            "entry {entry_id} create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = run_cli(config_path, &["form", "get", &space_path, "Task"]);
    let form = stdout_json(&output, "form get");
    let relation = form["sql_relation"]
        .as_str()
        .expect("Form SQL relation")
        .to_string();
    (space_path, relation)
}

fn create_saved_sql(
    config_path: &std::path::Path,
    space_path: &str,
    name: &str,
    sql: &str,
) -> String {
    let output = run_cli(
        config_path,
        &[
            "sql",
            "saved-create",
            "--name",
            name,
            "--sql",
            sql,
            space_path,
        ],
    );
    let created = stdout_json(&output, "saved-create");
    created["id"].as_str().expect("saved SQL id").to_string()
}

#[test]
fn sql_saved_execute_select_returns_stable_envelope_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, relation) = setup_sql_space(&config_path, &root, "sql-exec-core");
    let sql = format!("SELECT * FROM \"{relation}\" ORDER BY _ugoite_id");
    let sql_id = create_saved_sql(&config_path, &space_path, "all-tasks", &sql);

    let output = run_cli(
        &config_path,
        &["sql", "saved-execute", &space_path, &sql_id],
    );
    let body = stdout_json(&output, "saved-execute select");
    assert_eq!(body["sql_id"].as_str(), Some(sql_id.as_str()));
    assert_eq!(body["offset"].as_u64(), Some(0));
    assert_eq!(body["limit"].as_u64(), Some(50));
    let rows = body["rows"].as_array().expect("rows array");
    assert_eq!(rows.len(), 3, "{body}");
    assert_eq!(body["result"], body["rows"]);
    assert_eq!(body["count"].as_u64(), Some(3));
    assert_eq!(body["total_count"].as_u64(), Some(3));
}

#[test]
fn sql_saved_execute_empty_result_stays_stable_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, relation) = setup_sql_space(&config_path, &root, "sql-exec-empty");
    let sql = format!(
        "SELECT * FROM \"{relation}\" WHERE _ugoite_id = 'no-such-entry' ORDER BY _ugoite_id"
    );
    let sql_id = create_saved_sql(&config_path, &space_path, "empty", &sql);

    let output = run_cli(
        &config_path,
        &["sql", "saved-execute", &space_path, &sql_id],
    );
    let body = stdout_json(&output, "saved-execute empty");
    assert_eq!(body["rows"], serde_json::json!([]));
    assert_eq!(body["result"], serde_json::json!([]));
    assert_eq!(body["count"].as_u64(), Some(0));
    assert_eq!(body["total_count"].as_u64(), Some(0));
}

#[test]
fn sql_saved_execute_rejects_bad_id_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, _relation) = setup_sql_space(&config_path, &root, "sql-exec-bad-id");

    let output = run_cli(
        &config_path,
        &[
            "sql",
            "saved-execute",
            &space_path,
            "01900000-0000-7000-8000-000000000099",
        ],
    );
    assert!(!output.status.success(), "bad saved SQL id must fail");
}

#[test]
fn sql_saved_execute_paginates_with_offset_limit_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, relation) = setup_sql_space(&config_path, &root, "sql-exec-page");
    let sql = format!("SELECT * FROM \"{relation}\" ORDER BY _ugoite_id");
    let sql_id = create_saved_sql(&config_path, &space_path, "paged", &sql);

    let first = stdout_json(
        &run_cli(
            &config_path,
            &[
                "sql",
                "saved-execute",
                &space_path,
                &sql_id,
                "--offset",
                "0",
                "--limit",
                "1",
            ],
        ),
        "saved-execute page 0",
    );
    let second = stdout_json(
        &run_cli(
            &config_path,
            &[
                "sql",
                "saved-execute",
                &space_path,
                &sql_id,
                "--offset",
                "1",
                "--limit",
                "1",
            ],
        ),
        "saved-execute page 1",
    );
    assert_eq!(first["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(second["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(first["total_count"].as_u64(), Some(3));
    assert_eq!(second["total_count"].as_u64(), Some(3));
    assert_ne!(first["rows"], second["rows"], "offset must slice results");
    assert_eq!(first["offset"].as_u64(), Some(0));
    assert_eq!(second["offset"].as_u64(), Some(1));

    // Unbounded fetch is rejected fail-closed.
    let output = run_cli(
        &config_path,
        &[
            "sql",
            "saved-execute",
            &space_path,
            &sql_id,
            "--limit",
            "1001",
        ],
    );
    assert!(!output.status.success(), "over-limit fetch must fail");
}

#[test]
fn sql_saved_execute_rejects_write_sql_pre_execution_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, relation) = setup_sql_space(&config_path, &root, "sql-exec-write");
    let write_sql = format!("INSERT INTO \"{relation}\" SELECT * FROM \"{relation}\"");
    let sql_id = create_saved_sql(&config_path, &space_path, "write", &write_sql);

    let output = run_cli(
        &config_path,
        &["sql", "saved-execute", &space_path, &sql_id],
    );
    assert!(!output.status.success(), "write SQL must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("READ_ONLY_SQL_REQUIRED"),
        "write rejection must keep the policy code: {stderr}"
    );
}

#[test]
fn sql_session_lifecycle_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, relation) = setup_sql_space(&config_path, &root, "sql-session-core");
    let sql = format!("SELECT * FROM \"{relation}\" ORDER BY _ugoite_id");

    let created = stdout_json(
        &run_cli(
            &config_path,
            &["sql", "session-create", &space_path, "--sql", &sql],
        ),
        "session-create",
    );
    let session_id = created["id"].as_str().expect("session id").to_string();
    assert_eq!(created["status"].as_str(), Some("ready"));
    assert_eq!(created["sql"].as_str(), Some(sql.as_str()));

    let fetched = stdout_json(
        &run_cli(
            &config_path,
            &["sql", "session-get", &space_path, &session_id],
        ),
        "session-get",
    );
    assert_eq!(fetched["id"].as_str(), Some(session_id.as_str()));

    let metadata = stdout_json(
        &run_cli(
            &config_path,
            &["sql", "session-metadata", &space_path, &session_id],
        ),
        "session-metadata",
    );
    assert_eq!(metadata["id"].as_str(), Some(session_id.as_str()));

    let counted = stdout_json(
        &run_cli(
            &config_path,
            &["sql", "session-count", &space_path, &session_id],
        ),
        "session-count",
    );
    assert_eq!(counted["count"].as_u64(), Some(3));
    assert_eq!(counted["total_count"].as_u64(), Some(3));

    let first = stdout_json(
        &run_cli(
            &config_path,
            &[
                "sql",
                "session-rows",
                &space_path,
                &session_id,
                "--offset",
                "0",
                "--limit",
                "1",
            ],
        ),
        "session-rows page 0",
    );
    let second = stdout_json(
        &run_cli(
            &config_path,
            &[
                "sql",
                "session-rows",
                &space_path,
                &session_id,
                "--offset",
                "1",
                "--limit",
                "1",
            ],
        ),
        "session-rows page 1",
    );
    assert_eq!(first["rows"].as_array().map(Vec::len), Some(1));
    assert_ne!(first["rows"], second["rows"]);
    assert_eq!(first["result"], first["rows"]);
    assert_eq!(first["total_count"].as_u64(), Some(3));

    let bad = run_cli(
        &config_path,
        &[
            "sql",
            "session-rows",
            &space_path,
            "01900000-0000-7000-8000-000000000099",
        ],
    );
    assert!(!bad.status.success(), "bad session id must fail");

    let write = run_cli(
        &config_path,
        &[
            "sql",
            "session-create",
            &space_path,
            "--sql",
            &format!("DELETE FROM \"{relation}\""),
        ],
    );
    assert!(
        !write.status.success(),
        "write session SQL must be rejected"
    );
    let stderr = String::from_utf8_lossy(&write.stderr);
    assert!(
        stderr.contains("READ_ONLY_SQL_REQUIRED"),
        "session write rejection must keep the policy code: {stderr}"
    );
}

fn spawn_stub_server(
    handler: impl Fn(String) -> (u16, String) + Send + 'static,
    expected_requests: usize,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut served = 0_usize;
        while served < expected_requests {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .expect("read timeout");
                    let mut request = Vec::new();
                    let mut content_length = 0_usize;
                    let mut header_end = None;
                    loop {
                        let mut buffer = [0_u8; 4096];
                        let read = match stream.read(&mut buffer) {
                            Ok(read) => read,
                            Err(error)
                                if matches!(
                                    error.kind(),
                                    std::io::ErrorKind::WouldBlock
                                        | std::io::ErrorKind::TimedOut
                                        | std::io::ErrorKind::Interrupted
                                ) =>
                            {
                                assert!(Instant::now() < deadline, "timed out waiting");
                                continue;
                            }
                            Err(error) => panic!("read request: {error}"),
                        };
                        if read == 0 {
                            break;
                        }
                        request.extend_from_slice(&buffer[..read]);
                        if header_end.is_none() {
                            if let Some(pos) =
                                request.windows(4).position(|window| window == b"\r\n\r\n")
                            {
                                header_end = Some(pos + 4);
                                for line in String::from_utf8_lossy(&request[..pos]).lines().skip(1)
                                {
                                    if let Some(value) = line
                                        .strip_prefix("content-length:")
                                        .or_else(|| line.strip_prefix("Content-Length:"))
                                    {
                                        content_length =
                                            value.trim().parse().expect("content length");
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
                    let request_text = String::from_utf8_lossy(&request).into_owned();
                    tx.send(request_text.clone()).expect("send request");
                    let (status, body) = handler(request_text);
                    let reason = if status == 200 { "OK" } else { "Error" };
                    let response = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write response");
                    served += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for CLI request"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("accept: {error}"),
            }
        }
    });
    (base_url, rx, handle)
}

fn use_backend_mode(config_path: &std::path::Path, base_url: &str) {
    let output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            base_url,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("config set");
    assert!(output.status.success());
}

#[test]
fn sql_session_rows_remote_uses_stable_envelope() {
    let space_id = "019f1234-5678-7abc-8def-0123456789ab";
    let session_id = "019f1234-5678-7abc-8def-0123456789ac";
    let (base_url, rx, handle) = spawn_stub_server(
        move |request| {
            assert!(
                request.contains(&format!(
                    "GET /spaces/{space_id}/sql-sessions/{session_id}/rows?"
                )),
                "{request}"
            );
            (200, "{\"rows\":[{\"_ugoite_id\":\"task-a\"}],\"offset\":1,\"limit\":1,\"total_count\":3}".to_string())
        },
        1,
    );

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    use_backend_mode(&config_path, &base_url);

    let output = run_cli(
        &config_path,
        &[
            "sql",
            "session-rows",
            space_id,
            session_id,
            "--offset",
            "1",
            "--limit",
            "1",
        ],
    );
    handle.join().unwrap();
    let request = rx.recv().expect("captured request");
    assert!(request.contains("offset=1"), "{request}");
    assert!(request.contains("limit=1"), "{request}");
    let body = stdout_json(&output, "remote session-rows");
    assert_eq!(body["rows"], serde_json::json!([{"_ugoite_id": "task-a"}]));
    assert_eq!(body["result"], body["rows"]);
    assert_eq!(body["count"].as_u64(), Some(1));
    assert_eq!(body["total_count"].as_u64(), Some(3));
    assert_eq!(body["offset"].as_u64(), Some(1));
    assert_eq!(body["limit"].as_u64(), Some(1));
    assert_eq!(body["session_id"].as_str(), Some(session_id));
}

#[test]
fn sql_saved_execute_remote_reuses_saved_sql_and_session_ops() {
    let space_id = "019f1234-5678-7abc-8def-0123456789ab";
    let sql_id = "019f1234-5678-7abc-8def-0123456789ad";
    let session_id = "019f1234-5678-7abc-8def-0123456789ae";
    let (base_url, rx, handle) = spawn_stub_server(
        move |request| {
            if request.starts_with(&format!("GET /spaces/{space_id}/sql/{sql_id} ")) {
                (
                    200,
                    "{\"id\":\"saved-1\",\"sql\":\"SELECT * FROM t ORDER BY _ugoite_id\"}"
                        .to_string(),
                )
            } else if request.starts_with(&format!("POST /spaces/{space_id}/sql-sessions ")) {
                assert!(request.contains("SELECT * FROM t"), "{request}");
                (
                    200,
                    format!("{{\"id\":\"{session_id}\",\"status\":\"ready\"}}"),
                )
            } else if request.starts_with(&format!(
                "GET /spaces/{space_id}/sql-sessions/{session_id}/rows?"
            )) {
                (
                    200,
                    "{\"rows\":[],\"offset\":0,\"limit\":50,\"total_count\":0}".to_string(),
                )
            } else {
                panic!("unexpected request: {request}");
            }
        },
        3,
    );

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    use_backend_mode(&config_path, &base_url);

    let output = run_cli(&config_path, &["sql", "saved-execute", space_id, sql_id]);
    handle.join().unwrap();
    let body = stdout_json(&output, "remote saved-execute");
    assert_eq!(body["rows"], serde_json::json!([]));
    assert_eq!(body["count"].as_u64(), Some(0));
    assert_eq!(body["total_count"].as_u64(), Some(0));
    assert_eq!(body["offset"].as_u64(), Some(0));
    assert_eq!(body["limit"].as_u64(), Some(50));
    assert_eq!(body["sql_id"].as_str(), Some(sql_id));
    assert_eq!(body["session_id"].as_str(), Some(session_id));
    // Drain requests to prove the three shared operations were used in order.
    let first = rx.recv().expect("first request");
    let second = rx.recv().expect("second request");
    let third = rx.recv().expect("third request");
    assert!(first.contains("/sql/"), "{first}");
    assert!(second.contains("/sql-sessions "), "{second}");
    assert!(third.contains("/rows?"), "{third}");
}

fn assert_malformed_remote_response(output: &Output, what: &str) {
    assert!(
        !output.status.success(),
        "{what} must fail loudly: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "{what} must not print partial success JSON to stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("malformed remote response"),
        "{what} must use the remote-response error path: {stderr}"
    );
}

#[test]
fn sql_session_rows_remote_rejects_missing_total_count() {
    let space_id = "019f1234-5678-7abc-8def-0123456789ab";
    let session_id = "019f1234-5678-7abc-8def-0123456789ac";
    let (base_url, _rx, handle) = spawn_stub_server(
        |_| {
            (
                200,
                "{\"rows\":[{\"_ugoite_id\":\"task-a\"}],\"offset\":1,\"limit\":1}".to_string(),
            )
        },
        1,
    );

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    use_backend_mode(&config_path, &base_url);

    let output = run_cli(
        &config_path,
        &[
            "sql",
            "session-rows",
            space_id,
            session_id,
            "--offset",
            "1",
            "--limit",
            "1",
        ],
    );
    handle.join().unwrap();
    assert_malformed_remote_response(&output, "missing total_count");
}

#[test]
fn sql_session_rows_remote_rejects_string_offset() {
    let space_id = "019f1234-5678-7abc-8def-0123456789ab";
    let session_id = "019f1234-5678-7abc-8def-0123456789ac";
    let (base_url, _rx, handle) = spawn_stub_server(
        |_| {
            (
                200,
                "{\"rows\":[],\"total_count\":0,\"offset\":\"0\",\"limit\":50}".to_string(),
            )
        },
        1,
    );

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    use_backend_mode(&config_path, &base_url);

    let output = run_cli(&config_path, &["sql", "session-rows", space_id, session_id]);
    handle.join().unwrap();
    assert_malformed_remote_response(&output, "string offset");
}

#[test]
fn sql_session_count_remote_rejects_missing_count() {
    let space_id = "019f1234-5678-7abc-8def-0123456789ab";
    let session_id = "019f1234-5678-7abc-8def-0123456789ac";
    let (base_url, _rx, handle) =
        spawn_stub_server(|_| (200, "{\"total_count\":3}".to_string()), 1);

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    use_backend_mode(&config_path, &base_url);

    let output = run_cli(
        &config_path,
        &["sql", "session-count", space_id, session_id],
    );
    handle.join().unwrap();
    assert_malformed_remote_response(&output, "missing count");
}

/// A server error envelope keeps the server-error path: the server code is
/// surfaced, never reinterpreted as shape drift.
#[test]
fn sql_session_count_remote_server_error_keeps_server_error_path() {
    let space_id = "019f1234-5678-7abc-8def-0123456789ab";
    let session_id = "019f1234-5678-7abc-8def-0123456789ac";
    let (base_url, _rx, handle) = spawn_stub_server(
        |_| {
            (
                410,
                "{\"code\":\"SQL_SESSION_EXPIRED\",\"message\":\"SQL session expired\"}"
                    .to_string(),
            )
        },
        1,
    );

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    use_backend_mode(&config_path, &base_url);

    let output = run_cli(
        &config_path,
        &["sql", "session-count", space_id, session_id],
    );
    handle.join().unwrap();
    assert!(
        !output.status.success(),
        "server error must fail: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "server error must leave stdout empty: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("SQL_SESSION_EXPIRED"),
        "server code must be surfaced: {stderr}"
    );
    assert!(
        !stderr.contains("malformed remote response"),
        "server errors must not be reinterpreted as shape drift: {stderr}"
    );
}

/// SQL session state I/O failures mention the logical session ID and the OS
/// cause, never the absolute configured session directory.
#[test]
fn sql_session_create_fs_failure_redacts_absolute_session_dir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, relation) = setup_sql_space(&config_path, &root, "sql-session-redact");
    // Block session state creation with a regular file where the CLI
    // session directory must live: the OS cause fires without chmod games.
    std::fs::write(
        dir.path().join(".ugoite-cli-sql-sessions"),
        b"not a directory",
    )
    .expect("block session dir");
    let sql = format!("SELECT * FROM \"{relation}\" ORDER BY _ugoite_id");

    let output = run_cli(
        &config_path,
        &["sql", "session-create", &space_path, "--sql", &sql],
    );
    assert!(
        !output.status.success(),
        "blocked session state must fail: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.is_empty(),
        "machine-readable stdout must stay clean: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains(&root),
        "stderr must not print the absolute session directory: {stderr}"
    );
    assert!(
        stderr.contains("SQL session"),
        "stderr keeps the logical session context: {stderr}"
    );
    assert!(
        stderr.contains("os error"),
        "stderr keeps the OS root cause: {stderr}"
    );
}
