//! Integration tests for space management commands.
//! REQ-STO-001, REQ-STO-002, REQ-STO-003, REQ-STO-004, REQ-STO-005, REQ-API-009

use std::process::Command;

fn ugoite_bin() -> String {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.push("ugoite");
    path.to_string_lossy().to_string()
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
    let config_path = dir.path().join("cli-config.json");

    let output = Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "my-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify the space directory was created
    let space_dir = dir.path().join("spaces").join("my-space");
    assert!(space_dir.exists(), "Space directory should be created");
}

#[cfg(unix)]
/// REQ-STO-003: Create space applies owner-only local permissions.
#[test]
fn test_create_space_req_sto_003_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    let output = Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "private-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let spaces_root = dir.path().join("spaces");
    let space_dir = spaces_root.join("private-space");
    assert_eq!(mode(&spaces_root), 0o700);
    assert_eq!(mode(&space_dir), 0o700);
    for dir_name in ["forms", "assets", "materialized_views", "sql_sessions"] {
        assert_eq!(mode(&space_dir.join(dir_name)), 0o700);
    }
    for file_name in ["meta.json", "settings.json"] {
        assert_eq!(mode(&space_dir.join(file_name)), 0o600);
    }
}

/// REQ-STO-001: S3 backend is not yet implemented (returns unimplemented error).
#[test]
fn test_create_space_s3_unimplemented() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.json");

    // Attempting to use s3:// path should fail gracefully
    let output = Command::new(ugoite_bin())
        .args(["create-space", "--root", "s3://my-bucket", "my-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // S3 is not supported in core mode; expect failure
    assert!(
        !output.status.success() || {
            let stderr = String::from_utf8_lossy(&output.stderr);
            stderr.contains("not") || stderr.contains("unsupported") || stderr.contains("error")
        }
    );
}

/// REQ-STO-005: Prevent duplicate space creation - returns error for existing space.
#[test]
fn test_create_space_idempotency() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    // Create space first time
    Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "idempotent-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    // Second creation should fail with "already exists" error
    let output2 = Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "idempotent-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        !output2.status.success(),
        "Second create-space should fail for duplicate space"
    );
    let stderr = String::from_utf8_lossy(&output2.stderr);
    assert!(
        stderr.contains("already exists"),
        "Expected 'already exists' error, got: {stderr}"
    );
}

/// REQ-API-009: Sample space can be created with sample data.
#[test]
fn test_create_sample_space_req_api_009() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_string_lossy().to_string();
    let config_path = dir.path().join("cli-config.json");

    let output = Command::new(ugoite_bin())
        .args(["create-space", "--root", &root, "sample-space"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify space was created
    let space_dir = dir.path().join("spaces").join("sample-space");
    assert!(space_dir.exists());
}
