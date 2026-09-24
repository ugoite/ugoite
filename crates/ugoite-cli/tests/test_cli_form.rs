//! Integration tests for CLI form commands.
//! REQ-FORM-001, REQ-FORM-002

use std::process::Command;

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

/// REQ-FORM-001: CLI lists available form column types.
#[test]
fn test_cli_list_types() {
    // list-types does not require a space path
    let output = Command::new(ugoite_bin())
        .args(["form", "list-types"])
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should list some column types
    assert!(!stdout.trim().is_empty());
}

/// REQ-FORM-002: CLI form save writes the canonical Form definition.
#[test]
fn test_cli_form_save() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let init = Command::new(ugoite_bin())
        .args(["--config", config_path.to_str().unwrap(), "config", "init"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("config init");
    assert!(init.status.success());
    let connection = Command::new(ugoite_bin())
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
            &root,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("connection set");
    assert!(
        connection.status.success(),
        "connection set failed: {} {}",
        String::from_utf8_lossy(&connection.stdout),
        String::from_utf8_lossy(&connection.stderr)
    );
    let create = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "space",
            "create",
            "form-space",
            "--connection",
            "local",
        ])
        .output()
        .expect("space create");
    assert!(
        create.status.success(),
        "space create failed: {} {}",
        String::from_utf8_lossy(&create.stdout),
        String::from_utf8_lossy(&create.stderr)
    );
    // Create the form via form save
    let form_file = dir.path().join("entry-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"}}}"#,
    )
    .unwrap();

    let update_output = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "form",
            "save",
            form_file.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        update_output.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&update_output.stderr)
    );

    // Get the form that was just created
    let get_output = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "form",
            "get",
            "Entry",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        get_output.status.success(),
        "get stderr: {}",
        String::from_utf8_lossy(&get_output.stderr)
    );
}
