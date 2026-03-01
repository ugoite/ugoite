//! Integration tests for CLI endpoint routing configuration.
//! REQ-STO-001, REQ-STO-004, REQ-SEC-003

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
    let root = dir.path().to_string_lossy().to_string();

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
        .args(["space", "list", &root])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should fail (backend unreachable), confirming routing to backend
    assert!(
        !output.status.success(),
        "Expected failure connecting to unreachable backend"
    );
}

/// REQ-SEC-003: Service account create routes to backend when in backend mode.
#[test]
fn test_space_service_account_create_routes_to_backend() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    let root = dir.path().to_string_lossy().to_string();

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

    // Service account creation should attempt to route to backend
    let output = Command::new(ugoite_bin())
        .args([
            "space",
            "service-account-create",
            &root,
            "my-space",
            "sa-name",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should fail (backend unreachable or not in core mode), confirming routing to backend
    assert!(
        !output.status.success(),
        "Expected failure - service account requires backend mode"
    );
}
