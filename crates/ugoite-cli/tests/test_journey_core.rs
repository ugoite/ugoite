//! JOURNEY-KNOWLEDGE-001 through the local/core CLI.
//!
//! Evidence identity: surface=cli, transport=core/local. This runs the same
//! scenario as e2e/knowledge-journey.test.ts (Space create -> Form establish
//! -> Entry create -> Entry edit -> Search -> History -> Restore -> Reopen)
//! and asserts the same durable postconditions through canonical reads.
//! CLI stdout wording is never compared; only exit status and the returned
//! durable state matter. Form establish intentionally drives `form update`,
//! which is the upsert path behind a weaker name.

use std::process::Command;
use std::process::Output;

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

fn run_cli(config: &std::path::Path, args: &[&str]) -> Output {
    let bin = ugoite_bin();
    if !config.exists() && args.first().copied() != Some("config") {
        let initialized = Command::new(&bin)
            .args(["--config", config.to_str().unwrap(), "config", "init"])
            .output()
            .expect("initialize canonical config");
        assert!(initialized.status.success(), "config init failed");
        let configured = Command::new(&bin)
            .args([
                "--config",
                config.to_str().unwrap(),
                "config",
                "connection",
                "set",
                "local",
                "--type",
                "core",
                "--root",
                config.parent().unwrap().to_str().unwrap(),
            ])
            .output()
            .expect("configure canonical core connection");
        assert!(configured.status.success(), "connection set failed");
    }
    let mut canonical = vec![
        "--config".to_string(),
        config.to_string_lossy().into_owned(),
    ];
    canonical.extend(args.iter().map(|arg| (*arg).to_string()));
    Command::new(bin)
        .args(canonical)
        .output()
        .expect("run ugoite")
}

fn stdout_json(output: &Output, what: &str) -> serde_json::Value {
    assert!(
        output.status.success(),
        "{what} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("{what} stdout is not JSON: {stdout}"))
}

fn contains_string(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text == needle,
        serde_json::Value::Array(items) => items.iter().any(|item| contains_string(item, needle)),
        serde_json::Value::Object(fields) => {
            fields.values().any(|item| contains_string(item, needle))
        }
        _ => false,
    }
}

fn contains_substring(value: &serde_json::Value, needle: &str) -> bool {
    match value {
        serde_json::Value::String(text) => text.contains(needle),
        serde_json::Value::Array(items) => {
            items.iter().any(|item| contains_substring(item, needle))
        }
        serde_json::Value::Object(fields) => {
            fields.values().any(|item| contains_substring(item, needle))
        }
        _ => false,
    }
}

fn revision_ids(history: &serde_json::Value) -> Vec<String> {
    history
        .get("revisions")
        .and_then(|revisions| revisions.as_array())
        .unwrap_or_else(|| panic!("history has no revisions array: {history}"))
        .iter()
        .map(|revision| {
            revision
                .get("revision_id")
                .and_then(|id| id.as_str())
                .unwrap_or_else(|| panic!("revision has no revision_id: {revision}"))
                .to_string()
        })
        .collect()
}

/// JOURNEY-KNOWLEDGE-001 on surface=cli transport=core/local reaches the
/// same durable Knowledge outcome as the Frontend evidence.
#[test]
fn test_journey_cli_core_local_durable_outcome() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.toml");
    let space_id = "journey-core-space";
    let form_name = "JourneyCoreForm";
    let needle = "journey-core-needle";
    let entry_id = "journey-core-entry";

    // Space create: a durable Space comes into existence.
    let output = run_cli(&config_path, &["space", "create", space_id]);
    assert!(
        output.status.success(),
        "space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Form establish via `form update`: the upsert path behind a weaker name.
    let form_file = dir.path().join("journey-core-form.json");
    std::fs::write(
        &form_file,
        format!(
            "{{\"name\":\"{form_name}\",\"version\":1,\"template\":\"# {form_name}\\n\\n## Status\\n\\n## Body\\n\",\"fields\":{{\"Status\":{{\"type\":\"string\",\"required\":true}},\"Body\":{{\"type\":\"markdown\"}}}}}}"
        ),
    )
    .unwrap();
    let output = run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form = stdout_json(
        &run_cli(&config_path, &["form", "get", form_name]),
        "form get",
    );
    assert_eq!(
        form.get("name").and_then(|name| name.as_str()),
        Some(form_name)
    );
    assert_eq!(
        form.pointer("/fields/Status/type").and_then(|t| t.as_str()),
        Some("string")
    );
    assert_eq!(
        form.pointer("/fields/Status/required")
            .and_then(|r| r.as_bool()),
        Some(true)
    );

    // Entry create appends exactly one revision.
    let created = stdout_json(
        &run_cli(
            &config_path,
            &[
                "entry",
                "create",
                entry_id,
                "--form",
                form_name,
                "--field",
                &format!("Status={needle}"),
                "--field",
                "Body=journey core v1",
            ],
        ),
        "entry create",
    );
    assert!(contains_string(&created, entry_id));
    let create_change_id = created
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("create returns durable change_id")
        .to_string();
    let history = stdout_json(
        &run_cli(&config_path, &["entry", "history", entry_id]),
        "entry history after create",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 1);
    assert_eq!(
        history["revisions"][0]["change_id"],
        serde_json::Value::String(create_change_id)
    );
    let rev1 = ids[0].clone();

    // Entry edit appends a revision; a stale parent conflicts.
    let output = run_cli(
        &config_path,
        &[
            "entry",
            "update",
            entry_id,
            "--field",
            &format!("Status={needle}"),
            "--field",
            "Body=journey core v2",
            "--parent-revision-id",
            &rev1,
        ],
    );
    assert!(
        output.status.success(),
        "entry update failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = stdout_json(&output, "entry update");
    let update_change_id = updated
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("update returns durable change_id")
        .to_string();
    let history = stdout_json(
        &run_cli(&config_path, &["entry", "history", entry_id]),
        "entry history after edit",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&rev1));
    let rev2 = ids.into_iter().find(|id| id != &rev1).expect("rev2");
    let stale = run_cli(
        &config_path,
        &[
            "entry",
            "update",
            entry_id,
            "--field",
            &format!("Status={needle}"),
            "--field",
            "Body=journey core v2",
            "--parent-revision-id",
            &rev1,
        ],
    );
    assert!(
        !stale.status.success(),
        "stale parent revision must conflict instead of overwriting"
    );

    // Search finds the updated durable Entry.
    let results = stdout_json(
        &run_cli(&config_path, &["search", "keyword", needle]),
        "search keyword",
    );
    assert!(
        contains_string(&results, entry_id),
        "search must find the updated entry: {results}"
    );

    // Restore appends a new revision replaying rev1; history never shortens.
    let output = run_cli(&config_path, &["entry", "restore", entry_id, &rev1]);
    assert!(
        output.status.success(),
        "entry restore failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let history = stdout_json(
        &run_cli(&config_path, &["entry", "history", entry_id]),
        "entry history after restore",
    );
    let ids = revision_ids(&history);
    assert_eq!(ids.len(), 3);
    assert!(ids.contains(&rev1));
    assert!(ids.contains(&rev2));
    let rev3 = ids
        .into_iter()
        .find(|id| id != &rev1 && id != &rev2)
        .expect("rev3");
    assert_eq!(
        history["revisions"]
            .as_array()
            .expect("history revisions")
            .iter()
            .find(|revision| revision["revision_id"] == rev2)
            .expect("updated revision")
            .get("change_id")
            .and_then(|id| id.as_str()),
        Some(update_change_id.as_str())
    );
    let restore = stdout_json(&output, "entry restore");
    let restore_change_id = restore
        .get("change_id")
        .and_then(|id| id.as_str())
        .expect("restore returns durable change_id")
        .to_string();
    assert_eq!(
        history["revisions"]
            .as_array()
            .expect("history revisions")
            .iter()
            .find(|revision| revision["revision_id"] == rev3)
            .expect("restored revision")
            .get("change_id")
            .and_then(|id| id.as_str()),
        Some(restore_change_id.as_str())
    );
    let revision = stdout_json(
        &run_cli(&config_path, &["entry", "revision", entry_id, &rev3]),
        "entry revision after restore",
    );
    assert_eq!(
        revision.get("revision_id").and_then(|id| id.as_str()),
        Some(rev3.as_str())
    );
    assert!(
        contains_substring(&revision, "journey core v1"),
        "restored revision must replay rev1 content: {revision}"
    );

    // Reopen: fresh processes read identical durable state.
    let space = stdout_json(
        &run_cli(&config_path, &["space", "get"]),
        "space get on reopen",
    );
    assert!(contains_string(&space, space_id));
    let history = stdout_json(
        &run_cli(&config_path, &["entry", "history", entry_id]),
        "entry history on reopen",
    );
    assert_eq!(revision_ids(&history).len(), 3);
    let results = stdout_json(
        &run_cli(&config_path, &["search", "keyword", needle]),
        "search keyword on reopen",
    );
    assert!(contains_string(&results, entry_id));
}

/// The CLI supplies the current revision when the caller omits a parent for
/// structured updates. Explicit parents use the same write path, and a stale
/// parent remains a canonical conflict.
#[test]
fn test_cli_core_entry_update_parent_revision_matrix() {
    let space = setup_parity_space(r#"{"Status":{"type":"string"},"Body":{"type":"markdown"}}"#);

    let structured_created = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "create",
                "parent-matrix-structured",
                "--form",
                space.form_name,
                "--field",
                "Body=structured v1",
            ],
        ),
        "structured matrix create",
    );
    assert!(contains_string(
        &structured_created,
        "parent-matrix-structured"
    ));
    let structured_history = stdout_json(
        &run_cli(
            &space.config_path,
            &["entry", "history", "parent-matrix-structured"],
        ),
        "structured matrix history after create",
    );
    let structured_rev1 = revision_ids(&structured_history)[0].clone();

    let explicit = run_cli(
        &space.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-structured",
            "--field",
            "Body=structured explicit",
            "--parent-revision-id",
            &structured_rev1,
        ],
    );
    assert!(
        explicit.status.success(),
        "explicit structured update failed"
    );
    let structured_history = stdout_json(
        &run_cli(
            &space.config_path,
            &["entry", "history", "parent-matrix-structured"],
        ),
        "structured matrix history after explicit update",
    );
    let structured_rev2 = revision_ids(&structured_history)
        .into_iter()
        .find(|revision| revision != &structured_rev1)
        .expect("structured rev2");
    let structured_revision = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-structured",
                &structured_rev2,
            ],
        ),
        "structured matrix revision after explicit update",
    );
    assert_eq!(structured_revision["parent_revision_id"], structured_rev1);

    let omitted = run_cli(
        &space.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-structured",
            "--field",
            "Body=structured omitted",
        ],
    );
    assert!(omitted.status.success(), "omitted structured update failed");
    let structured_history = stdout_json(
        &run_cli(
            &space.config_path,
            &["entry", "history", "parent-matrix-structured"],
        ),
        "structured matrix history after omitted update",
    );
    let structured_rev3 = revision_ids(&structured_history)
        .into_iter()
        .find(|revision| revision != &structured_rev1 && revision != &structured_rev2)
        .expect("structured rev3");
    let structured_revision = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-structured",
                &structured_rev3,
            ],
        ),
        "structured matrix revision after omitted update",
    );
    assert_eq!(structured_revision["parent_revision_id"], structured_rev2);

    let created = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "create",
                "parent-matrix-canonical",
                "--form",
                space.form_name,
                "--field",
                "Body=canonical v1",
            ],
        ),
        "canonical matrix create",
    );
    assert!(contains_string(&created, "parent-matrix-canonical"));
    let history = stdout_json(
        &run_cli(
            &space.config_path,
            &["entry", "history", "parent-matrix-canonical"],
        ),
        "canonical matrix history after create",
    );
    let canonical_rev1 = revision_ids(&history)[0].clone();

    let explicit = run_cli(
        &space.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-canonical",
            "--field",
            "Body=canonical v2",
            "--parent-revision-id",
            &canonical_rev1,
        ],
    );
    assert!(
        explicit.status.success(),
        "explicit canonical update failed"
    );
    let history = stdout_json(
        &run_cli(
            &space.config_path,
            &["entry", "history", "parent-matrix-canonical"],
        ),
        "canonical matrix history after explicit update",
    );
    let canonical_rev2 = revision_ids(&history)
        .into_iter()
        .find(|revision| revision != &canonical_rev1)
        .expect("canonical rev2");
    let revision = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-canonical",
                &canonical_rev2,
            ],
        ),
        "canonical matrix revision after explicit update",
    );
    assert_eq!(revision["parent_revision_id"], canonical_rev1);

    let omitted = run_cli(
        &space.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-canonical",
            "--field",
            "Body=canonical v3",
        ],
    );
    assert!(omitted.status.success(), "omitted canonical update failed");
    let history = stdout_json(
        &run_cli(
            &space.config_path,
            &["entry", "history", "parent-matrix-canonical"],
        ),
        "canonical matrix history after omitted update",
    );
    let canonical_rev3 = revision_ids(&history)
        .into_iter()
        .find(|revision| revision != &canonical_rev1 && revision != &canonical_rev2)
        .expect("canonical rev3");
    let revision = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "revision",
                "parent-matrix-canonical",
                &canonical_rev3,
            ],
        ),
        "canonical matrix revision after omitted update",
    );
    assert_eq!(revision["parent_revision_id"], canonical_rev2);

    let stale = run_cli(
        &space.config_path,
        &[
            "entry",
            "update",
            "parent-matrix-canonical",
            "--field",
            "Body=canonical v3",
            "--parent-revision-id",
            &canonical_rev1,
        ],
    );
    assert!(!stale.status.success(), "stale parent must conflict");
    assert!(String::from_utf8_lossy(&stale.stderr).contains("REVISION_CONFLICT"));
}

// --- Semantic parity corpus (surface=cli, transport=core/local) ---
//
// Each case asserts the same two things the remote corpus asserts: the
// machine-readable failure classification and the unchanged durable state.
// Presentation wording is never compared across surfaces.

struct ParitySpace {
    _dir: tempfile::TempDir,
    config_path: std::path::PathBuf,
    form_name: &'static str,
}

fn setup_parity_space(form_fields: &str) -> ParitySpace {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("cli-config.toml");
    let space_id = "parity-core-space";
    let form_name = "ParityCoreForm";

    let output = run_cli(&config_path, &["space", "create", space_id]);
    assert!(
        output.status.success(),
        "parity setup space create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let form_file = dir.path().join("parity-core-form.json");
    std::fs::write(
        &form_file,
        format!(
            "{{\"name\":\"{form_name}\",\"version\":1,\"template\":\"# {form_name}\\n\\n## Status\\n\\n## Body\\n\",\"fields\":{form_fields}}}"
        ),
    )
    .unwrap();
    let output = run_cli(
        &config_path,
        &["form", "update", form_file.to_str().unwrap()],
    );
    assert!(
        output.status.success(),
        "parity setup form establish failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    ParitySpace {
        _dir: dir,
        config_path,
        form_name,
    }
}

fn entry_absent(space: &ParitySpace, entry_id: &str) {
    let output = run_cli(&space.config_path, &["entry", "get", entry_id]);
    assert!(
        !output.status.success(),
        "rejected mutation must not persist an entry"
    );
}

/// Invalid field values are rejected with field-identifying validation
/// semantics and persist nothing.
#[test]
fn test_parity_core_invalid_field_rejected_without_mutation() {
    let space = setup_parity_space(
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Count\":{\"type\":\"double\"},\"Body\":{\"type\":\"markdown\"}}",
    );
    let output = run_cli(
        &space.config_path,
        &[
            "entry",
            "create",
            "parity-invalid",
            "--form",
            space.form_name,
            "--field",
            "Status=ok",
            "--field",
            "Count=not-a-number",
            "--field",
            "Body=x",
        ],
    );
    assert!(!output.status.success(), "mistyped field must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Entry form validation failed"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("Count"), "stderr: {stderr}");
    entry_absent(&space, "parity-invalid");
}

/// Missing required fields are rejected and persist nothing.
#[test]
fn test_parity_core_missing_required_rejected_without_mutation() {
    let space = setup_parity_space(
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
    );
    let output = run_cli(
        &space.config_path,
        &[
            "entry",
            "create",
            "parity-missing",
            "--form",
            space.form_name,
            "--field",
            "Body=x",
        ],
    );
    assert!(
        !output.status.success(),
        "missing required field must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Entry form validation failed"),
        "stderr: {stderr}"
    );
    assert!(stderr.contains("Status"), "stderr: {stderr}");
    entry_absent(&space, "parity-missing");
}

/// Stale parents conflict with 409-equivalent semantics and persist nothing.
#[test]
fn test_parity_core_stale_revision_conflicts_without_mutation() {
    let space = setup_parity_space(
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
    );
    let created = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "create",
                "parity-stale",
                "--form",
                space.form_name,
                "--field",
                "Status=ok",
                "--field",
                "Body=v1",
            ],
        ),
        "parity setup entry create",
    );
    assert!(contains_string(&created, "parity-stale"));
    let history = stdout_json(
        &run_cli(&space.config_path, &["entry", "history", "parity-stale"]),
        "parity setup history",
    );
    let rev1 = revision_ids(&history)[0].clone();
    let updated = run_cli(
        &space.config_path,
        &[
            "entry",
            "update",
            "parity-stale",
            "--field",
            "Status=ok",
            "--field",
            "Body=v2",
            "--parent-revision-id",
            &rev1,
        ],
    );
    assert!(updated.status.success());
    let stale = run_cli(
        &space.config_path,
        &[
            "entry",
            "update",
            "parity-stale",
            "--field",
            "Status=ok",
            "--field",
            "Body=v2",
            "--parent-revision-id",
            &rev1,
        ],
    );
    assert!(!stale.status.success(), "stale parent must conflict");
    let stderr = String::from_utf8_lossy(&stale.stderr);
    assert!(stderr.contains("Revision conflict"), "stderr: {stderr}");
    let history = stdout_json(
        &run_cli(&space.config_path, &["entry", "history", "parity-stale"]),
        "parity history after conflict",
    );
    assert_eq!(revision_ids(&history).len(), 2);
}

/// Restoring an unknown revision is rejected as not-found; history unchanged.
#[test]
fn test_parity_core_restore_unknown_revision_rejected_without_mutation() {
    let space = setup_parity_space(
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
    );
    let created = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "create",
                "parity-restore",
                "--form",
                space.form_name,
                "--field",
                "Status=ok",
                "--field",
                "Body=v1",
            ],
        ),
        "parity setup entry create",
    );
    assert!(contains_string(&created, "parity-restore"));
    let output = run_cli(
        &space.config_path,
        &[
            "entry",
            "restore",
            "parity-restore",
            "00000000-0000-0000-0000-000000000000",
        ],
    );
    assert!(
        !output.status.success(),
        "unknown revision must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not found"), "stderr: {stderr}");
    let history = stdout_json(
        &run_cli(&space.config_path, &["entry", "history", "parity-restore"]),
        "parity history after rejected restore",
    );
    assert_eq!(revision_ids(&history).len(), 1);
}

/// Unknown Forms are rejected with form-identifying classification.
#[test]
fn test_parity_core_missing_form_rejected_without_mutation() {
    let space = setup_parity_space(
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
    );
    let output = run_cli(
        &space.config_path,
        &[
            "entry",
            "create",
            "parity-noform",
            "--form",
            "NoSuchFormParity",
            "--field",
            "Status=ok",
            "--field",
            "Body=x",
        ],
    );
    assert!(!output.status.success(), "unknown form must be rejected");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Form not found: NoSuchFormParity"),
        "stderr: {stderr}"
    );
    entry_absent(&space, "parity-noform");
}

/// Deleting an Entry records a tombstone: current reads exclude it while
/// history retains every revision.
#[test]
fn test_parity_core_delete_tombstone_keeps_history() {
    let space = setup_parity_space(
        "{\"Status\":{\"type\":\"string\",\"required\":true},\"Body\":{\"type\":\"markdown\"}}",
    );
    let created = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "create",
                "parity-delete",
                "--form",
                space.form_name,
                "--field",
                "Status=ok",
                "--field",
                "Body=v1",
            ],
        ),
        "parity setup entry create",
    );
    assert!(contains_string(&created, "parity-delete"));
    let history = stdout_json(
        &run_cli(&space.config_path, &["entry", "history", "parity-delete"]),
        "parity setup history",
    );
    let before = revision_ids(&history).len();
    assert!(before >= 1);

    let deleted_output = run_cli(&space.config_path, &["entry", "delete", "parity-delete"]);
    assert!(
        deleted_output.status.success(),
        "entry delete failed: {}",
        String::from_utf8_lossy(&deleted_output.stderr)
    );
    let deleted = stdout_json(&deleted_output, "entry delete");
    assert!(
        deleted
            .get("change_id")
            .and_then(|id| id.as_str())
            .is_some(),
        "delete returns durable change_id: {deleted}"
    );

    let current = run_cli(&space.config_path, &["entry", "get", "parity-delete"]);
    assert!(
        !current.status.success(),
        "deleted entry must leave current reads"
    );

    let listed = stdout_json(
        &run_cli(&space.config_path, &["entry", "list"]),
        "parity entry list after delete",
    );
    assert!(
        !contains_string(&listed, "parity-delete"),
        "deleted entry must leave current listings: {listed}"
    );

    let history = stdout_json(
        &run_cli(&space.config_path, &["entry", "history", "parity-delete"]),
        "parity history after delete",
    );
    assert!(
        revision_ids(&history).len() >= before,
        "delete must not shorten history"
    );
}

/// `entry list` translates CLI options into the canonical EntryQuery and
/// keeps typed filters, ordered sorts, projection, and the traversal goal in
/// the shared query boundary.
#[test]
fn test_cli_entry_list_canonical_query_options() {
    let space = setup_parity_space(
        r#"{"Status":{"type":"string"},"Priority":{"type":"long"},"Body":{"type":"markdown"}}"#,
    );
    for (entry_id, status, priority) in [
        ("query-cli-open-high", "open", "3"),
        ("query-cli-open-low", "open", "2"),
        ("query-cli-closed-high", "closed", "4"),
    ] {
        let output = run_cli(
            &space.config_path,
            &[
                "entry",
                "create",
                entry_id,
                "--form",
                space.form_name,
                "--field",
                &format!("Status={status}"),
                "--field",
                &format!("Priority={priority}"),
                "--field",
                "Body=canonical query text",
            ],
        );
        assert!(
            output.status.success(),
            "entry create failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let listed = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "entry",
                "list",
                "--form",
                space.form_name,
                "--filter",
                "Status:eq=open",
                "--filter",
                "Priority:gte=2",
                "--sort",
                "Priority:desc",
                "--sort",
                "Status:asc",
                "--columns",
                "Status,Priority",
                "--limit",
                "2",
            ],
        ),
        "canonical entry list",
    );
    let rows = listed.as_array().expect("entry list returns rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], "query-cli-open-high");
    assert_eq!(rows[0]["properties"]["Status"], "open");
    assert_eq!(rows[0]["properties"]["Priority"], 3);
    assert_eq!(rows[1]["properties"]["Priority"], 2);
    assert!(rows.iter().all(|row| {
        row["properties"].get("Body").is_none()
            && row.get("id").and_then(|id| id.as_str()).is_some()
    }));

    let listed_id = rows[0]["id"].as_str().expect("stable entry id");
    let fetched = stdout_json(
        &run_cli(&space.config_path, &["entry", "get", listed_id]),
        "entry get from canonical list identity",
    );
    assert!(
        contains_string(&fetched, listed_id),
        "canonical list identity must round-trip through entry get: {fetched}"
    );

    let table = run_cli(
        &space.config_path,
        &[
            "entry",
            "--format",
            "table",
            "list",
            "--form",
            space.form_name,
        ],
    );
    assert!(
        table.status.success(),
        "table entry list failed: {}",
        String::from_utf8_lossy(&table.stderr)
    );
    let table_stdout = String::from_utf8_lossy(&table.stdout);
    assert!(table_stdout.contains("PREVIEW"), "{table_stdout}");
    assert!(
        !table_stdout.contains("query-cli-open-high"),
        "human table must not use raw Entry ID as its primary display: {table_stdout}"
    );
}

#[test]
fn test_cli_sql_query_and_count_use_stateless_local_contract() {
    let space = setup_parity_space(r#"{"Status":{"type":"string"}}"#);

    let form = stdout_json(
        &run_cli(&space.config_path, &["form", "get", space.form_name]),
        "local SQL form lookup",
    );
    let form_id = form["id"].as_str().expect("form id").replace('-', "");
    let relation = format!("form_{form_id}");
    let sql = format!("SELECT _ugoite_id FROM \"{relation}\" ORDER BY _ugoite_id");
    let created = run_cli(
        &space.config_path,
        &[
            "entry",
            "create",
            "sql-local-entry",
            "--form",
            space.form_name,
            "--field",
            "Status=open",
        ],
    );
    assert!(created.status.success(), "local SQL entry setup failed");
    let created = run_cli(
        &space.config_path,
        &[
            "entry",
            "create",
            "sql-local-entry-2",
            "--form",
            space.form_name,
            "--field",
            "Status=closed",
        ],
    );
    assert!(
        created.status.success(),
        "second local SQL entry setup failed"
    );

    let page = stdout_json(
        &run_cli(&space.config_path, &["sql", "query", &sql, "--limit", "1"]),
        "local stateless SQL query",
    );
    assert_eq!(page["columns"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["rows"].as_array().map(Vec::len), Some(1));
    assert_eq!(page["has_more"], true);
    let first_id = page["rows"][0]["_ugoite_id"]
        .as_str()
        .expect("first SQL row id")
        .to_string();
    let continuation = page["next"].as_str().expect("SQL continuation");

    let next_page = stdout_json(
        &run_cli(
            &space.config_path,
            &[
                "sql",
                "query",
                &sql,
                "--limit",
                "1",
                "--continuation",
                continuation,
            ],
        ),
        "continued stateless SQL query",
    );
    assert_eq!(next_page["has_more"], false);
    assert!(next_page["next"].is_null());
    assert_ne!(next_page["rows"][0]["_ugoite_id"], first_id);

    let count = stdout_json(
        &run_cli(&space.config_path, &["sql", "count", &sql]),
        "local stateless SQL count",
    );
    assert_eq!(count["count"], 2);
}
