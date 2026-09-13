//! CLI output/error/receipt contract (Wave 4 E0/E1/E2/E3).
//!
//! - `stdout` carries success data only; `stderr` carries errors.
//! - Piped output is machine JSON; errors are `{"error": {code,kind,message,detail}}`.
//! - Exit codes: 0 success, 2 usage, 3 forbidden, 4 not-found, 5 conflict,
//!   6 dependency-unavailable, 7 unsupported, 1 internal.
//! - Mutation receipts (kind/id/revision_id, change/run null, never fabricated)
//!   are TTY display only in 0.1.x; machine output keeps the existing shape.
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

fn strip_ansi(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut escape = false;
    for character in text.chars() {
        if escape {
            if character.is_ascii_alphabetic() {
                escape = false;
            }
        } else if character == '\u{1b}' {
            escape = true;
        } else {
            stripped.push(character);
        }
    }
    stripped
}

fn assert_success(output: &std::process::Output, command: &str) {
    assert!(
        output.status.success(),
        "{command} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run(config_path: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(ugoite_bin())
        .args(args)
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("run CLI")
}

/// Quiet Accent's representative human projections stay compact and
/// borderless, while the same commands retain their machine JSON shape when
/// stdout is piped.
#[test]
fn representative_commands_lock_human_and_machine_output_contracts() {
    let dir = tempfile::tempdir().unwrap();
    let (root, config_path) = setup_space_with_form(&dir, "quiet-accent-space");
    let space_path = format!("{root}/spaces/quiet-accent-space");

    for (entry_id, title, body) in [
        ("note-1", "Planning", "planning details"),
        ("note-2", "Decisions", "decision details"),
    ] {
        let content = format!("---\nform: Entry\n---\n# {title}\n\n## Body\n\n{body}\n");
        let output = run(
            &config_path,
            &[
                "entry",
                "create",
                "--content",
                &content,
                &space_path,
                entry_id,
            ],
        );
        assert_success(&output, "entry create");
    }

    let space_table = run(&config_path, &["space", "--format", "table", "list", &root]);
    assert_success(&space_table, "space list table");
    let space_json = run(&config_path, &["space", "list", &root]);
    assert_success(&space_json, "space list JSON");
    let spaces: serde_json::Value = serde_json::from_slice(&space_json.stdout).unwrap();
    let space_id = spaces[0].as_str().expect("space list returns IDs");
    assert_eq!(
        strip_ansi(&String::from_utf8_lossy(&space_table.stdout)),
        format!("SPACE_ID\n{space_id}\n")
    );
    assert!(
        !space_table.stdout.contains(&0x1b),
        "piped table must be plain"
    );

    let entry_table = run(
        &config_path,
        &["entry", "--format", "table", "list", &space_path],
    );
    assert_success(&entry_table, "entry list table");
    assert_eq!(
        strip_ansi(&String::from_utf8_lossy(&entry_table.stdout)),
        "ID      TITLE\nnote-2  Decisions\nnote-1  Planning\n"
    );
    assert!(
        !entry_table.stdout.contains(&0x1b),
        "piped table must be plain"
    );

    let search_table = run(
        &config_path,
        &[
            "search",
            "--format",
            "table",
            "keyword",
            &space_path,
            "planning",
        ],
    );
    assert_success(&search_table, "search keyword table");
    assert_eq!(
        strip_ansi(&String::from_utf8_lossy(&search_table.stdout)),
        "ID      TITLE\nnote-1  Planning\n"
    );
    assert!(
        !search_table.stdout.contains(&0x1b),
        "piped table must be plain"
    );

    let receipt = run(
        &config_path,
        &[
            "entry",
            "--format",
            "plain",
            "create",
            "--content",
            "---\nform: Entry\n---\n# Receipt\n\n## Body\n\nreceipt\n",
            &space_path,
            "receipt-1",
        ],
    );
    assert_success(&receipt, "entry create receipt");
    let receipt_text = String::from_utf8_lossy(&receipt.stdout);
    assert!(
        receipt_text.starts_with("entry receipt-1\n"),
        "stdout: {receipt_text}"
    );
    assert!(receipt_text.contains("revision:"), "stdout: {receipt_text}");
    assert!(
        !receipt.stdout.contains(&0x1b),
        "piped receipt must be plain"
    );

    assert!(spaces
        .as_array()
        .is_some_and(|items| { items.iter().any(|item| item.as_str() == Some(space_id)) }));

    let entry_json = run(&config_path, &["entry", "list", &space_path]);
    assert_success(&entry_json, "entry list JSON");
    let entries: serde_json::Value = serde_json::from_slice(&entry_json.stdout).unwrap();
    assert_eq!(entries.as_array().map(Vec::len), Some(3));

    let search_json = run(
        &config_path,
        &["search", "keyword", &space_path, "planning"],
    );
    assert_success(&search_json, "search keyword JSON");
    let results: serde_json::Value = serde_json::from_slice(&search_json.stdout).unwrap();
    assert_eq!(results[0]["id"], "note-1");
    assert_eq!(results[0]["title"], "Planning");
    assert!(results[0]["form"].is_string());
}

/// Piped help and parser failures remain plain text with the established
/// command content and usage exit code.
#[test]
fn representative_help_and_invalid_arguments_are_plain_when_piped() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");

    let help = run(&config_path, &["--help"]);
    assert_success(&help, "top-level help");
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("Quick start (local-first / core mode):"));
    assert!(help_text.contains("ugoite space list ."));
    assert!(!help.stdout.contains(&0x1b), "piped help must be plain");

    let current = run(&config_path, &["config", "current"]);
    assert_success(&current, "config current");
    assert!(String::from_utf8_lossy(&current.stdout).starts_with("Current endpoint mode: core\n"));
    assert!(
        !current.stdout.contains(&0x1b),
        "piped config must be plain"
    );

    let invalid = run(&config_path, &["entry", "get"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(invalid.stdout.is_empty());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Usage:"));
    assert!(
        !invalid.stderr.contains(&0x1b),
        "piped parser errors must be plain"
    );
}

/// E0/E1: machine output keeps the existing shape on stdout with empty
/// stderr; the receipt is TTY display only in 0.1.x (machine default switches
/// to the receipt in v0.2).
#[test]
fn mutation_machine_output_keeps_existing_shape_with_empty_stderr() {
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
    assert_eq!(stdout["id"], "receipt-1");
    assert!(stdout["revision_id"].is_string());

    // Explicit table format renders the human receipt summary instead.
    let output = Command::new(ugoite_bin())
        .args([
            "entry",
            "-o",
            "table",
            "create",
            "--content",
            "---\nform: Entry\n---\n# Receipt\n\n## Body\n\nhi\n",
            &space_path,
            "receipt-2",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("create table");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    assert!(text.contains("entry receipt-2"), "stdout: {text}");
    assert!(text.contains("revision:"), "stdout: {text}");
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
