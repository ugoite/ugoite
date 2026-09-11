//! CLI output/error/receipt contract (Wave 4 E0/E1/E2/E3).
//!
//! - `stdout` carries success data only; `stderr` carries errors.
//! - Piped output is machine JSON; errors are `{"error": {code,kind,message,detail}}`.
//! - Exit codes: 0 success, 2 usage, 3 forbidden, 4 not-found, 5 conflict,
//!   6 dependency-unavailable, 7 unsupported, 1 internal.
//! - Mutation receipts carry kind/id/revision_id (change/run null, never fabricated).
//! - `--file` / `--file -` shell-safe ingress; inline+file rejected; no auto-stdin.
//! - Help examples are parse smoke fixtures.

use std::io::Write;
use std::process::{Command, Stdio};

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

fn setup_space_with_form(dir: &tempfile::TempDir, space_id: &str) -> (String, std::path::PathBuf) {
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");
    let space_path = format!("{root}/spaces/{space_id}");
    let status = Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, space_id])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create space")
        .status;
    assert!(status.success());
    let form_file = dir.path().join("entry-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"},"Count":{"type":"integer"}}}"#,
    )
    .unwrap();
    let status = Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create form")
        .status;
    assert!(status.success());
    (root, config_path)
}

fn error_envelope(stderr: &str) -> serde_json::Value {
    serde_json::from_str(stderr.trim()).expect("stderr must be a machine JSON envelope")
}

/// E0/E1: success prints receipt JSON on stdout with empty stderr.
#[test]
fn mutation_receipt_reaches_stdout_json_with_empty_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "receipt-space");
    let space_path = format!("{root}/spaces/receipt-space");
    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            "---\nform: Entry\n---\n# Receipt\n\n## Body\n\nhi\n",
            &space_path,
            "receipt-1",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create");
    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("stdout JSON");
    assert_eq!(stdout["kind"], "entry");
    assert_eq!(stdout["id"], "receipt-1");
    assert!(stdout["revision_id"].is_string());
}

/// E1: validation failure is exit 2 with a stable machine envelope.
#[test]
fn validation_failure_reports_machine_envelope_and_usage_exit() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "envelope-space");
    let space_path = format!("{root}/spaces/envelope-space");
    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            "---\nform: Entry\n---\n# Bad\n\n## Count\nnot-a-number\n",
            &space_path,
            "bad-1",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "stdout must be empty on error: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let envelope = error_envelope(&String::from_utf8_lossy(&output.stderr));
    assert_eq!(envelope["error"]["code"], "FORM_VALIDATION_FAILED");
    assert_eq!(envelope["error"]["kind"], "invalid_input");
    assert!(envelope["error"]["detail"]["warnings"].is_array());
}

/// E1: stale revisions conflict with exit 5 and canonical recovery detail.
#[test]
fn revision_conflict_reports_recovery_detail_and_conflict_exit() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "conflict-space");
    let space_path = format!("{root}/spaces/conflict-space");
    let create = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            "--content",
            "---\nform: Entry\n---\n# T\n\n## Body\n\na\n",
            &space_path,
            "conflict-1",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create");
    assert!(create.status.success());
    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "update",
            &space_path,
            "conflict-1",
            "--markdown",
            "---\nform: Entry\n---\n# T\n\n## Body\n\nb\n",
            "--parent-revision-id",
            "stale-revision",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("update");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(5));
    let envelope = error_envelope(&String::from_utf8_lossy(&output.stderr));
    assert_eq!(envelope["error"]["code"], "REVISION_CONFLICT");
    assert_eq!(envelope["error"]["kind"], "conflict");
    assert!(
        envelope["error"]["detail"]["current_revision_id"].is_string(),
        "envelope: {envelope}"
    );
    assert_eq!(
        envelope["error"]["detail"]["recovery_action"],
        "reload_and_retry"
    );
}

/// E1: missing entries exit 4 with a stable code.
#[test]
fn missing_entry_reports_not_found_exit() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "missing-space");
    let space_path = format!("{root}/spaces/missing-space");
    let output = Command::new(ugoite_bin())
        .args(["entry", "get", &space_path, "no-such-entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("get");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(4));
    let envelope = error_envelope(&String::from_utf8_lossy(&output.stderr));
    assert_eq!(envelope["error"]["code"], "ENTRY_NOT_FOUND");
}

/// E2: `--file` reads compatibility Markdown; inline+file is rejected.
#[test]
fn file_ingress_reads_markdown_and_rejects_inline_combination() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "file-space");
    let space_path = format!("{root}/spaces/file-space");
    let note = dir.path().join("note.md");
    std::fs::write(
        &note,
        "---\nform: Entry\n---\n# Filed\n\n## Body\n\nfrom file\n",
    )
    .unwrap();

    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "file-1",
            "--file",
            note.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create from file");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "file-2",
            "--content",
            "# x",
            "--file",
            note.to_str().unwrap(),
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("combined");
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(2));
}

/// E2: explicit `--file -` consumes stdin exactly once.
#[test]
fn explicit_stdin_file_ingress_creates_entry() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "stdin-space");
    let space_path = format!("{root}/spaces/stdin-space");
    let mut child = Command::new(ugoite_bin())
        .args(["entry", "create", &space_path, "stdin-1", "--file", "-"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(b"---\nform: Entry\n---\n# Piped\n\n## Body\n\nvia stdin\n")
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.stdout)).expect("stdout JSON");
    assert_eq!(stdout["id"], "stdin-1");
}

/// E3: help examples are executable documentation, not prose.
#[test]
fn help_examples_are_present_and_local_example_runs() {
    for args in [
        vec!["entry", "create", "--help"],
        vec!["entry", "update", "--help"],
    ] {
        let output = Command::new(ugoite_bin())
            .args(&args)
            .output()
            .expect("help");
        assert!(output.status.success());
        let text = String::from_utf8_lossy(&output.stdout).to_string();
        assert!(text.contains("Examples:"), "help: {text}");
        assert!(text.contains("--file"), "help: {text}");
    }

    // The documented core-mode create runs on a temp Space.
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "help-space");
    let space_path = format!("{root}/spaces/help-space");
    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "create",
            &space_path,
            "help-1",
            "--content",
            "---\nform: Entry\n---\n# Help\n\n## Body\n\nok\n",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("help example create");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
