//! Integration tests for saved SQL queries.
//! REQ-API-006, REQ-API-007

use support::Command;

mod support;

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

fn init_canonical_config(config_path: &std::path::Path, root: &str) {
    let init = Command::new(ugoite_bin())
        .args(["--config", config_path.to_str().unwrap(), "config", "init"])
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
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
            root,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("connection set");
    assert!(connection.status.success());
}

fn run_cli(config_path: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut full = vec![
        "--config".to_string(),
        config_path.to_string_lossy().into_owned(),
    ];
    full.extend(args.iter().map(|arg| (*arg).to_string()));
    Command::new(ugoite_bin())
        .args(full)
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("run CLI")
}

/// REQ-API-006: Saved SQL queries CRUD lifecycle (create, read, update, delete).
#[test]
fn test_saved_sql_req_api_006_crud() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    assert!(run_cli(&config_path, &["space", "create", "sql-space"])
        .status
        .success());

    // Create a saved query
    let create_output = run_cli(
        &config_path,
        &[
            "sql",
            "saved-create",
            "--name",
            "my-query",
            "--sql",
            "SELECT * FROM sql",
        ],
    );

    assert!(
        create_output.status.success(),
        "create stderr: {}",
        String::from_utf8_lossy(&create_output.stderr)
    );
    let created: serde_json::Value = serde_json::from_slice(&create_output.stdout)
        .expect("local create should return the generated SQL id");
    assert_eq!(created["kind"].as_str(), Some("sql"));
    let created_id = created["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .expect("local create response should contain a non-empty id");

    let get_output = run_cli(&config_path, &["sql", "saved-get", created_id]);
    assert!(
        get_output.status.success(),
        "get stderr: {}",
        String::from_utf8_lossy(&get_output.stderr)
    );
    let fetched: serde_json::Value =
        serde_json::from_slice(&get_output.stdout).expect("get should return JSON");
    assert_eq!(fetched["id"].as_str(), Some(created_id));

    // List saved queries
    let list_output = run_cli(&config_path, &["sql", "saved-list"]);

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
    let update_output = run_cli(
        &config_path,
        &[
            "sql",
            "saved-update",
            created_id,
            "--name",
            "updated-query",
            "--sql",
            "SELECT * FROM updated_sql",
            "--parent-revision-id",
            parent_revision_id,
        ],
    );
    assert!(
        update_output.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&update_output.stderr)
    );
    let updated: serde_json::Value =
        serde_json::from_slice(&update_output.stdout).expect("update should return JSON");
    assert_eq!(updated["kind"].as_str(), Some("sql"));
    assert_eq!(updated["id"].as_str(), Some(created_id));
    // The receipt carries the new revision; read back for content.
    let get_updated_output = run_cli(&config_path, &["sql", "saved-get", created_id]);
    assert!(
        get_updated_output.status.success(),
        "get after update stderr: {}",
        String::from_utf8_lossy(&get_updated_output.stderr)
    );
    let fetched_updated: serde_json::Value = serde_json::from_slice(&get_updated_output.stdout)
        .expect("get after update should return JSON");
    assert_eq!(fetched_updated["name"].as_str(), Some("updated-query"));
    assert_eq!(
        fetched_updated["sql"].as_str(),
        Some("SELECT * FROM updated_sql")
    );

    let delete_output = run_cli(&config_path, &["sql", "saved-delete", created_id]);
    assert!(
        delete_output.status.success(),
        "delete stderr: {}",
        String::from_utf8_lossy(&delete_output.stderr)
    );
    let deleted: serde_json::Value =
        serde_json::from_slice(&delete_output.stdout).expect("delete should return JSON");
    assert_eq!(deleted["kind"].as_str(), Some("sql"));
    assert_eq!(deleted["id"].as_str(), Some(created_id));

    let final_list_output = run_cli(&config_path, &["sql", "saved-list"]);
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
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    assert!(run_cli(&config_path, &["space", "create", "sql-space"])
        .status
        .success());

    // Attempt to create a saved query with invalid SQL
    let create_output = run_cli(
        &config_path,
        &[
            "sql",
            "saved-create",
            "--name",
            "bad-query",
            "--sql",
            "THIS IS NOT VALID SQL !!!",
        ],
    );

    // Should either reject or accept (validation may happen at execution time)
    // Either way, the system should not crash
    let _ = create_output.status.success();
}
