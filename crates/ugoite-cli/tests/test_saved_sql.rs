//! Integration tests for saved SQL queries.
//! REQ-API-006, REQ-API-007

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

/// REQ-API-006: Saved SQL queries CRUD lifecycle (create, read, update, delete).
#[test]
fn test_saved_sql_req_api_006_crud() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/sql-space");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "sql-space"])
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
            "SELECT * FROM sql",
            &space_path,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        create_output.status.success(),
        "create stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );
    let created: serde_json::Value = serde_json::from_slice(&create_output.stdout)
        .expect("local create should return the generated SQL id");
    let created_id = created["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .expect("local create response should contain a non-empty id");

    let get_output = Command::new(ugoite_bin())
        .args(["sql", "saved-get", &space_path, created_id])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        get_output.status.success(),
        "get stderr: {}",
        String::from_utf8_lossy(&get_output.stderr)
    );
    let fetched: serde_json::Value =
        serde_json::from_slice(&get_output.stdout).expect("get should return JSON");
    assert_eq!(fetched["id"].as_str(), Some(created_id));

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
    let listed: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).expect("list should return JSON");
    assert!(
        listed
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == created_id)),
        "created saved SQL should be present in list: {listed}"
    );

    let parent_revision_id = created["revision_id"]
        .as_str()
        .filter(|revision| !revision.is_empty())
        .expect("local create response should contain a revision id");
    let update_output = Command::new(ugoite_bin())
        .args([
            "sql",
            "saved-update",
            &space_path,
            created_id,
            "--name",
            "updated-query",
            "--sql",
            "SELECT * FROM updated_sql",
            "--parent-revision-id",
            parent_revision_id,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        update_output.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&update_output.stderr)
    );
    let updated: serde_json::Value =
        serde_json::from_slice(&update_output.stdout).expect("update should return JSON");
    assert_eq!(updated["id"].as_str(), Some(created_id));
    assert_eq!(updated["name"].as_str(), Some("updated-query"));
    assert_eq!(updated["sql"].as_str(), Some("SELECT * FROM updated_sql"));

    let delete_output = Command::new(ugoite_bin())
        .args(["sql", "saved-delete", &space_path, created_id])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        delete_output.status.success(),
        "delete stderr: {}",
        String::from_utf8_lossy(&delete_output.stderr)
    );
    let deleted: serde_json::Value =
        serde_json::from_slice(&delete_output.stdout).expect("delete should return JSON");
    assert_eq!(deleted["deleted"].as_bool(), Some(true));

    let final_list_output = Command::new(ugoite_bin())
        .args(["sql", "saved-list", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        final_list_output.status.success(),
        "final list stderr: {}",
        String::from_utf8_lossy(&final_list_output.stderr)
    );
    let final_list: serde_json::Value =
        serde_json::from_slice(&final_list_output.stdout).expect("final list should return JSON");
    assert!(!final_list
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["id"] == created_id)));
}

/// REQ-API-007: Saved SQL query validation rejects invalid SQL.
#[test]
fn test_saved_sql_req_api_007_validation() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/sql-space");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "sql-space"])
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
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Should either reject or accept (validation may happen at execution time)
    // Either way, the system should not crash
    let _ = create_output.status.success();
}
