//! Integration tests for space management commands.
//! REQ-STO-001, REQ-STO-002, REQ-STO-003, REQ-STO-004, REQ-STO-005, REQ-API-009

use support::Command;

mod support;

fn created_space_dir(root: &std::path::Path, output: &std::process::Output) -> std::path::PathBuf {
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let space_id = result["space"]["space_uid"].as_str().unwrap();
    assert!(uuid::Uuid::parse_str(space_id).is_ok());
    root.join("spaces").join(space_id)
}

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

fn init_canonical_config(config_path: &std::path::Path, root: &str) {
    let init = Command::new(ugoite_bin())
        .args(["--config", config_path.to_str().unwrap(), "config", "init"])
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("config init");
    assert!(init.status.success());
    let connection = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "config",
            "connection",
            "set",
            "local",
            "--type",
            "core",
            "--root",
            root,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("connection set");
    assert!(connection.status.success());
}

fn create_space(config_path: &std::path::Path, slug: &str) -> std::process::Output {
    Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "space",
            "create",
            slug,
        ])
        .env("UGOITE_CLI_CONFIG_PATH", config_path)
        .output()
        .expect("space create")
}

/// The connection command must exercise the shared Rust storage probe for a
/// local backend instead of inferring a mode from the URI.
#[test]
fn test_storage_connection_cli_probes_local_backend() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    let payload = serde_json::json!({
        "uri": format!("file://{}", dir.path().display()),
    })
    .to_string();

    let output = Command::new(ugoite_bin())
        .args(["space", "test-connection", &payload])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result, serde_json::json!({"status": "ok", "mode": "local"}));
}

#[test]
fn test_storage_connection_cli_probes_memory_backend() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    let payload = serde_json::json!({"uri": "memory://cli-probe"}).to_string();

    let output = Command::new(ugoite_bin())
        .args(["space", "test-connection", &payload])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        result,
        serde_json::json!({"status": "ok", "mode": "memory"})
    );
}

#[test]
fn test_storage_connection_cli_rejects_unsupported_backend() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");
    let payload = serde_json::json!({"uri": "ftp://example.test/data"}).to_string();

    let output = Command::new(ugoite_bin())
        .args(["space", "test-connection", &payload])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "STORAGE_MUTATION_UNAVAILABLE");
    assert_eq!(error["error"]["kind"], "unimplemented");
}

#[cfg(unix)]
fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// REQ-STO-001, REQ-STO-002: Create space scaffolding at local path.
#[test]
fn test_create_space_scaffolding() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    let output = create_space(&config_path, "my-space");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the space directory was created
    let space_dir = created_space_dir(dir.path(), &output);
    assert!(space_dir.exists(), "Space directory should be created");

    let settings: serde_json::Value =
        serde_json::from_slice(&std::fs::read(space_dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["default_form"], "Entry");

    let forms_output = Command::new(ugoite_bin())
        .args(["--config", config_path.to_str().unwrap(), "form", "list"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to list starter forms");
    assert!(
        forms_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&forms_output.stderr)
    );
    let forms: serde_json::Value = serde_json::from_slice(&forms_output.stdout).unwrap();
    assert!(forms
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["name"] == "Entry")));
}

#[cfg(unix)]
/// REQ-STO-003: Create space applies owner-only local permissions.
#[test]
fn test_create_space_req_sto_003_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    let output = create_space(&config_path, "private-space");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let spaces_root = dir.path().join("spaces");
    let space_dir = created_space_dir(dir.path(), &output);
    assert_eq!(mode(&spaces_root), 0o700);
    assert_eq!(mode(&space_dir), 0o700);
    for dir_name in ["forms", "assets"] {
        assert_eq!(mode(&space_dir.join(dir_name)), 0o700);
    }
    for file_name in ["meta.json", "settings.json"] {
        assert_eq!(mode(&space_dir.join(file_name)), 0o600);
    }
}

/// REQ-STO-005: Core-mode Space create is idempotent - retrying the same slug
/// converges to the same durable Space identity instead of failing.
#[test]
fn test_create_space_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    // Create space first time
    let output1 = create_space(&config_path, "idempotent-space");
    assert!(
        output1.status.success(),
        "First space create should succeed: {}",
        String::from_utf8_lossy(&output1.stderr)
    );
    let first: serde_json::Value =
        serde_json::from_slice(&output1.stdout).expect("first create prints JSON");
    assert_eq!(
        first["space"]["slug"],
        serde_json::json!("idempotent-space")
    );

    // Second creation with the same slug converges to the same Space.
    let output2 = create_space(&config_path, "idempotent-space");

    assert!(
        output2.status.success(),
        "Second space create should converge to the existing Space: {}",
        String::from_utf8_lossy(&output2.stderr)
    );
    let second: serde_json::Value =
        serde_json::from_slice(&output2.stdout).expect("retry prints JSON");
    assert_eq!(
        second["space"]["slug"],
        serde_json::json!("idempotent-space")
    );
    assert_eq!(second["space"]["space_uid"], first["space"]["space_uid"]);
}

/// `space create` keeps the positional slug as the lookup key while `--name`
/// seeds an independent display name in core mode.
#[test]
fn test_create_space_with_independent_display_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    let output = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "space",
            "create",
            "team-notes",
            "--name",
            "Team Notes",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("create prints JSON");
    assert_eq!(created["space"]["slug"], serde_json::json!("team-notes"));

    let space_dir = created_space_dir(dir.path(), &output);
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(space_dir.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["slug"], serde_json::json!("team-notes"));
    assert_eq!(meta["name"], serde_json::json!("Team Notes"));
}

/// `space create --name "   "` fails before any write: no Space directory
/// is created and the retry surface stays clean.
#[test]
fn test_create_space_rejects_whitespace_display_name_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    let output = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "space",
            "create",
            "blank-name",
            "--name",
            "   ",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        !output.status.success(),
        "whitespace-only display name must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Space display name must not be empty"),
        "unexpected stderr: {stderr}"
    );
    assert!(
        !dir.path().join("spaces").exists(),
        "failed create must not write any Space state"
    );
}

/// `space create --name "Alpha"` stores Alpha; retrying the same slug with
/// `--name "Beta"` converges to the existing Space without renaming.
#[test]
fn test_create_space_retry_does_not_rename() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    let first = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "space",
            "create",
            "named-space",
            "--name",
            "Alpha",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        first.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let created: serde_json::Value =
        serde_json::from_slice(&first.stdout).expect("create prints JSON");
    assert_eq!(created["context"]["created"], serde_json::json!(true));

    let retry = Command::new(ugoite_bin())
        .args([
            "--config",
            config_path.to_str().unwrap(),
            "space",
            "create",
            "named-space",
            "--name",
            "Beta",
        ])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");
    assert!(
        retry.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let converged: serde_json::Value =
        serde_json::from_slice(&retry.stdout).expect("retry prints JSON");
    assert_eq!(converged["context"]["created"], serde_json::json!(true));
    assert_eq!(
        converged["space"]["space_uid"],
        created["space"]["space_uid"]
    );

    let space_dir = created_space_dir(dir.path(), &first);
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(space_dir.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["name"], serde_json::json!("Alpha"));
}

/// `space create` without `--name` keeps the slug as the display name.
#[test]
fn test_create_space_defaults_display_name_to_slug() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    let output = create_space(&config_path, "plain-slug");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("create prints JSON");
    assert_eq!(created["space"]["slug"], serde_json::json!("plain-slug"));

    let space_dir = created_space_dir(dir.path(), &output);
    let meta: serde_json::Value =
        serde_json::from_slice(&std::fs::read(space_dir.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta["slug"], serde_json::json!("plain-slug"));
    assert_eq!(meta["name"], serde_json::json!("plain-slug"));
}

/// REQ-API-009: Sample space can be created with sample data.
#[test]
fn test_create_sample_space_req_api_009() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.toml");
    init_canonical_config(&config_path, &root);

    let output = create_space(&config_path, "sample-space");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify space was created
    let space_dir = created_space_dir(dir.path(), &output);
    assert!(space_dir.exists());
}

/// REQ-API-009: Direct sample-data CLI should show progress and create the target space.
#[test]
fn test_sample_data_progress_req_api_009() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    let output = Command::new(ugoite_bin())
        .args([
            "space",
            "sample-data",
            &root,
            "sample-progress",
            "--scenario",
            "lab-qa",
            "--entry-count",
            "10",
            "--seed",
            "7",
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
    assert!(
        stdout.contains("\"created\": true"),
        "Expected created JSON output, got: {stdout}"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Seed progress ["),
        "Expected progress output, got: {stderr}"
    );
    assert!(
        stderr.contains("(10/10) Completed"),
        "Expected completed progress output, got: {stderr}"
    );

    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["slug"], "sample-progress");
    let space_dir = dir
        .path()
        .join("spaces")
        .join(result["id"].as_str().unwrap());
    assert!(
        space_dir.exists(),
        "Sample data command should create the space"
    );
}
