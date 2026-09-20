//! Integration tests for the pin lifecycle CLI surface.
//!
//! Pins are read-only Knowledge snapshots: read/delete never mutate Entry
//! history, delete removes only the pin identity, and pins never span
//! spaces.

use std::path::PathBuf;
use std::process::Command;
use ugoite_cli::cli_config::{ConfigFile, ConnectionConfig, ContextConfig};

fn write_remote_config(path: &std::path::Path, endpoint: &str, space_uid: &str) {
    let mut config = ConfigFile::empty();
    config.connections.insert(
        "remote".to_string(),
        ConnectionConfig::Backend {
            url: endpoint.to_string(),
        },
    );
    config.contexts.insert(
        "pin-test".to_string(),
        ContextConfig {
            connection: "remote".to_string(),
            space_uid: space_uid.parse().expect("valid Space UID"),
            credential: None,
        },
    );
    config.current_context = Some("pin-test".to_string());
    std::fs::write(
        path,
        toml::to_string_pretty(&config).expect("serialize config"),
    )
    .expect("write canonical config");
}

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

fn run_cli(config_path: &std::path::Path, args: &[&str]) -> std::process::Output {
    let bin = ugoite_bin();
    if !config_path.exists() {
        let initialized = Command::new(&bin)
            .args(["--config", config_path.to_str().unwrap(), "config", "init"])
            .output()
            .expect("initialize canonical config");
        assert!(initialized.status.success(), "config init failed");
        let configured = Command::new(&bin)
            .args([
                "--config",
                config_path.to_str().unwrap(),
                "config",
                "connection",
                "set",
                "local",
                "--type",
                "core",
                "--root",
                config_path.parent().unwrap().to_str().unwrap(),
            ])
            .output()
            .expect("configure canonical connection");
        assert!(configured.status.success(), "connection set failed");
    }
    let mut canonical = vec![
        "--config".to_string(),
        config_path.to_string_lossy().into_owned(),
    ];
    let configured_space_uid = std::fs::read_to_string(config_path).ok().and_then(|text| {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix("space_uid = \"")
                .and_then(|value| value.strip_suffix('"'))
                .map(str::to_owned)
        })
    });
    let mut index = 0;
    while index < args.len() {
        match args[index] {
            "create-space" => canonical.extend(["space".into(), "create".into()]),
            "--root" => index += 1,
            arg if arg.contains("/spaces/") => {}
            arg if Some(arg) == configured_space_uid.as_deref() => {}
            arg => canonical.push(arg.to_string()),
        }
        index += 1;
    }
    Command::new(bin).args(canonical).output().expect("run CLI")
}

fn json_of(output: &std::process::Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).expect("stdout JSON")
}

fn history_len(config_path: &std::path::Path, space_path: &str, entry_id: &str) -> usize {
    let history = run_cli(
        config_path,
        &["entry", "history", space_path, entry_id, "-o", "json"],
    );
    assert!(
        history.status.success(),
        "history stderr: {}",
        String::from_utf8_lossy(&history.stderr)
    );
    json_of(&history)["revisions"]
        .as_array()
        .expect("revisions")
        .len()
}

/// Core mode: create/list/read/diff/delete share snapshot semantics and
/// read/delete leave Entry history untouched.
#[test]
fn test_pin_lifecycle_shares_core_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/pin-space");

    assert!(run_cli(
        &config_path,
        &["create-space", "--root", &root, "pin-space"]
    )
    .status
    .success());
    let form_file = dir.path().join("entry-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"}}}"#,
    )
    .unwrap();
    assert!(run_cli(
        &config_path,
        &["form", "update", &space_path, form_file.to_str().unwrap()]
    )
    .status
    .success());

    let create = run_cli(
        &config_path,
        &[
            "entry",
            "create",
            "--content",
            "---\nform: Entry\n---\n# One\n\n## Body\n\nfirst",
            &space_path,
            "note-1",
        ],
    );
    assert!(
        create.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );

    assert!(run_cli(&config_path, &["pin", "create", &space_path, "v1"])
        .status
        .success());
    let create_two = run_cli(
        &config_path,
        &[
            "entry",
            "create",
            "--content",
            "---\nform: Entry\n---\n# Two\n\n## Body\n\nsecond",
            &space_path,
            "note-2",
        ],
    );
    assert!(create_two.status.success());
    assert!(run_cli(&config_path, &["pin", "create", &space_path, "v2"])
        .status
        .success());

    let list = run_cli(&config_path, &["pin", "list", &space_path, "-o", "json"]);
    assert!(list.status.success());
    let pins = json_of(&list);
    assert!(pins.get("v1").is_some());
    assert!(pins.get("v2").is_some());

    let before = history_len(&config_path, &space_path, "note-1");
    let read = run_cli(
        &config_path,
        &["pin", "read", &space_path, "v1", "-o", "json"],
    );
    assert!(
        read.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    let read_json = json_of(&read);
    assert_eq!(read_json["name"], "v1");

    let diff = run_cli(
        &config_path,
        &[
            "pin",
            "diff",
            &space_path,
            "--from",
            "v1",
            "--to",
            "v2",
            "-o",
            "json",
        ],
    );
    assert!(
        diff.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&diff.stderr)
    );
    let diff_json = json_of(&diff);
    let diff_text = serde_json::to_string(&diff_json).unwrap();
    assert!(diff_text.contains("note-2"), "{diff_text}");

    let delete = run_cli(&config_path, &["pin", "delete", &space_path, "v1"]);
    assert!(
        delete.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&delete.stderr)
    );
    assert_eq!(history_len(&config_path, &space_path, "note-1"), before);

    let list_after = run_cli(&config_path, &["pin", "list", &space_path, "-o", "json"]);
    assert!(json_of(&list_after).get("v1").is_none());
    assert!(json_of(&list_after).get("v2").is_some());

    // Unknown pins and foreign spaces fail closed without guessing.
    let missing = run_cli(
        &config_path,
        &["pin", "read", &space_path, "missing", "-o", "json"],
    );
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("CHECKPOINT_UNAVAILABLE"),
        "stderr: {}",
        String::from_utf8_lossy(&missing.stderr)
    );
    assert!(run_cli(
        &config_path,
        &["create-space", "--root", &root, "other-space"]
    )
    .status
    .success());
    let other_path = format!("{root}/spaces/other-space");
    let foreign = run_cli(
        &config_path,
        &["pin", "read", &other_path, "v2", "-o", "json"],
    );
    assert!(!foreign.status.success());
    assert!(
        String::from_utf8_lossy(&foreign.stderr).contains("CHECKPOINT_UNAVAILABLE"),
        "stderr: {}",
        String::from_utf8_lossy(&foreign.stderr)
    );
}

/// Backend mode reaches the same snapshot surface through shared operations.
#[test]
fn test_pin_lifecycle_backend_uses_shared_operations() {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let space_uid = uuid::Uuid::now_v7().to_string();
    let served_uid = space_uid.clone();
    let pin_list_body = serde_json::json!({
        "v1": {"coordinate": {"kind": "pin", "name": "v1"}, "created_at_micros": 1, "created_by_principal_id": "tester"},
        "v2": {"coordinate": {"kind": "pin", "name": "v2"}, "created_at_micros": 2, "created_by_principal_id": "tester"},
    })
    .to_string();
    let handle = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        // list, read (served by list), diff, create, missing-read (list).
        let mut served = 0;
        while served < 5 {
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
            let (status, body) = if head.starts_with(&format!("GET /spaces/{served_uid}/pins/diff"))
            {
                assert!(head.contains("from=v1"), "{head}");
                assert!(head.contains("to=v2"), "{head}");
                (
                    "200 OK",
                    r#"{"changes":[{"entry_id":"note-2"}]}"#.to_string(),
                )
            } else if head.starts_with(&format!("POST /spaces/{served_uid}/pins ")) {
                ("200 OK", r#"{"name":"v3"}"#.to_string())
            } else if head.starts_with(&format!("DELETE /spaces/{served_uid}/pins/v1 ")) {
                ("200 OK", r#"{"name":"v1","status":"deleted"}"#.to_string())
            } else {
                assert!(
                    head.starts_with(&format!("GET /spaces/{served_uid}/pins ")),
                    "{head}"
                );
                ("200 OK", pin_list_body.clone())
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            served += 1;
        }
    });

    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    write_remote_config(&config_path, &endpoint, &space_uid);

    let list = run_cli(&config_path, &["pin", "list", &space_uid, "-o", "json"]);
    assert!(
        list.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let pins = json_of(&list);
    assert!(pins.get("v1").is_some());

    // Read resolves from the shared listing: no second request is needed and
    // the target revision is surfaced without touching current state.
    let read = run_cli(
        &config_path,
        &["pin", "read", &space_uid, "v2", "-o", "json"],
    );
    assert!(
        read.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&read.stderr)
    );
    assert_eq!(json_of(&read)["name"], "v2");

    let diff = run_cli(
        &config_path,
        &[
            "pin", "diff", &space_uid, "--from", "v1", "--to", "v2", "-o", "json",
        ],
    );
    assert!(
        diff.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&diff.stderr)
    );
    assert_eq!(json_of(&diff)["changes"][0]["entry_id"], "note-2");

    let create = run_cli(&config_path, &["pin", "create", &space_uid, "v3"]);
    assert!(
        create.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create.stderr)
    );

    let missing = run_cli(
        &config_path,
        &["pin", "read", &space_uid, "missing", "-o", "json"],
    );
    assert!(!missing.status.success());
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("CHECKPOINT_UNAVAILABLE"),
        "stderr: {}",
        String::from_utf8_lossy(&missing.stderr)
    );
    handle.join().unwrap();

    // Delete is served by its own listener to prove the exact route.
    let delete_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let delete_endpoint = format!("http://{}", delete_listener.local_addr().unwrap());
    let delete_uid = space_uid.clone();
    let delete_handle = std::thread::spawn(move || {
        let (mut stream, _) = delete_listener.accept().expect("delete request");
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
        let head = String::from_utf8_lossy(&request).into_owned();
        assert!(
            head.starts_with(&format!("DELETE /spaces/{delete_uid}/pins/v1 ")),
            "{head}"
        );
        let body = r#"{"name":"v1","status":"deleted"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    write_remote_config(&config_path, &delete_endpoint, &space_uid);
    let delete = run_cli(&config_path, &["pin", "delete", &space_uid, "v1"]);
    assert!(
        delete.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&delete.stderr)
    );
    delete_handle.join().unwrap();
}
