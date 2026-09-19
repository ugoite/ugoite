//! PR-04 CLI help contract: every Space-bound command leads with the selected
//! context, shows the `--context` override second, and keeps the legacy
//! explicit Space positional as a labeled 0.1.x compatibility third tier.
//!
//! Structured authoring is shown first; raw Markdown is the labeled 0.1.x
//! compatibility surface. Selected-context examples are executable smoke
//! fixtures (not prose).

use std::path::PathBuf;
use std::process::Command;

fn ugoite_bin() -> PathBuf {
    if let Some(path) = option_env!("CARGO_BIN_EXE_ugoite") {
        return PathBuf::from(path);
    }
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path
}

fn help_text(args: &[&str]) -> String {
    let output = Command::new(ugoite_bin())
        .args(args)
        .output()
        .expect("help must run");
    assert!(
        output.status.success(),
        "{} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// The three example tiers appear in order: selected context, `--context`
/// override, then labeled 0.1.x compatibility.
fn assert_three_tiers(command: &[&str]) {
    let mut args: Vec<&str> = command.to_vec();
    args.push("--help");
    let text = help_text(&args);
    let tier1 = text.find("# Selected context").unwrap_or_else(|| {
        panic!(
            "{} help must show the selected context first: {text}",
            args.join(" ")
        )
    });
    let tier2 = text.find("--context NAME").unwrap_or_else(|| {
        panic!(
            "{} help must show the --context override second: {text}",
            args.join(" ")
        )
    });
    let tier3 = text.find("# 0.1.x compatibility").unwrap_or_else(|| {
        panic!(
            "{} help must label the legacy explicit Space third: {text}",
            args.join(" ")
        )
    });
    assert!(
        tier1 < tier2 && tier2 < tier3,
        "{} help tiers out of order: {text}",
        args.join(" ")
    );
}

#[test]
fn space_bound_help_is_context_first_with_compat_third() {
    for command in [
        vec!["entry", "list"],
        vec!["entry", "get"],
        vec!["entry", "create"],
        vec!["entry", "update"],
        vec!["entry", "delete"],
        vec!["entry", "history"],
        vec!["entry", "revision"],
        vec!["entry", "restore"],
        vec!["form", "list"],
        vec!["form", "get"],
        vec!["form", "update"],
        vec!["pin", "create"],
        vec!["pin", "list"],
        vec!["pin", "read"],
        vec!["pin", "diff"],
        vec!["pin", "delete"],
        vec!["change", "list"],
        vec!["change", "revert"],
        vec!["run", "undo"],
        vec!["asset", "upload"],
        vec!["asset", "delete"],
        vec!["asset", "list"],
        vec!["asset", "read"],
        vec!["asset", "download"],
        vec!["index", "run"],
        vec!["index", "stats"],
        vec!["search", "keyword"],
        vec!["search", "query"],
        vec!["space", "get"],
        vec!["space", "patch"],
        vec!["query"],
        vec!["sql", "saved-list"],
        vec!["sql", "saved-get"],
        vec!["sql", "saved-create"],
        vec!["sql", "saved-update"],
        vec!["sql", "saved-delete"],
        vec!["sql", "saved-execute"],
        vec!["sql", "session-create"],
        vec!["sql", "session-get"],
        vec!["sql", "session-metadata"],
        vec!["sql", "session-count"],
        vec!["sql", "session-rows"],
    ] {
        assert_three_tiers(&command);
    }
}

#[test]
fn structured_authoring_leads_and_raw_markdown_is_compat_labeled() {
    for command in [vec!["entry", "create"], vec!["entry", "update"]] {
        let mut args: Vec<&str> = command.clone();
        args.push("--help");
        let text = help_text(&args);
        let structured = text
            .find("Examples (structured, preferred)")
            .unwrap_or_else(|| {
                panic!(
                    "{} help must lead with structured authoring: {text}",
                    args.join(" ")
                )
            });
        let compat = text
            .find("raw Markdown, 0.1.x compatibility")
            .unwrap_or_else(|| {
                panic!(
                    "{} help must label raw Markdown as 0.1.x compatibility: {text}",
                    args.join(" ")
                )
            });
        assert!(
            structured < compat,
            "{} help must show structured authoring first: {text}",
            args.join(" ")
        );
    }
}

#[test]
fn context_path_help_states_uid_directory_classifier_only() {
    let text = help_text(&["context", "add", "--help"]);
    assert!(
        text.contains("exactly one local directory"),
        "context add help must state the UID -> directory rule: {text}"
    );
    assert!(
        text.contains("never consulted"),
        "context add help must exclude slug/path discovery: {text}"
    );
    let current = help_text(&["config", "current", "--help"]);
    assert!(
        current.contains("Machine shape"),
        "config current help must document the machine shape: {current}"
    );
}

struct Sandbox {
    home: PathBuf,
    work: PathBuf,
    legacy_config: PathBuf,
    _home_dir: tempfile::TempDir,
    _work_dir: tempfile::TempDir,
}

impl Sandbox {
    fn fresh() -> Self {
        let home_dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        let legacy_config = home_dir.path().join("legacy-endpoints.json");
        Self {
            home: home_dir.path().to_path_buf(),
            work: work_dir.path().to_path_buf(),
            legacy_config,
            _home_dir: home_dir,
            _work_dir: work_dir,
        }
    }

    fn command(&self, bin: &std::path::Path) -> Command {
        let mut command = Command::new(bin);
        command
            .env("HOME", &self.home)
            .env("UGOITE_CONFIG", "")
            .env("UGOITE_CLI_CONFIG_PATH", &self.legacy_config)
            .env("UGOITE_CONFIG_HOME", "")
            .env("XDG_CONFIG_HOME", "")
            .current_dir(&self.work);
        command
    }
}

fn assert_success(output: &std::process::Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// The tier-1 (selected context) and tier-2 (`--context`) examples execute.
#[test]
fn selected_context_and_override_examples_run() {
    let sandbox = Sandbox::fresh();
    let bin = ugoite_bin();

    assert_success(
        &sandbox
            .command(&bin)
            .args(["config", "init"])
            .output()
            .unwrap(),
        "config init",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args(["space", "create", "demo"])
            .output()
            .unwrap(),
        "space create demo",
    );

    // Tier 1: ENTRY_ID only against the selected context (structured first).
    // Seed the Task form through the selected context first.
    let form_file = sandbox.work.join("task-form.json");
    std::fs::write(
        &form_file,
        r#"{"name":"Task","fields":{"status":{"id":100,"type":"string"}}}"#,
    )
    .unwrap();
    assert_success(
        &sandbox
            .command(&bin)
            .args(["form", "update", form_file.to_str().unwrap()])
            .output()
            .unwrap(),
        "tier-1 selected-context form update",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args([
                "entry",
                "create",
                "smoke-1",
                "--form",
                "Task",
                "--field",
                "status=open",
            ])
            .output()
            .unwrap(),
        "tier-1 selected-context create",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args(["entry", "list"])
            .output()
            .unwrap(),
        "tier-1 selected-context list",
    );

    // Tier 2: one-invocation override without changing the selection.
    let listed = sandbox
        .command(&bin)
        .args(["--context", "demo", "entry", "list"])
        .output()
        .unwrap();
    assert_success(&listed, "tier-2 --context override list");
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("smoke-1"),
        "override must see the same Space: {}",
        String::from_utf8_lossy(&listed.stdout),
    );

    // Selection is unchanged by the override.
    let current = sandbox
        .command(&bin)
        .args(["context", "current"])
        .output()
        .unwrap();
    assert_success(&current, "context current");
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "demo");
}
