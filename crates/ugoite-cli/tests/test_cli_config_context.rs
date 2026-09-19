//! Acceptance tests for CLI configuration & contexts.
//!
//! Golden journey (context-first, no repeated Space path) and Knowledge
//! safety (config operations never mutate Spaces).

use std::path::{Path, PathBuf};
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

    fn command(&self, bin: &Path) -> Command {
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

    fn spaces_snapshot(&self, root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let spaces = root.join("spaces");
        let mut entries: Vec<_> = std::fs::read_dir(&spaces)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        entries
            .into_iter()
            .map(|dir| {
                let meta = std::fs::read(dir.join("meta.json")).unwrap_or_default();
                (dir, meta)
            })
            .collect()
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

/// Golden journey: init → space create (auto-context) → context-free
/// form/entry/search → reopen resolves the same context.
#[test]
fn golden_journey_needs_no_repeated_space_path() {
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
    let created = sandbox
        .command(&bin)
        .args(["space", "create", "demo"])
        .output()
        .unwrap();
    assert_success(&created, "space create demo");

    // No Space path or UID from here on.
    for args in [
        vec!["form", "list"],
        vec!["entry", "list"],
        vec!["search", "keyword", "planning"],
        vec!["space", "get"],
    ] {
        assert_success(
            &sandbox.command(&bin).args(&args).output().unwrap(),
            &format!("{} (context-first)", args.join(" ")),
        );
    }

    // Temporary override does not change the selection.
    assert_success(
        &sandbox
            .command(&bin)
            .args(["--context", "demo", "entry", "list"])
            .output()
            .unwrap(),
        "--context override",
    );

    // Reopen (fresh process) resolves the same context.
    let current = sandbox
        .command(&bin)
        .args(["context", "current"])
        .output()
        .unwrap();
    assert_success(&current, "reopen context current");
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "demo");
}

/// Knowledge safety: config mutations and config deletion never change Spaces.
#[test]
fn config_operations_never_mutate_knowledge() {
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
    let root = crate_root_of(&sandbox);
    let before = sandbox.spaces_snapshot(&root);
    assert!(!before.is_empty(), "expected a created Space on disk");

    // Config-only mutations.
    assert_success(
        &sandbox
            .command(&bin)
            .args([
                "context",
                "add",
                "spare",
                "--connection",
                "local",
                "--space",
                "019f9999-9999-7abc-8def-999999999999",
            ])
            .output()
            .unwrap(),
        "context add",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args(["context", "use", "spare"])
            .output()
            .unwrap(),
        "context use",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args(["context", "use", "demo"])
            .output()
            .unwrap(),
        "context use back",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args(["context", "remove", "spare"])
            .output()
            .unwrap(),
        "context remove",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args([
                "config",
                "connection",
                "add",
                "extra",
                "--type",
                "backend",
                "--url",
                "https://ugoite.example.com",
            ])
            .output()
            .unwrap(),
        "connection add",
    );
    assert_success(
        &sandbox
            .command(&bin)
            .args(["config", "connection", "remove", "extra"])
            .output()
            .unwrap(),
        "connection remove",
    );
    assert_eq!(
        sandbox.spaces_snapshot(&root),
        before,
        "config mutations changed Space storage"
    );

    // Deleting every config file leaves Knowledge intact.
    std::fs::remove_file(sandbox.home.join(".ugoite").join("config.toml")).unwrap();
    assert_eq!(
        sandbox.spaces_snapshot(&root),
        before,
        "config deletion changed Space storage"
    );
}

/// Legacy migrate: refuse on existing canonical, migrate endpoint URLs.
#[test]
fn legacy_migrate_produces_canonical_connections() {
    let sandbox = Sandbox::fresh();
    let bin = ugoite_bin();

    std::fs::create_dir_all(sandbox.home.join(".ugoite")).unwrap();
    let legacy_path = legacy_path_of(&sandbox);
    std::fs::write(
        &legacy_path,
        serde_json::json!({
            "mode": "backend",
            "backend_url": "https://ugoite.example.com",
            "api_url": "https://ugoite.example.com/api",
        })
        .to_string(),
    )
    .unwrap();

    let migrated = sandbox
        .command(&bin)
        .args(["config", "migrate"])
        .env("UGOITE_CLI_CONFIG_PATH", &legacy_path)
        .output()
        .unwrap();
    assert_success(&migrated, "config migrate");
    let stdout = String::from_utf8_lossy(&migrated.stdout);
    assert!(stdout.contains("backend"), "{stdout}");

    // Second run refuses to overwrite.
    let again = sandbox
        .command(&bin)
        .args(["config", "migrate"])
        .env("UGOITE_CLI_CONFIG_PATH", &legacy_path)
        .output()
        .unwrap();
    assert!(!again.status.success(), "migrate must not overwrite");
    assert!(
        String::from_utf8_lossy(&again.stderr).contains("Refusing"),
        "refusal must name the existing file: {}",
        String::from_utf8_lossy(&again.stderr),
    );
}

fn crate_root_of(sandbox: &Sandbox) -> PathBuf {
    let text = std::fs::read_to_string(sandbox.home.join(".ugoite").join("config.toml")).unwrap();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(root) = trimmed.strip_prefix("root = ") {
            return PathBuf::from(root.trim_matches('"'));
        }
    }
    panic!("migrated config has no core root");
}

fn legacy_path_of(sandbox: &Sandbox) -> PathBuf {
    sandbox.legacy_config.clone()
}
