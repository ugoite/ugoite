use std::{path::PathBuf, process::Command};
use ugoite_cli::commands::auth::DEFAULT_DEVICE_ACTIONS;

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

#[test]
fn test_help() {
    let output = Command::new(ugoite_bin())
        .arg("--help")
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("ugoite"));
}

#[test]
fn default_device_login_actions_exclude_unapproved_dangerous_actions() {
    assert_eq!(DEFAULT_DEVICE_ACTIONS, "read,create,update");
    assert!(!DEFAULT_DEVICE_ACTIONS
        .split(',')
        .any(|action| action == "delete" || action == "share"));
}

#[test]
fn auth_login_help_exposes_named_mcp_target() {
    let output = Command::new(ugoite_bin())
        .args(["auth", "login", "--help"])
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--for <TARGET>"), "stdout:\n{stdout}");
    assert!(stdout.contains("mcp"), "stdout:\n{stdout}");
}

/// REQ-OPS-018: top-level help must show a task-oriented quick-start path.
#[test]
fn test_help_req_ops_018_shows_task_oriented_quick_start() {
    let output = Command::new(ugoite_bin())
        .arg("--help")
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    for expected in [
        "Quick start (local-first / core mode):",
        "ugoite space list .",
        "ugoite space create /path/to/workspace/spaces/demo",
        "Quick start (backend / API mode):",
        "ugoite config set --mode backend --backend-url http://localhost:8000",
        "ugoite auth login",
        "ugoite space list",
    ] {
        assert!(
            stdout.contains(expected),
            "expected top-level help to include {expected:?}\nstdout:\n{stdout}",
        );
    }
}

/// REQ-OPS-018: top-level CLI version flags must report the installed version.
#[test]
fn test_version_req_ops_018_reports_installed_version() {
    let output = Command::new(ugoite_bin())
        .arg("--version")
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "expected version output to include the crate version, got: {stdout}"
    );
}

#[test]
fn test_config_show() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(ugoite_bin())
        .arg("config")
        .arg("show")
        .env("UGOITE_CLI_CONFIG_PATH", dir.path().join("config.json"))
        .output()
        .expect("failed to execute process");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    assert_eq!(v.get("mode").and_then(|m| m.as_str()), Some("core"));
}

#[test]
fn test_config_set_and_show() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");

    let output = Command::new(ugoite_bin())
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
    assert!(output.status.success());

    let output = Command::new(ugoite_bin())
        .args(["config", "show"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    assert_eq!(v.get("mode").and_then(|m| m.as_str()), Some("backend"));
    assert_eq!(
        v.get("backend_url").and_then(|m| m.as_str()),
        Some("http://localhost:9000")
    );
}

#[test]
fn test_space_create_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/test-space");

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
    let created: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("create response should be JSON");
    let created_id = created["id"].as_str().expect("immutable Space id");
    assert_eq!(
        uuid::Uuid::parse_str(created_id).unwrap().get_version_num(),
        7
    );
    assert_eq!(created["slug"], "test-space");

    let output = Command::new(ugoite_bin())
        .args(["space", "list", &root])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    assert!(v
        .as_array()
        .is_some_and(|ids| { ids.iter().any(|value| value.as_str() == Some(created_id)) }));
}
