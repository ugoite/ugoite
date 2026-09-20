//! PR-04 `config current` machine output contract.
//!
//! Canonical config present: stable section order (`Config sources:`,
//! `Write target:`, `Current context:`, `Connection:`, `Root:` local or
//! `Endpoint:` remote, `Space:`, `Credential:`) carrying the selected
//! context's connection, immutable Space UID, and credential name (never
//! secrets).

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

struct Sandbox {
    home: PathBuf,
    work: PathBuf,
    _home_dir: tempfile::TempDir,
    _work_dir: tempfile::TempDir,
}

impl Sandbox {
    fn fresh() -> Self {
        let home_dir = tempfile::tempdir().unwrap();
        let work_dir = tempfile::tempdir().unwrap();
        Self {
            home: home_dir.path().to_path_buf(),
            work: work_dir.path().to_path_buf(),
            _home_dir: home_dir,
            _work_dir: work_dir,
        }
    }

    fn command(&self, bin: &std::path::Path) -> Command {
        let mut command = Command::new(bin);
        command.env("HOME", &self.home).current_dir(&self.work);
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

fn section_index(stdout: &str, section: &str) -> usize {
    stdout
        .find(section)
        .unwrap_or_else(|| panic!("missing section {section:?}: {stdout}"))
}

/// Canonical shape: sections in order, carrying connection, UID, credential.
#[test]
fn config_current_pins_canonical_machine_shape() {
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

    let output = sandbox
        .command(&bin)
        .args(["config", "current"])
        .output()
        .unwrap();
    assert_success(&output, "config current");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    // Stable section order.
    let order = [
        "Config sources:",
        "Write target:",
        "Current context:",
        "Connection:",
        "Space:",
        "Credential:",
    ];
    let mut previous = 0;
    for section in order {
        let index = section_index(&stdout, section);
        assert!(
            index >= previous,
            "section {section:?} out of order: {stdout}"
        );
        previous = index;
    }
    // Local selection reports its root between Connection and Space.
    let connection = section_index(&stdout, "Connection:");
    let space = section_index(&stdout, "Space:");
    let root = section_index(&stdout, "Root:");
    assert!(
        connection < root && root < space,
        "local selection must report Root: between Connection: and Space:: {stdout}"
    );

    // Values: context name, connection, UID, credential slot.
    assert!(stdout.contains("demo"), "{stdout}");
    assert!(stdout.contains("local"), "{stdout}");
    let space_line = stdout
        .lines()
        .skip_while(|line| !line.starts_with("Space:"))
        .nth(1)
        .expect("Space: value line");
    assert_eq!(
        space_line.trim().len(),
        36,
        "Space: must carry the immutable UID: {stdout}"
    );
    assert!(
        uuid::Uuid::parse_str(space_line.trim()).is_ok(),
        "Space: must be a UID: {stdout}"
    );
    let credential_line = stdout
        .lines()
        .skip_while(|line| !line.starts_with("Credential:"))
        .nth(1)
        .expect("Credential: value line");
    assert!(
        !credential_line.trim().is_empty(),
        "Credential: must name the profile or none: {stdout}"
    );

    // One-invocation override resolves without mutating the selection.
    let overridden = sandbox
        .command(&bin)
        .args(["--context", "demo", "config", "current"])
        .output()
        .unwrap();
    assert_success(&overridden, "--context override config current");
    let overridden_stdout = String::from_utf8_lossy(&overridden.stdout).to_string();
    assert!(overridden_stdout.contains("demo"), "{overridden_stdout}");

    let current = sandbox
        .command(&bin)
        .args(["context", "current"])
        .output()
        .unwrap();
    assert_success(&current, "context current");
    assert_eq!(String::from_utf8_lossy(&current.stdout).trim(), "demo");
}
