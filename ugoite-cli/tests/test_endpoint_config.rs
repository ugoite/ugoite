//! Integration tests for the endpoint configuration system.
//! REQ-STO-001, REQ-STO-004, REQ-SEC-011

use std::{fs, process::Command};

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
}

/// REQ-STO-001: Config roundtrip persists to XDG home directory.
#[test]
fn test_endpoint_config_roundtrip_uses_home_directory() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let set_output = Command::new(ugoite_bin())
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://localhost:8080",
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
    assert_eq!(v["backend_url"].as_str(), Some("http://localhost:8080"));
}

/// REQ-STO-001: Base URL resolves according to configured mode.
#[test]
fn test_resolve_base_url_by_mode() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    // Default mode is "core" - should not require backend URL
    let output = Command::new(ugoite_bin())
        .args(["config", "show"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
    assert_eq!(v["mode"].as_str(), Some("core"));
}

/// REQ-STO-004: Space ID can be parsed from both path and explicit ID.
#[test]
fn test_parse_space_id_from_path_and_id() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/path-id-space");

    // Create a space - this tests that space_id can be derived from path components
    let output = Command::new(ugoite_bin())
        .args(["space", "create", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // List spaces - verify the space shows up with the expected ID
    let list_output = Command::new(ugoite_bin())
        .args(["space", "list", &root])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(list_output.status.success());
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(stdout.contains("path-id-space"));
}

/// REQ-SEC-011: config set must reject non-loopback cleartext backend and API URLs.
#[test]
fn test_config_set_req_sec_011_rejects_non_loopback_cleartext_urls() {
    let dir = tempfile::tempdir().unwrap();

    for (name, args, label) in [
        (
            "backend",
            vec![
                "config",
                "set",
                "--mode",
                "backend",
                "--backend-url",
                "http://example.com",
            ],
            "Backend endpoint URL http://example.com uses cleartext http:// for a non-loopback host",
        ),
        (
            "api",
            vec![
                "config",
                "set",
                "--mode",
                "api",
                "--api-url",
                "http://example.com/api",
            ],
            "API endpoint URL http://example.com/api uses cleartext http:// for a non-loopback host",
        ),
        (
            "backend-lookalike",
            vec![
                "config",
                "set",
                "--mode",
                "backend",
                "--backend-url",
                "http://localhost.example.com:8000",
            ],
            "Backend endpoint URL http://localhost.example.com:8000 uses cleartext http:// for a non-loopback host",
        ),
    ] {
        let config_path = dir.path().join(format!("{name}.json"));
        let output = Command::new(ugoite_bin())
            .args(&args)
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .output()
            .expect("failed to execute");
        assert!(
            !output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(label), "stderr: {stderr}");
        assert!(
            stderr.contains("https:// for remote endpoints"),
            "stderr: {stderr}"
        );
        assert!(
            !config_path.exists(),
            "config should not be written for rejected insecure endpoints",
        );
    }
}

/// REQ-SEC-011: loopback cleartext endpoints remain available for local development.
#[test]
fn test_config_set_req_sec_011_allows_loopback_cleartext_urls() {
    let dir = tempfile::tempdir().unwrap();
    for (name, url) in [
        ("ipv4", "http://127.0.0.1:8080"),
        ("localhost-dot", "http://localhost.:8081"),
        ("ipv6", "http://[::1]:8082"),
    ] {
        let config_path = dir.path().join(format!("{name}.json"));

        let set_output = Command::new(ugoite_bin())
            .args(["config", "set", "--mode", "backend", "--backend-url", url])
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .output()
            .expect("failed to execute");
        assert!(
            set_output.status.success(),
            "url: {url}\nstderr: {}",
            String::from_utf8_lossy(&set_output.stderr)
        );

        let show_output = Command::new(ugoite_bin())
            .args(["config", "show"])
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .output()
            .expect("failed to execute");
        assert!(show_output.status.success());

        let stdout = String::from_utf8_lossy(&show_output.stdout);
        let v: serde_json::Value = serde_json::from_str(&stdout).expect("JSON");
        assert_eq!(v["backend_url"].as_str(), Some(url), "url: {url}");
    }
}

/// REQ-SEC-011: config current must warn about previously saved insecure remote endpoints.
#[test]
fn test_config_current_req_sec_011_warns_about_saved_non_loopback_cleartext_url() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    fs::write(
        &config_path,
        r#"{
  "mode": "api",
  "backend_url": "http://localhost:8000",
  "api_url": "http://example.com/api"
}
"#,
    )
    .expect("write insecure config");

    let output = Command::new(ugoite_bin())
        .args(["config", "current"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Current endpoint mode: api"), "stdout: {stdout}");
    assert!(
        stdout.contains(
            "Warning: API endpoint URL http://example.com/api uses cleartext http:// for a non-loopback host"
        ),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("Server-backed commands will refuse this endpoint"),
        "stdout: {stdout}"
    );
}

/// REQ-SEC-011: remote commands must refuse insecure saved endpoints before any request is sent.
#[test]
fn test_space_list_req_sec_011_rejects_saved_non_loopback_cleartext_endpoint() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    fs::write(
        &config_path,
        r#"{
  "mode": "backend",
  "backend_url": "http://example.com",
  "api_url": "http://localhost:3000/api"
}
"#,
    )
    .expect("write insecure config");

    let output = Command::new(ugoite_bin())
        .args(["space", "list"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Backend endpoint URL http://example.com uses cleartext http:// for a non-loopback host"),
        "stderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "Use https:// for remote endpoints, or use a loopback http:// URL for local development."
        ),
        "stderr: {stderr}"
    );
}
