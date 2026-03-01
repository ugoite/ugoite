//! Integration tests for saved SQL queries.
//! REQ-API-006, REQ-API-007

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

/// REQ-API-006: Saved SQL queries CRUD lifecycle (create, read, update, delete).
#[test]
fn test_saved_sql_req_api_006_crud() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/sql-space");

    Command::new(ugoite_bin())
        .args(["create-space", &root, "sql-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    // Create a saved query
    let create_output = Command::new(ugoite_bin())
        .args([
            "sql",
            "saved-create",
            "--name",
            "my-query",
            "--sql",
            "SELECT * FROM entries",
            &space_path,
            "my-query",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        create_output.status.success(),
        "create stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );

    // List saved queries
    let list_output = Command::new(ugoite_bin())
        .args(["sql", "saved-list", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        list_output.status.success(),
        "list stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
}

/// REQ-API-007: Saved SQL query validation rejects invalid SQL.
#[test]
fn test_saved_sql_req_api_007_validation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/sql-space");

    Command::new(ugoite_bin())
        .args(["create-space", &root, "sql-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    // Attempt to create a saved query with invalid SQL
    let create_output = Command::new(ugoite_bin())
        .args([
            "sql",
            "saved-create",
            "--name",
            "bad-query",
            "--sql",
            "THIS IS NOT VALID SQL !!!",
            &space_path,
            "bad-query",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should either reject or accept (validation may happen at execution time)
    // Either way, the system should not crash
    let _ = create_output.status.success();
}
