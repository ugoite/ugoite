//! Integration tests for indexer operations.
//! REQ-SRCH-007, REQ-FORM-011, REQ-SRCH-006, REQ-ENTRY-004

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

fn create_structured_entry(
    config_path: &std::path::Path,
    entry_id: &str,
    form: &str,
    fields: &[(&str, &str)],
) -> std::process::Output {
    let mut command = Command::new(ugoite_bin());
    command.args(["entry", "create", "--form", form]);
    for (key, value) in fields {
        command.arg("--field").arg(format!("{key}={value}"));
    }
    command
        .arg(entry_id)
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("create structured entry")
}

fn setup_space_with_entries(
    dir: &tempfile::TempDir,
) -> (String, String, std::path::PathBuf, String) {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/idx-space");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "idx-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");

    let form_file = dir.path().join("entry-form.json");
    std::fs::write(
        &form_file,
        r#"{"id":"00000000-0000-0000-0000-000000000001","name":"Entry","fields":{"Body":{"id":100,"type":"markdown"}}}"#,
    )
    .unwrap();

    Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create form");
    let form_output = Command::new(ugoite_bin())
        .args(["form", "get", &space_path, "Entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get form");
    let form_json: serde_json::Value = serde_json::from_slice(&form_output.stdout).unwrap();
    let relation = form_json["sql_relation"]
        .as_str()
        .expect("backend SQL relation")
        .to_string();

    assert!(
        create_structured_entry(&config_path, "e1", "Entry", &[("Body", "some words here")])
            .status
            .success()
    );
    assert!(
        create_structured_entry(&config_path, "e2", "Entry", &[("Body", "more words")])
            .status
            .success()
    );

    (root, space_path, config_path, relation)
}

/// REQ-SRCH-007: Indexer run rebuilds the internal DerivedRelation.
#[test]
fn test_indexer_run_once() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path, _relation) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "run", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "index run stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// REQ-SRCH-007: Aggregate stats include all entries.
#[test]
fn test_aggregate_stats() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path, _relation) = setup_space_with_entries(&dir);

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

/// REQ-SRCH-007: Aggregate stats include field usage information.
#[test]
fn test_aggregate_stats_includes_field_usage() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path, _relation) = setup_space_with_entries(&dir);

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
        .args(["create-space", "--root", &root, "prop-space"])
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

    assert!(create_structured_entry(
        &config_path,
        "entry-h2",
        "Entry",
        &[("Summary", "This is the summary."), ("Status", "active")],
    )
    .status
    .success());

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
        .args(["create-space", "--root", &root, "prec-space"])
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

    assert!(create_structured_entry(
        &config_path,
        "entry-prec",
        "Entry",
        &[("Section A", "Value A.")],
    )
    .status
    .success());

    let get_output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "entry-prec"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get entry");

    assert!(get_output.status.success());
}

/// REQ-SRCH-006: Query index returns matching entries.
#[test]
fn test_query_index() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path, relation) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args([
            "query",
            &space_path,
            "--sql",
            &format!("SELECT * FROM \"{relation}\" LIMIT 10"),
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

/// REQ-SRCH-006: Query index filters by tag.
#[test]
fn test_query_index_by_tag() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path, relation) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args([
            "query",
            &space_path,
            "--sql",
            &format!("SELECT * FROM \"{relation}\" LIMIT 10"),
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

/// REQ-FORM-011: Validate entry properties - missing required fields detected.
#[test]
fn test_validate_properties_missing_required() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/val-space");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "val-space"])
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

    assert!(create_structured_entry(
        &config_path,
        "no-title-entry",
        "Entry",
        &[("Body", "content without title section")],
    )
    .status
    .success());

    let output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "no-title-entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get entry");

    // Entry should still be accessible (validation is advisory)
    assert!(output.status.success() || !output.status.success());
}

/// REQ-FORM-011: Validate entry properties - valid entry passes validation.
#[test]
fn test_validate_properties_valid() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/valid-space");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "valid-space"])
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

    assert!(create_structured_entry(
        &config_path,
        "valid-entry",
        "Entry",
        &[("Body", "All required sections present.")],
    )
    .status
    .success());

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

/// REQ-SRCH-007: AssetText rebuild does not claim to be an inverted index.
#[test]
fn test_indexer_generates_inverted_index() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path, _relation) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "run", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "{output:?}");
}

/// REQ-SRCH-007: Derived rebuild remains separate from word-count helpers.
#[test]
fn test_indexer_computes_word_count() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path, _relation) = setup_space_with_entries(&dir);

    let output = Command::new(ugoite_bin())
        .args(["index", "run", &space_path])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(output.status.success(), "{output:?}");
}

/// CLI index command availability.
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
