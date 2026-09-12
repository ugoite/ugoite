//! Change history, revert, and Run undo through the CLI.
//!
//! Evidence identity: surface=cli, transports=core/local and remote.
//! Revert and undo append new Changes; the reverted Change is never
//! removed. CLI stdout wording is never compared; only exit status,
//! machine error codes, and canonical reads/history matter.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
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

fn change_ids(changes: &serde_json::Value) -> Vec<String> {
    changes
        .as_array()
        .unwrap_or_else(|| panic!("change list must be an array: {changes}"))
        .iter()
        .map(|change| {
            change
                .get("change_id")
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("change has no change_id: {change}"))
                .to_string()
        })
        .collect()
}

fn setup_entry_space(
    config_path: &std::path::Path,
    root: &str,
    space_id: &str,
) -> (String, String) {
    let space_path = format!("{root}/spaces/{space_id}");
    let output = run_cli(config_path, &["create-space", "--root", root, space_id]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form_file = format!("{root}/recovery-form.json");
    std::fs::write(
        &form_file,
        "{\"name\":\"Note\",\"version\":1,\"template\":\"# Note\\n\\n## Body\\n\",\"fields\":{\"Body\":{\"type\":\"string\",\"required\":true}}}",
    )
    .expect("write recovery form");
    let output = run_cli(config_path, &["form", "update", &space_path, &form_file]);
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created = stdout_json(
        &run_cli(
            config_path,
            &[
                "entry",
                "create",
                "--content",
                "---\nform: Note\n---\n# Recovery\n\n## Body\nrecover me\n",
                &space_path,
                "recovery-entry",
            ],
        ),
        "entry create",
    );
    let change_id = created
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("create returns durable change_id")
        .to_string();
    (space_path, change_id)
}

#[test]
fn change_revert_appends_without_removing_history() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, change_id) = setup_entry_space(&config_path, &root, "recovery-core");

    let before = stdout_json(
        &run_cli(&config_path, &["change", "list", &space_path]),
        "change list before revert",
    );
    assert!(change_ids(&before).contains(&change_id));

    let reverted = stdout_json(
        &run_cli(&config_path, &["change", "revert", &space_path, &change_id]),
        "change revert",
    );
    let revert_id = reverted
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("revert returns the appended change_id")
        .to_string();
    assert_ne!(revert_id, change_id);
    assert_eq!(
        reverted.get("reverts_change_id").and_then(|id| id.as_str()),
        Some(change_id.as_str())
    );

    // History grows append-only: the reverted Change is still listed.
    let after = stdout_json(
        &run_cli(&config_path, &["change", "list", &space_path]),
        "change list after revert",
    );
    let ids = change_ids(&after);
    assert!(ids.contains(&change_id), "reverted Change must be kept");
    assert!(ids.contains(&revert_id), "revert Change must be appended");
    assert_eq!(ids.len(), change_ids(&before).len() + 1);
}

#[test]
fn change_recovery_errors_stay_stable_machine_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let (space_path, _) = setup_entry_space(&config_path, &root, "recovery-errors");

    // Unknown Change: not-found classification.
    let output = run_cli(
        &config_path,
        &["change", "revert", &space_path, "change-missing"],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("REVISION_NOT_FOUND"),
        "unknown change must report REVISION_NOT_FOUND: {stderr}"
    );

    // Blank IDs are usage errors.
    let output = run_cli(&config_path, &["change", "revert", &space_path, " "]);
    assert_eq!(output.status.code(), Some(2));
    let output = run_cli(&config_path, &["run", "undo", &space_path, " "]);
    assert_eq!(output.status.code(), Some(2));

    // Unknown Run: eligible-for-nothing, reported as an empty undo outcome.
    let undone = stdout_json(
        &run_cli(&config_path, &["run", "undo", &space_path, "run-missing"]),
        "run undo unknown run",
    );
    assert_eq!(
        undone
            .get("reverted_change_count")
            .and_then(|count| count.as_u64()),
        Some(0)
    );
}

/// Remote transport uses the same operation meaning over change/revert/undo routes.
#[test]
fn change_recovery_remote_uses_canonical_routes() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let base_url = format!("http://{}", listener.local_addr().expect("addr"));
    let bodies: std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let bodies_for_thread = bodies.clone();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        for _ in 0..3 {
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(accepted) => break accepted,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "timed out waiting");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept: {error}"),
                }
            };
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
                    if let Some(pos) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                        header_end = Some(pos + 4);
                        for line in String::from_utf8_lossy(&request[..pos]).lines().skip(1) {
                            if let Some(value) = line
                                .strip_prefix("content-length:")
                                .or_else(|| line.strip_prefix("Content-Length:"))
                            {
                                content_length = value.trim().parse().expect("content length");
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
            let text = String::from_utf8_lossy(&request).into_owned();
            let head_end = text.find("\r\n\r\n").expect("request head") + 4;
            let head = text[..head_end].to_string();
            let body = text[head_end..].to_string();
            let response_body = if head.starts_with("GET ") {
                "[]"
            } else if head.contains("/revert") {
                "{\"change_id\":\"change-new\",\"reverts_change_id\":\"change-1\"}"
            } else {
                "{\"run_id\":\"run-1\",\"reverted_change_count\":1,\"inverses\":[]}"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
            bodies_for_thread
                .lock()
                .expect("bodies lock")
                .push((head, body));
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

    let output = run_cli(&config_path, &["change", "list", "remote-space"]);
    assert!(
        output.status.success(),
        "remote change list: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = run_cli(
        &config_path,
        &["change", "revert", "remote-space", "change-1"],
    );
    assert!(
        output.status.success(),
        "remote change revert: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = run_cli(&config_path, &["run", "undo", "remote-space", "run-1"]);
    assert!(
        output.status.success(),
        "remote run undo: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The recording server serves exactly three requests, so joining also
    // proves all three CLI invocations reached the backend.
    handle.join().unwrap();

    let captured = bodies.lock().expect("bodies lock");
    assert_eq!(captured.len(), 3);
    assert!(
        captured[0]
            .0
            .starts_with("GET /spaces/remote-space/changes "),
        "change list route: {}",
        captured[0].0
    );
    assert!(
        captured[1]
            .0
            .starts_with("POST /spaces/remote-space/changes/change-1/revert "),
        "change revert route: {}",
        captured[1].0
    );
    assert!(
        captured[2]
            .0
            .starts_with("POST /spaces/remote-space/runs/run-1/undo "),
        "run undo route: {}",
        captured[2].0
    );
}
