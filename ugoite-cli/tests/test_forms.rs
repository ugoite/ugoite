//! Integration tests for form schema management via ugoite-core.
//! REQ-FORM-001, REQ-FORM-002

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

/// Create a space and an Entry form, return (root, space_path, config_path).
fn setup_space_with_form(
    dir: &tempfile::TempDir,
    space_id: &str,
) -> (String, String, std::path::PathBuf) {
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

    (root, space_path, config_path)
}

/// REQ-FORM-001: List available column types.
#[test]
fn test_list_column_types() {
    // list-types does not require a space path
    let output = Command::new(ugoite_bin())
        .args(["form", "list-types"])
        .output()
        .expect("failed to execute");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "Expected non-empty type list");
}

/// REQ-FORM-002: Add a column to a form with default value.
#[test]
fn test_migrate_form_add_column_with_default() {
    let dir = tempfile::tempdir().unwrap();
    let (_root, space_path, config_path) = setup_space_with_form(&dir, "form-space");

    // Get the form that was just created
    let get_output = Command::new(ugoite_bin())
        .args(["form", "get", &space_path, "Entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        get_output.status.success(),
        "get stderr: {}",
        String::from_utf8_lossy(&get_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&get_output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("should be JSON");
    assert_eq!(v.get("name").and_then(|x| x.as_str()), Some("Entry"));
}

/// REQ-FORM-002: Remove a column from a form.
#[test]
fn test_migrate_form_remove_column() {
    let dir = tempfile::tempdir().unwrap();
    let (root, space_path, config_path) = setup_space_with_form(&dir, "form-space");

    // Add a second column to the form
    let form_file2 = dir.path().join("entry-form2.json");
    std::fs::write(
        &form_file2,
        r#"{"name":"Entry","fields":{"Body":{"type":"markdown"},"Status":{"type":"text"}}}"#,
    )
    .unwrap();

    let update_output = Command::new(ugoite_bin())
        .args(["form", "update", &space_path, form_file2.to_str().unwrap()])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        update_output.status.success(),
        "update stderr: {}",
        String::from_utf8_lossy(&update_output.stderr)
    );

    // Verify form still accessible
    let get_output = Command::new(ugoite_bin())
        .args(["form", "get", &format!("{root}/spaces/form-space"), "Entry"])
        .env("UGOITE_CLI_CONFIG_PATH", &config_path)
        .output()
        .expect("failed to execute");

    assert!(
        get_output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&get_output.stderr)
    );
}
