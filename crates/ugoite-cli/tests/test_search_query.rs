//! Typed structured Search through the CLI (`search query`).
//!
//! Evidence identity: surface=cli, transports=core/local and remote.
//! Core mode runs the shared use case; remote mode sends the identical DTO
//! through `search.query`. CLI stdout wording is never compared; only exit
//! status, error codes, and the returned durable state matter.

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

fn result_ids(results: &serde_json::Value) -> Vec<String> {
    results
        .as_array()
        .unwrap_or_else(|| panic!("search results must be an array: {results}"))
        .iter()
        .map(|row| {
            row.get("_ugoite_id")
                .or_else(|| row.get("id"))
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("search row has no id: {row}"))
                .to_string()
        })
        .collect()
}

fn setup_task_space(config_path: &std::path::Path, root: &str, space_id: &str) -> String {
    let space_path = format!("{root}/spaces/{space_id}");
    let output = run_cli(config_path, &["create-space", "--root", root, space_id]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form_file = format!("{root}/task-form.json");
    std::fs::write(
        &form_file,
        "{\"name\":\"Task\",\"version\":1,\"template\":\"# Task\\n\\n## status\\n\\n## priority\\n\",\"fields\":{\"status\":{\"type\":\"string\",\"required\":true},\"priority\":{\"type\":\"integer\",\"required\":false}}}",
    )
    .expect("write task form");
    let output = run_cli(config_path, &["form", "update", &space_path, &form_file]);
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let open = "---\nform: Task\n---\n# Release\n\n## status\nopen\n\n## priority\n3\n";
    let output = run_cli(
        config_path,
        &[
            "entry",
            "create",
            "--content",
            open,
            &space_path,
            "task-open",
        ],
    );
    assert!(
        output.status.success(),
        "entry create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let closed = "---\nform: Task\n---\n# Cleanup\n\n## status\nclosed\n\n## priority\n7\n";
    let output = run_cli(
        config_path,
        &[
            "entry",
            "create",
            "--content",
            closed,
            &space_path,
            "task-closed",
        ],
    );
    assert!(
        output.status.success(),
        "entry create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    space_path
}

#[test]
fn search_query_flags_filter_entries_in_core_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = setup_task_space(&config_path, &root, "search-query-core");

    let results = stdout_json(
        &run_cli(
            &config_path,
            &[
                "search",
                "query",
                &space_path,
                "--form",
                "Task",
                "--eq",
                "status=open",
            ],
        ),
        "search query eq",
    );
    assert_eq!(result_ids(&results), vec!["task-open".to_string()]);

    let results = stdout_json(
        &run_cli(
            &config_path,
            &[
                "search",
                "query",
                &space_path,
                "--form",
                "Task",
                "--gte",
                "priority=5",
            ],
        ),
        "search query gte",
    );
    assert_eq!(result_ids(&results), vec!["task-closed".to_string()]);

    let results = stdout_json(
        &run_cli(
            &config_path,
            &[
                "search",
                "query",
                &space_path,
                "--form",
                "Task",
                "--contains",
                "status=los",
            ],
        ),
        "search query contains",
    );
    assert_eq!(result_ids(&results), vec!["task-closed".to_string()]);
}

#[test]
fn search_query_criteria_file_matches_flag_input() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = setup_task_space(&config_path, &root, "search-query-file");

    let criteria_path = dir.path().join("criteria.json");
    std::fs::write(
        &criteria_path,
        "{\"form\":\"Task\",\"conditions\":[{\"field\":\"status\",\"operator\":\"equals\",\"value\":\"open\"}],\"limit\":100}",
    )
    .expect("write criteria file");
    let from_file = stdout_json(
        &run_cli(
            &config_path,
            &[
                "search",
                "query",
                &space_path,
                "--criteria-file",
                criteria_path.to_str().unwrap(),
            ],
        ),
        "search query criteria file",
    );
    let from_flags = stdout_json(
        &run_cli(
            &config_path,
            &[
                "search",
                "query",
                &space_path,
                "--form",
                "Task",
                "--eq",
                "status=open",
            ],
        ),
        "search query flags",
    );
    assert_eq!(result_ids(&from_file), result_ids(&from_flags));
    assert_eq!(result_ids(&from_file), vec!["task-open".to_string()]);
}

#[test]
fn search_query_rejects_mixed_and_malformed_input_with_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = setup_task_space(&config_path, &root, "search-query-usage");

    // criteria-file mixed with condition flags is ambiguous.
    let criteria_path = dir.path().join("criteria.json");
    std::fs::write(&criteria_path, "{\"form\":\"Task\",\"conditions\":[]}")
        .expect("write criteria file");
    let output = run_cli(
        &config_path,
        &[
            "search",
            "query",
            &space_path,
            "--criteria-file",
            criteria_path.to_str().unwrap(),
            "--eq",
            "status=open",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("INVALID_INPUT"),
        "mixed input must report INVALID_INPUT: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Malformed FIELD=VALUE is a usage error before any search runs.
    let output = run_cli(
        &config_path,
        &[
            "search",
            "query",
            &space_path,
            "--form",
            "Task",
            "--eq",
            "status",
        ],
    );
    assert_eq!(output.status.code(), Some(2));

    // Missing --form without a criteria file is a usage error.
    let output = run_cli(
        &config_path,
        &["search", "query", &space_path, "--eq", "status=open"],
    );
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn search_query_invalid_criteria_keep_stable_machine_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = setup_task_space(&config_path, &root, "search-query-errors");

    // Unknown form.
    let output = run_cli(
        &config_path,
        &[
            "search",
            "query",
            &space_path,
            "--form",
            "Missing",
            "--eq",
            "status=open",
        ],
    );
    assert_eq!(output.status.code(), Some(4));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("FORM_NOT_FOUND"),
        "unknown form must report FORM_NOT_FOUND: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Unknown field.
    let output = run_cli(
        &config_path,
        &[
            "search",
            "query",
            &space_path,
            "--form",
            "Task",
            "--eq",
            "missing=open",
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("UNKNOWN_FORM_FIELDS"),
        "unknown field must report UNKNOWN_FORM_FIELDS: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Invalid typed value.
    let output = run_cli(
        &config_path,
        &[
            "search",
            "query",
            &space_path,
            "--form",
            "Task",
            "--eq",
            "priority=high",
        ],
    );
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("INVALID_INPUT"),
        "invalid value must report INVALID_INPUT: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Remote transport sends the identical criteria DTO through search.query.
#[test]
fn search_query_remote_sends_identical_criteria_dto() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
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
                    tx.send(String::from_utf8_lossy(&request).into_owned())
                        .expect("send request");
                    let body = "[]";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write response");
                    return;
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

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
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
        .expect("config set");
    assert!(set_output.status.success());

    let output = run_cli(
        &config_path,
        &[
            "search",
            "query",
            "remote-space",
            "--form",
            "Task",
            "--eq",
            "status=open",
            "--gte",
            "priority=3",
        ],
    );
    handle.join().unwrap();
    let request = rx.recv().expect("captured request");
    assert!(
        output.status.success(),
        "remote search query failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        request.starts_with("POST /spaces/remote-space/query HTTP/1.1"),
        "{request}"
    );
    let body_start = request.find("\r\n\r\n").expect("request body") + 4;
    let body: serde_json::Value =
        serde_json::from_str(&request[body_start..]).expect("request body JSON");
    assert_eq!(
        body["criteria"],
        serde_json::json!({
            "form": "Task",
            "conditions": [
                {"field": "status", "operator": "equals", "value": "open"},
                {"field": "priority", "operator": "gte", "value": "3"},
            ],
        }),
        "remote transport must send the identical criteria DTO"
    );
    // The canned backend response round-trips unchanged through the CLI.
    let stdout = stdout_json(&output, "remote search query");
    assert_eq!(stdout, serde_json::json!([]));
}
