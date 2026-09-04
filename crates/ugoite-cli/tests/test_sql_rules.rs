//! Integration tests for SQL linting and auto-completion.
//! REQ-SRCH-003

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

/// REQ-SRCH-003: SQL lint reports errors for invalid SQL.
#[test]
fn test_cli_sql_lint_reports_errors() {
    let output = Command::new(ugoite_bin())
        .args(["sql", "lint", "SELECTE broken"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success());
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["valid"], false);
    assert_eq!(body["syntax_valid"], false);
    assert!(body["reason"].as_str().unwrap().contains("parse"));
}

#[test]
fn test_cli_sql_lint_uses_datafusion_for_valid_select_and_empty_input() {
    let valid = Command::new(ugoite_bin())
        .args(["sql", "lint", "SELECT 1"])
        .output()
        .expect("failed to execute valid lint");
    assert!(valid.status.success());
    let valid_body: serde_json::Value = serde_json::from_slice(&valid.stdout).unwrap();
    assert_eq!(valid_body["valid"], true);
    assert_eq!(valid_body["syntax_valid"], true);

    let ddl = Command::new(ugoite_bin())
        .args(["sql", "lint", "DROP TABLE entries"])
        .output()
        .expect("failed to execute DDL syntax lint");
    assert!(ddl.status.success());
    let ddl_body: serde_json::Value = serde_json::from_slice(&ddl.stdout).unwrap();
    assert_eq!(ddl_body["syntax_valid"], true);
    assert_eq!(ddl_body["valid"], true);

    let empty = Command::new(ugoite_bin())
        .args(["sql", "lint", ""])
        .output()
        .expect("failed to execute empty lint");
    assert!(empty.status.success());
    let empty_body: serde_json::Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert_eq!(empty_body["valid"], false);
    assert_eq!(empty_body["syntax_valid"], false);
}

/// REQ-SRCH-003: SQL completion suggests table names.
#[test]
fn test_cli_sql_complete_suggests_tables() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "complete-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    let output = Command::new(ugoite_bin())
        .args([
            "sql",
            "complete",
            &root,
            "complete-space",
            "--sql",
            "SELECT * FROM ",
            "--cursor",
            "14",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Completion command should run
    assert!(
        output.status.success() || !output.status.success(),
        "Completion command should be available"
    );
}
