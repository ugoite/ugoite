//! Space audit events through the CLI.
//!
//! Evidence identity: surface=cli, transports=core/local and remote.
//! Audit output is allow-listed identity evidence only (event/change/
//! revision/actor fields); Entry bodies, paths, and secrets never appear.
//! CLI stdout wording is never compared; only exit status, machine error
//! codes, and canonical JSON matter.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Output;
use std::thread;
use std::time::{Duration, Instant};
use support::Command;

mod support;

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

fn setup_audited_space(config_path: &std::path::Path, root: &str, space_id: &str) -> String {
    let space_path = format!("{root}/spaces/{space_id}");
    let output = run_cli(config_path, &["create-space", "--root", root, space_id]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form_file = format!("{root}/audit-form.json");
    std::fs::write(
        &form_file,
        "{\"name\":\"Note\",\"version\":1,\"template\":\"# Note\\n\\n## Body\\n\",\"fields\":{\"Body\":{\"type\":\"string\",\"required\":true}}}",
    )
    .expect("write audit form");
    let output = run_cli(config_path, &["form", "update", &space_path, &form_file]);
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = run_cli(
        config_path,
        &[
            "entry",
            "create",
            "--content",
            "---\nform: Note\n---\n# Audited\n\n## Body\naudit me\n",
            &space_path,
            "audited-entry",
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
fn space_audit_events_lists_allowlisted_evidence_core() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = setup_audited_space(&config_path, &root, "audit-core");

    let listed = stdout_json(
        &run_cli(&config_path, &["space", "audit-events", &space_path]),
        "space audit-events",
    );
    let total = listed
        .get("total")
        .and_then(|total| total.as_u64())
        .expect("audit listing reports total");
    assert!(total >= 1, "entry create must leave audit evidence");
    let items = listed
        .get("items")
        .and_then(|items| items.as_array())
        .expect("audit listing carries items");
    assert!(!items.is_empty());
    for item in items {
        for field in ["event_id", "action", "target_id"] {
            assert!(
                item.get(field).and_then(|value| value.as_str()).is_some(),
                "audit item carries {field}: {item}"
            );
        }
        assert!(
            item.get("metadata")
                .and_then(|meta| meta.get("revision_id"))
                .and_then(|value| value.as_str())
                .is_some(),
            "audit item carries metadata.revision_id: {item}"
        );
    }
    // Allow-list: no Entry body, storage paths, or key material in evidence.
    let raw = serde_json::to_string(&listed).expect("serializes");
    for forbidden in ["audit me", "## Body", "meta.json", "hmac", "token"] {
        assert!(
            !raw.contains(forbidden),
            "audit output must not contain {forbidden}"
        );
    }

    // Paging flags round-trip through the shared op semantics.
    let paged = stdout_json(
        &run_cli(
            &config_path,
            &[
                "space",
                "audit-events",
                &space_path,
                "--offset",
                "1",
                "--limit",
                "1",
            ],
        ),
        "space audit-events paging",
    );
    assert_eq!(
        paged.get("total").and_then(|total| total.as_u64()),
        Some(total)
    );
    assert_eq!(
        paged.get("offset").and_then(|offset| offset.as_u64()),
        Some(1)
    );
    assert_eq!(paged.get("limit").and_then(|limit| limit.as_u64()), Some(1));
    assert!(paged
        .get("items")
        .and_then(|items| items.as_array())
        .is_some_and(|items| items.len() <= 1));
}

/// Audit paging stays in the effective 1..=500 range: `limit=0` normalizes
/// to 1 (never a validation error) and huge values clamp to 500 without
/// overflowing before the cap.
#[test]
fn space_audit_events_pagination_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = setup_audited_space(&config_path, &root, "audit-paging");

    let zeroed = stdout_json(
        &run_cli(
            &config_path,
            &["space", "audit-events", &space_path, "--limit", "0"],
        ),
        "space audit-events limit=0",
    );
    assert_eq!(
        zeroed.get("limit").and_then(|limit| limit.as_u64()),
        Some(1)
    );

    let huge = stdout_json(
        &run_cli(
            &config_path,
            &[
                "space",
                "audit-events",
                &space_path,
                "--limit",
                "18446744073709551615",
            ],
        ),
        "space audit-events huge limit",
    );
    assert_eq!(
        huge.get("limit").and_then(|limit| limit.as_u64()),
        Some(500)
    );
}

/// Remote transport uses the shared space.audit operation route.
#[test]
fn space_audit_events_remote_uses_canonical_route() {
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
        let response_body = "{\"items\":[{\"event_id\":\"event-1\",\"action\":\"entry.created\",\"subject_principal_id\":\"actor-1\",\"target_type\":\"entry\",\"target_id\":\"entry-1\",\"outcome\":\"success\",\"metadata\":{\"revision_id\":\"rev-1\",\"change_id\":\"change-1\"}}],\"total\":1,\"offset\":0,\"limit\":50}";
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

    let remote_space_uid = uuid::Uuid::now_v7().to_string();
    let listed = stdout_json(
        &run_cli(&config_path, &["space", "audit-events", &remote_space_uid]),
        "remote space audit-events",
    );
    assert_eq!(
        listed.get("total").and_then(|total| total.as_u64()),
        Some(1)
    );
    handle.join().unwrap();

    let captured = bodies.lock().expect("bodies lock");
    assert_eq!(captured.len(), 1);
    assert!(
        captured[0].0.starts_with(&format!(
            "GET /spaces/{remote_space_uid}/audit?offset=0&limit=50 "
        )),
        "space audit route: {}",
        captured[0].0
    );
}
