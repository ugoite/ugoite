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

/// Lane1 PR8: structured update appends one revision and keeps history.
#[test]
fn test_structured_update_appends_revision() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "update-space");
    let space_path = format!("{root}/spaces/update-space");
    let env = |cmd: &mut std::process::Command| {
        cmd.env("UGOITE_CLI_CONFIG_PATH", &config_path);
    };

    let mut create = std::process::Command::new(ugoite_bin());
    create.args([
        "entry",
        "create",
        &space_path,
        "up-entry",
        "--form",
        "Entry",
        "--field",
        "Body=v1",
    ]);
    env(&mut create);
    let created = create.output().unwrap();
    assert!(
        created.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let mut history = std::process::Command::new(ugoite_bin());
    history.args(["entry", "history", &space_path, "up-entry"]);
    env(&mut history);
    let history = history.output().unwrap();
    let history_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&history.stdout)).unwrap();
    let rev1 = history_json["revisions"][0]["revision_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(history_json["revisions"].as_array().unwrap().len(), 1);

    let fields_file = dir.path().join("update-fields.json");
    std::fs::write(&fields_file, r#"{"Body":"v2"}"#).unwrap();
    let mut update = std::process::Command::new(ugoite_bin());
    update.args([
        "entry",
        "update",
        &space_path,
        "up-entry",
        "--fields-file",
        fields_file.to_str().unwrap(),
        "--parent-revision-id",
        &rev1,
    ]);
    env(&mut update);
    let updated = update.output().unwrap();
    assert!(
        updated.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&updated.stderr)
    );

    let mut history2 = std::process::Command::new(ugoite_bin());
    history2.args(["entry", "history", &space_path, "up-entry"]);
    env(&mut history2);
    let history2 = history2.output().unwrap();
    let history2_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&history2.stdout)).unwrap();
    assert_eq!(history2_json["revisions"].as_array().unwrap().len(), 2);

    // Existing revision is unchanged.
    let mut rev = std::process::Command::new(ugoite_bin());
    rev.args(["entry", "revision", &space_path, "up-entry", &rev1]);
    env(&mut rev);
    let rev = rev.output().unwrap();
    assert!(rev.status.success());
    assert!(String::from_utf8_lossy(&rev.stdout).contains("v1"));
}

/// Lane1 PR8: stale parent revisions conflict identically on both paths.
#[test]
fn test_structured_update_stale_parent_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "conflict-space");
    let space_path = format!("{root}/spaces/conflict-space");
    let env = |cmd: &mut std::process::Command| {
        cmd.env("UGOITE_CLI_CONFIG_PATH", &config_path);
    };

    let mut create = std::process::Command::new(ugoite_bin());
    create.args([
        "entry",
        "create",
        &space_path,
        "c-entry",
        "--form",
        "Entry",
        "--field",
        "Body=v1",
    ]);
    env(&mut create);
    assert!(create.output().unwrap().status.success());

    let mut history = std::process::Command::new(ugoite_bin());
    history.args(["entry", "history", &space_path, "c-entry"]);
    env(&mut history);
    let history = history.output().unwrap();
    let history_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&history.stdout)).unwrap();
    let rev1 = history_json["revisions"][0]["revision_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Advance once so rev1 goes stale.
    let mut advance = std::process::Command::new(ugoite_bin());
    advance.args([
        "entry",
        "update",
        &space_path,
        "c-entry",
        "--field",
        "Body=v2",
    ]);
    env(&mut advance);
    assert!(advance.output().unwrap().status.success());

    let mut update = std::process::Command::new(ugoite_bin());
    update.args([
        "entry",
        "update",
        &space_path,
        "c-entry",
        "--field",
        "Body=v3",
        "--parent-revision-id",
        &rev1,
    ]);
    env(&mut update);
    let out = update.output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("REVISION_CONFLICT"));

    // Failed conflicts append no revision.
    let mut history_after = std::process::Command::new(ugoite_bin());
    history_after.args(["entry", "history", &space_path, "c-entry"]);
    env(&mut history_after);
    let history_after = history_after.output().unwrap();
    let history_after_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&history_after.stdout)).unwrap();
    assert_eq!(history_after_json["revisions"].as_array().unwrap().len(), 2);
}

/// Lane1 PR8: raw Markdown and structured updates reach the same durable result.
#[test]
fn test_raw_and_structured_update_parity() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "parity-space");
    let space_path = format!("{root}/spaces/parity-space");
    let env = |cmd: &mut std::process::Command| {
        cmd.env("UGOITE_CLI_CONFIG_PATH", &config_path);
    };
    let markdown = "---\nform: Entry\n---\n# T\n\n## Body\n\nsame\n";
    for entry_id in ["raw-entry", "st-entry"] {
        let mut create = std::process::Command::new(ugoite_bin());
        create.args([
            "entry",
            "create",
            "--content",
            markdown,
            &space_path,
            entry_id,
        ]);
        env(&mut create);
        assert!(create.output().unwrap().status.success());
    }

    let mut raw_update = std::process::Command::new(ugoite_bin());
    raw_update.args([
        "entry",
        "update",
        &space_path,
        "raw-entry",
        "--markdown",
        "---\nform: Entry\n---\n# New\n\n## Body\n\nsame\n",
    ]);
    env(&mut raw_update);
    assert!(raw_update.output().unwrap().status.success());

    let mut st_update = std::process::Command::new(ugoite_bin());
    st_update.args([
        "entry",
        "update",
        &space_path,
        "st-entry",
        "--title",
        "New",
        "--field",
        "Body=same",
    ]);
    env(&mut st_update);
    let st_out = st_update.output().unwrap();
    assert!(
        st_out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&st_out.stderr)
    );

    let mut raw_get = std::process::Command::new(ugoite_bin());
    raw_get.args(["entry", "get", &space_path, "raw-entry"]);
    env(&mut raw_get);
    let mut st_get = std::process::Command::new(ugoite_bin());
    st_get.args(["entry", "get", &space_path, "st-entry"]);
    env(&mut st_get);
    let raw_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&raw_get.output().unwrap().stdout)).unwrap();
    let st_json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&st_get.output().unwrap().stdout)).unwrap();
    assert_eq!(raw_json.get("content"), st_json.get("content"));
    assert_eq!(raw_json.get("title"), st_json.get("title"));
    assert_eq!(raw_json.get("form"), st_json.get("form"));
    assert_eq!(raw_json.get("sections"), st_json.get("sections"));
}

/// Lane1 PR8: form identity changes are canonical errors, not rewrites.
#[test]
fn test_structured_update_form_change_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "form-space");
    let space_path = format!("{root}/spaces/form-space");

    let mut create = std::process::Command::new(ugoite_bin());
    create.args([
        "entry",
        "create",
        &space_path,
        "f-entry",
        "--form",
        "Entry",
        "--field",
        "Body=x",
    ]);
    create.env("UGOITE_CLI_CONFIG_PATH", &config_path);
    assert!(create.output().unwrap().status.success());

    let mut update = std::process::Command::new(ugoite_bin());
    update.args([
        "entry",
        "update",
        &space_path,
        "f-entry",
        "--form",
        "Other",
        "--field",
        "Body=x",
    ]);
    update.env("UGOITE_CLI_CONFIG_PATH", &config_path);
    let out = update.output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("Form change is not supported"));
}

/// Lane1 PR8: structured/markdown mixing and field-less updates are usage errors.
#[test]
fn test_structured_update_usage_errors() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "update-usage-space");
    let space_path = format!("{root}/spaces/update-usage-space");
    let env = |cmd: &mut std::process::Command| {
        cmd.env("UGOITE_CLI_CONFIG_PATH", &config_path);
    };

    let mut create = std::process::Command::new(ugoite_bin());
    create.args([
        "entry",
        "create",
        &space_path,
        "u-entry",
        "--form",
        "Entry",
        "--field",
        "Body=v1",
    ]);
    env(&mut create);
    assert!(create.output().unwrap().status.success());

    // Structured + Markdown together.
    let mut both = std::process::Command::new(ugoite_bin());
    both.args([
        "entry",
        "update",
        &space_path,
        "u-entry",
        "--markdown",
        "# Hi",
        "--field",
        "Body=v2",
    ]);
    env(&mut both);
    let both = both.output().unwrap();
    assert!(!both.status.success());
    assert!(String::from_utf8_lossy(&both.stderr).contains("cannot be combined"));

    // Bare --title without field inputs would silently clear; it is rejected.
    let mut bare = std::process::Command::new(ugoite_bin());
    bare.args(["entry", "update", &space_path, "u-entry", "--title", "New"]);
    env(&mut bare);
    let bare = bare.output().unwrap();
    assert!(!bare.status.success());
    assert!(String::from_utf8_lossy(&bare.stderr).contains("requires --field or --fields-file"));
}
