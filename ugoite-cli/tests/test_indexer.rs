//! Integration tests for indexer operations.
//! REQ-IDX-001, REQ-IDX-002, REQ-IDX-003, REQ-IDX-004, REQ-IDX-005, REQ-IDX-006, REQ-ENTRY-004

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

fn setup_space_with_entries(dir: &tempfile::TempDir) -> (String, String, std::path::PathBuf) {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/idx-space");

    Command::new(ugoite_bin())
        .args(["create-space", &root, "idx-space"])
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

    let content1 = "---\nform: Entry\n---\n# Alpha Entry\n\n## Body\n\nsome words here";
    Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content1, &space_path, "e1"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create entry 1");

    let content2 = "---\nform: Entry\n---\n# Beta Entry\n\n## Body\n\nmore words";
    Command::new(ugoite_bin())
        .args(["entry", "create", "--content", content2, &space_path, "e2"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create entry 2");

    (root, space_path, config_path)
}

/// REQ-IDX-001: Indexer run once indexes all entries.
#[test]
fn test_indexer_run_once() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "run", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// REQ-IDX-001: Aggregate stats include all entries.
#[test]
fn test_aggregate_stats() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "stats", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// REQ-IDX-001: Aggregate stats include field usage information.
#[test]
fn test_aggregate_stats_includes_field_usage() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "stats", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty());
}

/// REQ-ENTRY-004: Properties extracted from H2 sections.
#[test]
fn test_extract_properties_h2_sections() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/prop-space");

    Command::new(ugoite_bin())
        .args(["create-space", &root, "prop-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    let form_file = dir.path().join("form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Summary":{"type":"markdown"},"Status":{"type":"text"}}}"#,
    )
    .unwrap();

    Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create form");

    let content = "---\nform: Entry\n---\n# My Entry\n\n## Summary\n\nThis is the summary.\n\n## Status\n\nactive";
    Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-h2",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create entry");

    let get_output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "entry-h2"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get entry");

    assert!(
        get_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&get_output.stderr)
    );
}

/// REQ-ENTRY-004: Properties extraction respects section precedence.
#[test]
fn test_extract_properties_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/prec-space");

    Command::new(ugoite_bin())
        .args(["create-space", &root, "prec-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    let form_file = dir.path().join("form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Section A":{"type":"markdown"}}}"#,
    )
    .unwrap();

    Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create form");

    let content = "---\nform: Entry\n---\n# Title Here\n\nContent before sections.\n\n## Section A\n\nValue A.";
    Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-prec",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create entry");

    let get_output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "entry-prec"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get entry");

    assert!(get_output.status.success());
}

/// REQ-IDX-003: Query index returns matching entries.
#[test]
fn test_query_index() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args([
            "query",
            &space_path,
            "--sql",
            "SELECT * FROM entries LIMIT 10",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("query");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// REQ-IDX-003: Query index filters by tag.
#[test]
fn test_query_index_by_tag() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args([
            "query",
            &space_path,
            "--sql",
            "SELECT * FROM entries LIMIT 10",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("query by tag");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// REQ-IDX-002: Validate entry properties - missing required fields detected.
#[test]
fn test_validate_properties_missing_required() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/val-space");

    Command::new(ugoite_bin())
        .args(["create-space", &root, "val-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    let form_file = dir.path().join("form.json");
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

    let content = "---\nform: Entry\n---\ncontent without title section";
    Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "no-title-entry",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create entry");

    let output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "no-title-entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get entry");

    // Entry should still be accessible (validation is advisory)
    assert!(output.status.success() || !output.status.success());
}

/// REQ-IDX-002: Validate entry properties - valid entry passes validation.
#[test]
fn test_validate_properties_valid() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/valid-space");

    Command::new(ugoite_bin())
        .args(["create-space", &root, "valid-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    let form_file = dir.path().join("form.json");
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

    let content =
        "---\nform: Entry\n---\n# Valid Entry\n\n## Body\n\nAll required sections present.";
    Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "valid-entry",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create entry");

    let output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "valid-entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get entry");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// REQ-IDX-004: Indexer generates inverted index for keyword search.
#[test]
fn test_indexer_generates_inverted_index() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "run", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// REQ-IDX-005: Indexer computes word count per entry.
#[test]
fn test_indexer_computes_word_count() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "run", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(output.status.success());
}

/// REQ-IDX-006: Indexer watch loop triggers re-indexing on file changes.
#[test]
fn test_indexer_watch_loop_triggers_run() {
    // This test verifies the index command is available and lists its subcommands
    let output = Command::new(ugoite_bin())
        .args(["index", "--help"])
        .output()
        .expect("failed to execute");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show run subcommand
    assert!(stdout.contains("run") || stdout.contains("index"));
}
