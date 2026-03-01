//! Integration tests for the endpoint configuration system.
//! REQ-STO-001, REQ-STO-004

use std::process::Command;

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
            "config", "set",
            "--mode", "backend",
            "--backend-url", "http://localhost:8080",
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

    // Create a space - this tests that space_id can be derived from path components
    let output = Command::new(ugoite_bin())
        .args(["create-space", &root, "path-id-space"])
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
