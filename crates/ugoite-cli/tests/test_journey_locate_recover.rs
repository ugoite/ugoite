//! JOURNEY-LOCATE-RECOVER-001 through the local/core CLI.
//!
//! Evidence identity: surface=cli, transport=core/local. This runs the same
//! logical locate-and-recover scenario as the Frontend evidence: Form-backed
//! Entries are discovered by keyword, narrowed by typed structured Search,
//! updated, observed in Space Change history, recovered by Change revert,
//! and re-verified by search and reopen reads. CLI stdout wording is never
//! compared; only exit status and the returned durable state matter.

use std::process::{Command, Output};

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

fn run_cli(config: &std::path::Path, args: &[&str]) -> Output {
    let bin = ugoite_bin();
    if !config.exists() && args.first().copied() != Some("config") {
        let initialized = Command::new(&bin)
            .args(["--config", config.to_str().unwrap(), "config", "init"])
            .output()
            .expect("initialize canonical config");
        assert!(initialized.status.success(), "config init failed");
        let configured = Command::new(&bin)
            .args([
                "--config",
                config.to_str().unwrap(),
                "config",
                "connection",
                "set",
                "local",
                "--type",
                "core",
                "--root",
                config.parent().unwrap().to_str().unwrap(),
            ])
            .output()
            .expect("configure canonical core connection");
        assert!(configured.status.success(), "connection set failed");
    }
    let mut canonical = vec![
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
    ];
    canonical.extend(args.iter().map(|arg| (*arg).to_string()));
    Command::new(bin)
        .args(canonical)
        .output()
        .expect("run ugoite")
}

fn stdout_json(output: &Output, what: &str) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{what} stdout is not JSON: {stdout}"))
}

fn contains_string(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text == needle,
        serde_json::Value::Array(items) => items.iter().any(|item| contains_string(item, needle)),
        serde_json::Value::Object(fields) => {
            fields.values().any(|item| contains_string(item, needle))
        }
        _ => false,
    }
}

fn search_ids(results: &serde_json::Value) -> Vec<String> {
    results
        .as_array()
        .unwrap_or_else(|| panic!("search results must be an array: {results}"))
        .iter()
        .map(|row| {
            row.get("_ugoite_id")
                .or_else(|| row.get("id"))
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("search row has no id: {row}"))
                .to_string()
        })
        .collect()
}

fn change_ids(changes: &serde_json::Value) -> Vec<String> {
    changes
        .as_array()
        .unwrap_or_else(|| panic!("change list must be an array: {changes}"))
        .iter()
        .map(|change| {
            change
                .get("change_id")
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("change has no change_id: {change}"))
                .to_string()
        })
        .collect()
}

fn revision_ids(history: &serde_json::Value) -> Vec<String> {
    history
        .get("revisions")
        .and_then(|revisions| revisions.as_array())
        .unwrap_or_else(|| panic!("history has no revisions array: {history}"))
        .iter()
        .map(|revision| {
            revision
                .get("revision_id")
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("revision has no revision_id: {revision}"))
                .to_string()
        })
        .collect()
}

/// JOURNEY-LOCATE-RECOVER-001 on surface=cli transport=core/local reaches
/// the same durable locate-and-recover outcome as the Frontend evidence.
#[test]
fn test_journey_cli_core_locate_recover_durable_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.toml");
    let space_id = "locate-recover-core";

    // 1. Create the canonical Space.
    let output = run_cli(&config_path, &["space", "create", space_id]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 2. Create multiple Form-backed Entries sharing one Form.
    let form_file = dir.path().join("locate-recover-form.json");
    std::fs::write(
        &form_file,
        "{\"name\":\"Task\",\"version\":1,\"template\":\"# Task\\n\\n## status\\n\\n## priority\\n\",\"fields\":{\"status\":{\"type\":\"string\",\"required\":true},\"priority\":{\"type\":\"integer\",\"required\":false}}}",
    )
    .unwrap();
    let output = run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    for (entry_id, status, priority) in
        [("locate-task-a", "open", 3), ("locate-task-b", "closed", 7)]
    {
        let priority = priority.to_string();
        let status_field = format!("status={status}");
        let priority_field = format!("priority={priority}");
        let output = run_cli(
            &config_path,
            &[
                "entry",
                "create",
                entry_id,
                "--form",
                "Task",
                "--field",
                &status_field,
                "--field",
                &priority_field,
            ],
        );
        assert!(
            output.status.success(),
            "entry create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // 3. Keyword Search discovers the target by title.
    let results = stdout_json(
        &run_cli(&config_path, &["search", "keyword", "locate-task-a"]),
        "keyword search discovers the target",
    );
    assert!(contains_string(&results, "locate-task-a"));

    // 4. Typed structured Search narrows to the intended Entry set.
    let results = stdout_json(
        &run_cli(
            &config_path,
            &["search", "query", "--form", "Task", "--eq", "status=open"],
        ),
        "structured search narrows to open tasks",
    );
    assert_eq!(search_ids(&results), vec!["locate-task-a".to_string()]);
    let results = stdout_json(
        &run_cli(
            &config_path,
            &["search", "query", "--form", "Task", "--eq", "status=nope"],
        ),
        "structured search excludes on non-matching condition",
    );
    assert!(search_ids(&results).is_empty());

    // 5. Update the Entry; the receipt carries the durable Change ID.
    let history = stdout_json(
        &run_cli(&config_path, &["entry", "history", "locate-task-a"]),
        "entry history after create",
    );
    assert_eq!(revision_ids(&history).len(), 1);
    let status_field = "status=in-progress";
    let priority_field = "priority=3";
    let rev1 = revision_ids(&history)[0].clone();
    let updated = stdout_json(
        &run_cli(
            &config_path,
            &[
                "entry",
                "update",
                "locate-task-a",
                "--form",
                "Task",
                "--field",
                status_field,
                "--field",
                priority_field,
                "--parent-revision-id",
                &rev1,
            ],
        ),
        "entry update",
    );
    let update_change_id = updated
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("update returns durable change_id")
        .to_string();

    // Narrowing reflects the updated state: open no longer matches task-a.
    let results = stdout_json(
        &run_cli(
            &config_path,
            &["search", "query", "--form", "Task", "--eq", "status=open"],
        ),
        "structured search reflects the update",
    );
    assert!(search_ids(&results).is_empty());

    // 6. Space History observes create and update Changes.
    let changes = stdout_json(
        &run_cli(&config_path, &["change", "list"]),
        "change list observes the timeline",
    );
    let before_ids = change_ids(&changes);
    assert!(before_ids.contains(&update_change_id));
    assert!(before_ids.len() >= 3);

    // 7. Change revert appends its inverse; the reverted Change is kept.
    let reverted = stdout_json(
        &run_cli(&config_path, &["change", "revert", &update_change_id]),
        "change revert",
    );
    let revert_id = reverted
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("revert returns the appended change_id")
        .to_string();
    assert_ne!(revert_id, update_change_id);

    // 8. Entry history grows append-only; no revision is lost.
    let history = stdout_json(
        &run_cli(&config_path, &["entry", "history", "locate-task-a"]),
        "entry history after revert",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&rev1));
    let changes = stdout_json(
        &run_cli(&config_path, &["change", "list"]),
        "change list after revert",
    );
    let after_ids = change_ids(&changes);
    assert!(after_ids.contains(&update_change_id));
    assert!(after_ids.contains(&revert_id));
    assert_eq!(after_ids.len(), before_ids.len() + 1);

    // 9. Current search results reflect the recovered state.
    let results = stdout_json(
        &run_cli(
            &config_path,
            &["search", "query", "--form", "Task", "--eq", "status=open"],
        ),
        "structured search reflects recovered state",
    );
    assert_eq!(search_ids(&results), vec!["locate-task-a".to_string()]);

    // 10. Reopen: fresh invocations read the same durable state.
    let history = stdout_json(
        &run_cli(&config_path, &["entry", "history", "locate-task-a"]),
        "entry history on reopen",
    );
    assert_eq!(revision_ids(&history).len(), 3);
    let results = stdout_json(
        &run_cli(&config_path, &["search", "keyword", "locate-task-a"]),
        "keyword search on reopen",
    );
    assert!(contains_string(&results, "locate-task-a"));
    let changes = stdout_json(
        &run_cli(&config_path, &["change", "list"]),
        "change list on reopen",
    );
    assert_eq!(change_ids(&changes).len(), after_ids.len());
}
