//! Integration tests for entry management commands.
//! REQ-ENTRY-001, REQ-ENTRY-002, REQ-ENTRY-003, REQ-ENTRY-004, REQ-ENTRY-005

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

/// Set up a space with an Entry form for tests.
fn setup_space_with_form(dir: &tempfile::TempDir, space_id: &str) -> (String, std::path::PathBuf) {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/{space_id}");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, space_id])
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
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-001",
        ])
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

/// REQ-ENTRY-002: Local CLI validation errors identify the field, contract,
/// and reason using the structured warning returned by core.
#[test]
fn test_invalid_typed_entry_cli_error_is_actionable() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/typed-entry-space");

    let create_space = Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "typed-entry-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");
    assert!(create_space.status.success());

    let form_file = dir.path().join("typed-entry-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"TypedEntry","fields":{"StartedAt":{"type":"date"},"ArtifactId":{"type":"uuid"}}}"#,
    )
    .unwrap();
    let create_form = Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create typed form");
    assert!(
        create_form.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_form.stderr)
    );

    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            "---\nform: TypedEntry\n---\n# Invalid Entry\n\n## StartedAt\n\ntomorrow",
            &space_path,
            "invalid-typed-entry",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create invalid typed entry");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("StartedAt"), "stderr: {stderr}");
    assert!(stderr.contains("ISO date YYYY-MM-DD"), "stderr: {stderr}");
    assert!(stderr.contains("does not match"), "stderr: {stderr}");
}

/// REQ-ENTRY-002: Optimistic concurrency - revision mismatch returns error.
#[test]
fn test_update_entry_revision_mismatch() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# Initial\n\n## Body\n\nContent.";

    Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-rev",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Update with wrong revision should fail
    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "update",
            &space_path,
            "entry-rev",
            "--markdown",
            "# Updated\n\n## Body\n\nNew content.",
            "--parent-revision-id",
            "wrong-revision-id",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        !output.status.success(),
        "Expected failure on revision mismatch"
    );
}

/// REQ-ENTRY-003: Entry history is appended on each update.
#[test]
fn test_entry_history_append() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# Version 1\n\n## Body\n\nContent.";

    Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-hist",
        ])
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
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-diff",
        ])
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
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-sections",
        ])
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

/// REQ-ENTRY-005: List entries returns Form properties.
#[test]
fn test_list_entries_returns_properties() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "test-space");
    let space_path = format!("{root}/spaces/test-space");
    let content = "---\nform: Entry\n---\n# List Test\n\n## Body\n\nContent here.";

    Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            content,
            &space_path,
            "entry-list-test",
        ])
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

/// Lane1 PR7: structured create matches the Markdown compatibility result.
#[test]
fn test_structured_create_matches_markdown_result() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "structured-space");
    let space_path = format!("{root}/spaces/structured-space");

    let markdown = "---\nform: Entry\n---\n# Hello\n\n## Body\n\nContent here.";
    let created_md = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            markdown,
            &space_path,
            "md-entry",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("markdown create");
    assert!(
        created_md.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&created_md.stderr)
    );

    let created_st = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "st-entry",
            "--form",
            "Entry",
            "--title",
            "Hello",
            "--field",
            "Body=Content here.",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("structured create");
    assert!(
        created_st.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&created_st.stderr)
    );

    for entry_id in ["md-entry", "st-entry"] {
        let got = Command::new(ugoite_bin())
            .args(["entry", "get", &space_path, entry_id])
            .env("UGOITE_CLI_CONFIG_PATH", &config_path)
            .output()
            .expect("get");
        assert!(got.status.success());
    }
    let md = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "md-entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .unwrap();
    let st = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "st-entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .unwrap();
    let md_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&md.stdout)).unwrap();
    let st_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&st.stdout)).unwrap();
    assert_eq!(md_json.get("content"), st_json.get("content"));
}

/// Lane1 PR7: structured invalid fields reuse the shared error taxonomy.
#[test]
fn test_structured_create_invalid_field_error_matches_shared_taxonomy() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/structured-err-space");

    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "structured-err-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space");
    let form_file = dir.path().join("num-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Numbers","fields":{"Count":{"type":"integer"}}}"#,
    )
    .unwrap();
    Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create form");

    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "bad-entry",
            "--form",
            "Numbers",
            "--field",
            "Count=not-an-int",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("structured invalid");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("FORM_VALIDATION_FAILED"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("Count"), "stderr: {stderr}");
}

/// Lane1 PR7: structured usage errors are deterministic.
#[test]
fn test_structured_create_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "usage-space");
    let space_path = format!("{root}/spaces/usage-space");

    // Structured + Markdown together.
    let both = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "e1",
            "--content",
            "# Hi",
            "--form",
            "Entry",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .unwrap();
    assert!(!both.status.success());
    assert!(String::from_utf8_lossy(&both.stderr).contains("cannot be combined"));

    // Missing --form.
    let no_form = Command::new(ugoite_bin())
        .args(["entry", "create", &space_path, "e2", "--field", "Body=x"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .unwrap();
    assert!(!no_form.status.success());
    assert!(String::from_utf8_lossy(&no_form.stderr).contains("--form is required"));

    // Malformed --field.
    let bad_field = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "e3",
            "--form",
            "Entry",
            "--field",
            "NoEquals",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .unwrap();
    assert!(!bad_field.status.success());
    assert!(String::from_utf8_lossy(&bad_field.stderr).contains("KEY=VALUE"));

    // Duplicate keys across --field and --fields-file.
    let fields_file = dir.path().join("fields.json");
    std::fs::write(&fields_file, r#"{"Body":"from-file"}"#).unwrap();
    let dup = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "e4",
            "--form",
            "Entry",
            "--field",
            "Body=from-flag",
            "--fields-file",
            fields_file.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .unwrap();
    assert!(!dup.status.success());
    assert!(String::from_utf8_lossy(&dup.stderr).contains("duplicate field"));
}

/// Lane1 PR7: --fields-file JSON merges with --field and reads explicit stdin.
#[test]
fn test_structured_create_fields_file_and_stdin() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "file-space");
    let space_path = format!("{root}/spaces/file-space");

    let fields_file = dir.path().join("typed.json");
    std::fs::write(&fields_file, r#"{"Body":"file body"}"#).unwrap();
    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "file-entry",
            "--form",
            "Entry",
            "--fields-file",
            fields_file.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut child = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "stdin-entry",
            "--form",
            "Entry",
            "--fields-file",
            "-",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"Body":"stdin body"}"#)
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
