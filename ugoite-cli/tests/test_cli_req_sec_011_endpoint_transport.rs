//! CLI transport security coverage for endpoint configuration.
//! REQ-SEC-011

use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().expect("current exe");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
}

fn cli_command(config_path: &Path) -> Command {
    let mut command = Command::new(ugoite_bin());
    command.env("UGOITE_CLI_CONFIG_PATH", config_path);
    command
}

fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn parse_stdout_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).expect("stdout json")
}

fn write_endpoint_config(config_path: &Path, mode: &str, backend_url: &str, api_url: &str) {
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent).expect("create config parent");
    }
    std::fs::write(
        config_path,
        serde_json::json!({
            "mode": mode,
            "backend_url": backend_url,
            "api_url": api_url,
        })
        .to_string(),
    )
    .expect("write endpoint config");
}

#[test]
fn test_cli_req_sec_011_config_set_rejects_non_loopback_cleartext_api_urls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");

    let output = cli_command(&config_path)
        .args([
            "config",
            "set",
            "--mode",
            "api",
            "--api-url",
            "http://api.example.test/api",
        ])
        .output()
        .expect("config set api");

    assert!(
        !output.status.success(),
        "remote cleartext api endpoint should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("API endpoint URL http://api.example.test/api uses cleartext http:// for a non-loopback host"));
    assert!(stderr.contains("https:// for remote endpoints"));
}

#[test]
fn test_cli_req_sec_011_config_set_rejects_non_loopback_cleartext_backend_urls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");

    let output = cli_command(&config_path)
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://backend.example.test",
        ])
        .output()
        .expect("config set backend");

    assert!(
        !output.status.success(),
        "remote cleartext backend endpoint should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(
        "Backend endpoint URL http://backend.example.test uses cleartext http:// for a non-loopback host"
    ));
    assert!(stderr.contains("https:// for remote endpoints"));
}

#[test]
fn test_cli_req_sec_011_config_set_allows_loopback_cleartext_development_endpoints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");

    let output = cli_command(&config_path)
        .args([
            "config",
            "set",
            "--mode",
            "api",
            "--api-url",
            "http://127.0.0.1:9999/api",
        ])
        .output()
        .expect("config set loopback api");
    assert_success(&output, "config set loopback api");
    assert_eq!(
        parse_stdout_json(&output)["config"]["api_url"].as_str(),
        Some("http://127.0.0.1:9999/api")
    );
}

#[test]
fn test_cli_req_sec_011_config_set_allows_ipv6_loopback_cleartext_development_endpoints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");

    let output = cli_command(&config_path)
        .args([
            "config",
            "set",
            "--mode",
            "backend",
            "--backend-url",
            "http://[::1]:9999",
        ])
        .output()
        .expect("config set ipv6 loopback backend");
    assert_success(&output, "config set ipv6 loopback backend");
    assert_eq!(
        parse_stdout_json(&output)["config"]["backend_url"].as_str(),
        Some("http://[::1]:9999")
    );
}

#[test]
fn test_cli_req_sec_011_config_set_rejects_non_loopback_cleartext_ipv6_api_urls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");

    let output = cli_command(&config_path)
        .args([
            "config",
            "set",
            "--mode",
            "api",
            "--api-url",
            "http://[2001:db8::1]:8443/api",
        ])
        .output()
        .expect("config set remote ipv6 api");

    assert!(
        !output.status.success(),
        "remote cleartext ipv6 api endpoint should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(
        "API endpoint URL http://[2001:db8::1]:8443/api uses cleartext http:// for a non-loopback host"
    ));
    assert!(stderr.contains("https:// for remote endpoints"));
}

#[test]
fn test_cli_req_sec_011_config_current_warns_about_legacy_insecure_remote_endpoints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");
    write_endpoint_config(
        &config_path,
        "api",
        "http://localhost:8000",
        "http://api.example.test/api",
    );

    let output = cli_command(&config_path)
        .args(["config", "current"])
        .output()
        .expect("config current");
    assert_success(&output, "config current");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Current endpoint mode: api"));
    assert!(stdout.contains("Warning: API endpoint URL http://api.example.test/api uses cleartext http:// for a non-loopback host"));
    assert!(stdout.contains("Server-backed commands will refuse this endpoint"));
}

#[test]
fn test_cli_req_sec_011_config_current_warns_about_legacy_insecure_backend_endpoints() {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");
    write_endpoint_config(
        &config_path,
        "backend",
        "http://backend.example.test",
        "http://localhost:3000/api",
    );

    let output = cli_command(&config_path)
        .args(["config", "current"])
        .output()
        .expect("config current backend");
    assert_success(&output, "config current backend");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Current endpoint mode: backend"));
    assert!(stdout.contains(
        "Warning: Backend endpoint URL http://backend.example.test uses cleartext http:// for a non-loopback host"
    ));
    assert!(stdout.contains("Server-backed commands will refuse this endpoint"));
}

#[test]
fn test_cli_req_sec_011_server_backed_commands_refuse_legacy_insecure_remote_endpoints_before_requests_are_sent(
) {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("cli-config.json");
    write_endpoint_config(
        &config_path,
        "api",
        "http://localhost:8000",
        "http://api.example.test/api",
    );

    let output = cli_command(&config_path)
        .args(["space", "list"])
        .output()
        .expect("space list");

    assert!(
        !output.status.success(),
        "server-backed command should fail before sending a request"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("API endpoint URL http://api.example.test/api uses cleartext http:// for a non-loopback host"));
    assert!(
        !stderr.contains("dns error"),
        "failure should come from the CLI guard, not network resolution"
    );
}
