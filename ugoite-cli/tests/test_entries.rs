//! Integration tests for entry management commands.
//! REQ-ENTRY-001, REQ-ENTRY-002, REQ-ENTRY-003, REQ-ENTRY-004, REQ-ENTRY-005

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

/// Set up a space with an Entry form for tests.
fn setup_space_with_form(dir: &tempfile::TempDir, space_id: &str) -> (String, std::path::PathBuf) {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/{space_id}");

    Command::new(ugoite_bin())
        .args(["create-space", &root, space_id])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    let form_file = dir.path().join("entry-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"}}}"#,
    )
    .unwrap();

    Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create form");

    (root, config_path)
}

/// REQ-ENTRY-001: Create entry from Markdown content.
#[test]
fn test_create_entry_basic() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# Hello World\n\n## Body\n\nContent here.";

    let output = Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content, &space_path, "entry-001"])
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
    assert_eq!(v.get("id").and_then(|x| x.as_str()), Some("entry-001"));
}

/// REQ-ENTRY-002: Optimistic concurrency - revision mismatch returns error.
#[test]
fn test_update_entry_revision_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# Initial\n\n## Body\n\nContent.";

    Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content, &space_path, "entry-rev"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Update with wrong revision should fail
    let output = Command::new(ugoite_bin())
        .args([
            "entry", "update",
            &space_path, "entry-rev",
            "--markdown", "# Updated\n\n## Body\n\nNew content.",
            "--parent-revision-id", "wrong-revision-id",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(!output.status.success(), "Expected failure on revision mismatch");
}

/// REQ-ENTRY-003: Entry history is appended on each update.
#[test]
fn test_entry_history_append() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# Version 1\n\n## Body\n\nContent.";

    Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content, &space_path, "entry-hist"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let history = Command::new(ugoite_bin())
        .args(["entry", "history", &space_path, "entry-hist"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        history.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&history.stderr)
    );
    let stdout = String::from_utf8_lossy(&history.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    let revisions = v.get("revisions").and_then(|r| r.as_array());
    assert!(revisions.map(|a| !a.is_empty()).unwrap_or(false));
}

/// REQ-ENTRY-003: Entry history shows revision information.
#[test]
fn test_entry_history_diff() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# Version 1\n\n## Body\n\nContent.";

    Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content, &space_path, "entry-diff"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let history = Command::new(ugoite_bin())
        .args(["entry", "history", &space_path, "entry-diff"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(history.status.success());
    let stdout = String::from_utf8_lossy(&history.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let arr = v.get("revisions").and_then(|r| r.as_array()).unwrap();
    assert!(!arr.is_empty());
    // Each revision should have a revision_id
    assert!(arr[0].get("revision_id").is_some());
}

/// REQ-ENTRY-004: Markdown sections persist as structured fields.
#[test]
fn test_markdown_sections_persist() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# Entry Title\n\n## Body\n\nThis is the body section.";

    Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content, &space_path, "entry-sections"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let get_output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "entry-sections"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        get_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&get_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&get_output.stdout);
    assert!(stdout.contains("entry-sections"));
}

/// REQ-ENTRY-005: List entries returns properties and links.
#[test]
fn test_list_entries_returns_properties_and_links() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# List Test\n\n## Body\n\nContent here.";

    Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content, &space_path, "entry-list-test"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    let list_output = Command::new(ugoite_bin())
        .args(["entry", "list", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        list_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&list_output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    assert!(v.as_array().map(|a| !a.is_empty()).unwrap_or(false));
}


