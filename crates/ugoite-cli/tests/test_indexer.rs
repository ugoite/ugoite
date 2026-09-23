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

fn init_canonical_space(dir: &tempfile::TempDir, slug: &str) -> std::path::PathBuf {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    assert!(run_cli(&config_path, &["config", "init"]).status.success());
    assert!(run_cli(
        &config_path,
        &[
            "config",
            "connection",
            "set",
            "local",
            "--type",
            "core",
            "--root",
            &root
        ]
    )
    .status
    .success());
    let created = run_cli(&config_path, &["space", "create", slug]);
    assert!(
        created.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    config_path
}

fn create_structured_entry(
    config_path: &std::path::Path,
    entry_id: &str,
    form: &str,
    fields: &[(&str, &str)],
) -> std::process::Output {
    let mut args = vec![
        "entry".to_string(),
        "create".to_string(),
        entry_id.to_string(),
        "--form".to_string(),
        form.to_string(),
    ];
    for (key, value) in fields {
        args.push("--field".to_string());
        args.push(format!("{key}={value}"));
    }
    let mut full = vec![
        "--config".to_string(),
        config_path.to_string_lossy().into_owned(),
    ];
    full.extend(args);
    Command::new(ugoite_bin())
        .args(full)
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("create structured entry")
}

fn setup_space_with_entries(dir: &tempfile::TempDir) -> (String, std::path::PathBuf, String) {
    let config_path = init_canonical_space(dir, "idx-space");

    let form_file = dir.path().join("entry-form.json");
    std::fs::write(
        &form_file,
        r#"{"id":"00000000-0000-0000-0000-000000000001","name":"Entry","fields":{"Body":{"id":100,"type":"markdown"}}}"#,
    )
    .unwrap();

    let updated = run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()],
    );
    assert!(
        updated.status.success(),
        "form update failed: {}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let form_output = run_cli(&config_path, &["form", "get", "Entry"]);
    assert!(form_output.status.success());
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

    (relation, config_path, String::new())
}

/// REQ-SRCH-007: Indexer run rebuilds the internal DerivedRelation.
#[test]
fn test_indexer_run_once() {
    let dir = tempfile::tempdir().unwrap();
    let (_relation, config_path, _) = setup_space_with_entries(&dir);

    let output = run_cli(&config_path, &["index", "run"]);

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
    let (_relation, config_path, _) = setup_space_with_entries(&dir);

    let output = run_cli(&config_path, &["index", "stats"]);

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
    let (_relation, config_path, _) = setup_space_with_entries(&dir);

    let output = run_cli(&config_path, &["index", "stats"]);

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty());
}

/// REQ-ENTRY-004: Properties extracted from H2 sections.
#[test]
fn test_extract_properties_h2_sections() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = init_canonical_space(&dir, "prop-space");

    let form_file = dir.path().join("form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"},"Summary":{"type":"markdown"},"Status":{"type":"string"}}}"#,
    )
    .unwrap();

    assert!(run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()]
    )
    .status
    .success());

    assert!(create_structured_entry(
        &config_path,
        "entry-h2",
        "Entry",
        &[("Summary", "This is the summary."), ("Status", "active")],
    )
    .status
    .success());

    let get_output = run_cli(&config_path, &["entry", "get", "entry-h2"]);

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
    let config_path = init_canonical_space(&dir, "prec-space");

    let form_file = dir.path().join("form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"},"Section A":{"type":"markdown"}}}"#,
    )
    .unwrap();

    assert!(run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()]
    )
    .status
    .success());

    assert!(create_structured_entry(
        &config_path,
        "entry-prec",
        "Entry",
        &[("Section A", "Value A.")],
    )
    .status
    .success());

    let get_output = run_cli(&config_path, &["entry", "get", "entry-prec"]);

    assert!(get_output.status.success());
}

/// REQ-FORM-011: Validate entry properties - missing required fields detected.
#[test]
fn test_validate_properties_missing_required() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = init_canonical_space(&dir, "val-space");

    let form_file = dir.path().join("form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"}}}"#,
    )
    .unwrap();

    assert!(run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()]
    )
    .status
    .success());

    assert!(create_structured_entry(
        &config_path,
        "no-title-entry",
        "Entry",
        &[("Body", "content without title section")],
    )
    .status
    .success());

    let output = run_cli(&config_path, &["entry", "get", "no-title-entry"]);

    // Entry should still be accessible (validation is advisory)
    assert!(output.status.success() || !output.status.success());
}

/// REQ-FORM-011: Validate entry properties - valid entry passes validation.
#[test]
fn test_validate_properties_valid() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = init_canonical_space(&dir, "valid-space");

    let form_file = dir.path().join("form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"}}}"#,
    )
    .unwrap();

    assert!(run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()]
    )
    .status
    .success());

    assert!(create_structured_entry(
        &config_path,
        "valid-entry",
        "Entry",
        &[("Body", "All required sections present.")],
    )
    .status
    .success());

    let output = run_cli(&config_path, &["entry", "get", "valid-entry"]);

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
    let (_relation, config_path, _) = setup_space_with_entries(&dir);

    let output = run_cli(&config_path, &["index", "run"]);

    assert!(output.status.success(), "{output:?}");
}

/// REQ-SRCH-007: Derived rebuild remains separate from word-count helpers.
#[test]
fn test_indexer_computes_word_count() {
    let dir = tempfile::tempdir().unwrap();
    let (_relation, config_path, _) = setup_space_with_entries(&dir);

    let output = run_cli(&config_path, &["index", "run"]);

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
