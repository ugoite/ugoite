//! PR00 characterization of F03/F04/F06 through real core CLI processes.
//!
//! These assertions deliberately record the pre-fix gaps; they are not proof
//! of the desired contracts. Each owning fix replaces its gap assertion with
//! a positive regression. Fixtures are generated here, without audit artifacts.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct AuditFixture {
    dir: tempfile::TempDir,
    config: PathBuf,
}

impl AuditFixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("cli-config.toml");
        let fixture = Self { dir, config };
        fixture.success(&["config", "init"]);
        fixture.success(&[
            "config",
            "connection",
            "set",
            "local",
            "--type",
            "core",
            "--root",
            fixture.dir.path().to_str().unwrap(),
        ]);
        fixture.success(&[
            "space",
            "create",
            &format!("audit-{}", uuid::Uuid::now_v7()),
        ]);
        fixture
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ugoite"))
            .args(["--config", self.config.to_str().unwrap()])
            .args(args)
            .output()
            .expect("execute real CLI")
    }

    fn success(&self, args: &[&str]) -> Output {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.success(args);
        serde_json::from_slice(&output.stdout).expect("CLI JSON output")
    }

    fn write_json(&self, filename: &str, value: &Value) -> PathBuf {
        let path = self.dir.path().join(filename);
        std::fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
        path
    }

    fn note_form(&self) -> PathBuf {
        self.write_json(
            "audit-note.json",
            &json!({
                "name": "AuditNote",
                "fields": {
                    "Subject": {"type": "string", "required": true},
                    "Body": {"type": "markdown"},
                    "Status": {"type": "string"}
                }
            }),
        )
    }

    fn save_form(&self, path: &Path) -> Value {
        self.json(&["form", "save", path.to_str().unwrap(), "-o", "json"])
    }
}

#[test]
fn audit_baseline_f03_upload_reference_attaches_and_downloads() {
    let fixture = AuditFixture::new();
    let form = fixture.write_json(
        "audit-asset.json",
        &json!({
            "name": "AuditAsset", "fields": {"File": {"type": "asset_reference"}}
        }),
    );
    fixture.save_form(&form);
    let proof = fixture.dir.path().join("proof.txt");
    let bytes = b"audit proof: 0123456789\n";
    assert_eq!(bytes.len(), 24);
    std::fs::write(&proof, bytes).unwrap();

    let receipt = fixture.json(&["asset", "upload", proof.to_str().unwrap(), "-o", "json"]);
    assert_eq!(receipt["kind"], "asset");
    assert!(!receipt["id"].as_str().unwrap().is_empty());
    for key in ["revision_id", "change_id", "run_id"] {
        assert_eq!(receipt.get(key), Some(&Value::Null));
    }
    let reference = receipt.get("asset_reference").expect("complete reference");
    assert_eq!(reference.as_object().unwrap().len(), 5);
    assert_eq!(reference["asset_id"], receipt["id"]);
    assert_eq!(reference["name"], "proof.txt");
    assert_eq!(reference["media_type"], "application/octet-stream");
    assert_eq!(reference["size_bytes"], bytes.len());
    let checksum = reference["sha256"].as_str().expect("sha256");
    assert_eq!(checksum.len(), 64);
    assert!(checksum
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    assert_eq!(receipt.as_object().unwrap().len(), 6);
    // Upload alone persists bytes but does not create a Knowledge reference.
    assert_eq!(fixture.json(&["asset", "list", "-o", "json"]), json!([]));
    let fields = fixture.write_json("asset-fields.json", &json!({"File": reference}));
    fixture.success(&[
        "entry",
        "create",
        "--id",
        "audit-asset-1",
        "--form",
        "AuditAsset",
        "--fields-file",
        fields.to_str().unwrap(),
    ]);
    let downloaded = fixture.dir.path().join("proof-downloaded.txt");
    fixture.success(&[
        "asset",
        "download",
        receipt["id"].as_str().unwrap(),
        "--entry",
        "audit-asset-1",
        "--field",
        "File",
        "--out",
        downloaded.to_str().unwrap(),
    ]);
    assert_eq!(std::fs::read(downloaded).unwrap(), bytes);
    let listed = fixture.json(&["asset", "list", "-o", "json"]);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["asset_id"], receipt["id"]);
}

#[test]
fn audit_f04_update_help_and_full_replacement_workflow_are_explicit() {
    let fixture = AuditFixture::new();
    fixture.save_form(&fixture.note_form());
    let entry_id = uuid::Uuid::now_v7().to_string();
    let before = fixture.json(&[
        "entry",
        "create",
        "--id",
        &entry_id,
        "--form",
        "AuditNote",
        "--field",
        "Subject=監査",
        "--field",
        "Body=変更前の本文",
        "--field",
        "Status=draft",
    ]);
    let parent = before["revision_id"].as_str().unwrap();
    let after = fixture.json(&[
        "entry",
        "update",
        &entry_id,
        "--field",
        "Subject=監査後",
        "--parent-revision-id",
        parent,
    ]);
    assert_ne!(before["change_id"], after["change_id"]);
    let current = fixture.json(&["entry", "get", &entry_id]);
    assert_eq!(current["fields"]["Subject"], "監査後");
    assert!(current["fields"].get("Body").is_none());
    assert!(current["fields"].get("Status").is_none());

    // A one-field edit is safe only after resending the full field map.
    let full_fields = fixture.write_json(
        "all-fields.json",
        &json!({
            "Subject": "監査後",
            "Body": "保持する本文",
            "Status": "published"
        }),
    );
    let complete = fixture.json(&[
        "entry",
        "update",
        &entry_id,
        "--fields-file",
        full_fields.to_str().unwrap(),
        "--parent-revision-id",
        after["revision_id"].as_str().unwrap(),
    ]);
    let complete_entry = fixture.json(&["entry", "get", &entry_id]);
    assert_eq!(complete_entry["fields"]["Subject"], "監査後");
    assert_eq!(complete_entry["fields"]["Body"], "保持する本文");
    assert_eq!(complete_entry["fields"]["Status"], "published");

    // Required-field validation rejects an incomplete map without appending.
    let missing_required = fixture.write_json(
        "missing-required.json",
        &json!({
            "Body": "本文だけでは保存できない"
        }),
    );
    let changes_before_invalid = fixture.json(&["change", "list", "-o", "json"]);
    let invalid = fixture.run(&[
        "entry",
        "update",
        &entry_id,
        "--fields-file",
        missing_required.to_str().unwrap(),
        "--parent-revision-id",
        complete["revision_id"].as_str().unwrap(),
    ]);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("required"));
    assert_eq!(
        fixture.json(&["change", "list", "-o", "json"]),
        changes_before_invalid
    );

    // An explicitly stale revision conflicts and leaves the current Entry intact.
    let stale = fixture.run(&[
        "entry",
        "update",
        &entry_id,
        "--fields-file",
        full_fields.to_str().unwrap(),
        "--parent-revision-id",
        parent,
    ]);
    assert_eq!(stale.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("REVISION_CONFLICT"));
    assert_eq!(fixture.json(&["entry", "get", &entry_id]), complete_entry);

    let history = fixture.json(&["entry", "history", &entry_id]);
    assert_eq!(history["revisions"].as_array().unwrap().len(), 3);
    fixture.success(&["entry", "restore", &entry_id, parent]);
    let restored = fixture.json(&["entry", "get", &entry_id]);
    assert_eq!(restored["fields"]["Body"], "変更前の本文");
    assert_eq!(restored["fields"]["Status"], "draft");

    let help = fixture.success(&["entry", "update", "--help"]);
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--field") && help.contains("--fields-file"));
    assert!(help.contains("Complete replacement"));
    assert!(help.contains("unspecified existing Form fields are removed"));
    assert!(help.contains("get all fields with `entry get`"));
    assert!(help.contains("resend the full map with `--fields-file`"));
    assert!(help.contains("`--parent-revision-id`"));
}

#[test]
fn audit_baseline_f06_form_receipt_identifies_commit_and_noop() {
    let fixture = AuditFixture::new();
    let form_file = fixture.note_form();
    let changes_before = fixture.json(&["change", "list", "-o", "json"]);
    let created = fixture.save_form(&form_file);
    let stored = fixture.json(&["form", "get", "AuditNote"]);
    assert!(!stored["id"].as_str().unwrap().is_empty());
    let changes_after = fixture.json(&["change", "list", "-o", "json"]);
    assert_eq!(
        changes_after.as_array().unwrap().len(),
        changes_before.as_array().unwrap().len() + 1
    );
    let noop = fixture.save_form(&form_file);
    let changes_noop = fixture.json(&["change", "list", "-o", "json"]);
    assert_eq!(changes_after, changes_noop);
    assert_eq!(fixture.json(&["form", "get", "AuditNote"]), stored);

    // Keep the common mutation fields and Form-name identity while reporting
    // the immutable persisted ID, version, and exact Change committed here.
    assert_eq!(created["kind"], "form");
    assert_eq!(created["id"], "AuditNote");
    assert_eq!(created["name"], "AuditNote");
    assert_eq!(created["revision_id"], Value::Null);
    assert_eq!(created["run_id"], Value::Null);
    assert_eq!(created["form_id"], stored["id"]);
    assert_eq!(created["form_version"], stored["version"]);
    assert_eq!(created["applied"], true);
    let committed_change_id = created["change_id"].as_str().expect("create Change ID");
    assert_eq!(
        changes_after.as_array().unwrap().last().unwrap()["change_id"],
        committed_change_id
    );

    assert_eq!(noop["kind"], "form");
    assert_eq!(noop["id"], "AuditNote");
    assert_eq!(noop["form_id"], created["form_id"]);
    assert_eq!(noop["form_version"], created["form_version"]);
    assert_eq!(noop["applied"], false);
    assert_eq!(noop["change_id"], Value::Null);
    assert_ne!(created, noop, "no-op must be distinguishable from create");
}
