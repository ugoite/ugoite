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
fn audit_baseline_f03_upload_receipt_omits_asset_reference() {
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
    // F03 desired invariant: asset_reference must carry the upload's complete
    // AssetReference. Today only an opaque ID survives CLI projection.
    assert!(receipt.get("asset_reference").is_none());
    assert_eq!(receipt.as_object().unwrap().len(), 5);
    let fields = fixture.write_json("asset-fields.json", &json!({"File": receipt}));
    let rejected = fixture.run(&[
        "entry",
        "create",
        "--form",
        "AuditAsset",
        "--fields-file",
        fields.to_str().unwrap(),
    ]);
    assert!(
        !rejected.status.success(),
        "receipt is not an AssetReference"
    );
    // Upload persists bytes but is not an Entry save; unreferenced assets do
    // not become discoverable by listing Form-owned references.
    assert_eq!(fixture.json(&["asset", "list", "-o", "json"]), json!([]));
}

#[test]
fn audit_baseline_f04_update_replaces_fields_without_help_warning() {
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
    let history = fixture.json(&["entry", "history", &entry_id]);
    assert_eq!(history["revisions"].as_array().unwrap().len(), 2);
    fixture.success(&["entry", "restore", &entry_id, parent]);
    let restored = fixture.json(&["entry", "get", &entry_id]);
    assert_eq!(restored["fields"]["Body"], "変更前の本文");
    assert_eq!(restored["fields"]["Status"], "draft");

    let help = fixture.success(&["entry", "update", "--help"]);
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("--field") && help.contains("--fields-file"));
    // F04 keeps these complete-replacement semantics, but requires the real
    // help output to explain omitted-field deletion before users execute it.
    assert!(!help.contains("complete post-update"));
    assert!(!help.contains("omitted fields are removed"));
}

#[test]
fn audit_baseline_f06_form_receipt_cannot_identify_commit_or_noop() {
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

    // F06 desired invariant: preserve id=Form name and add durable Form ID,
    // actual committed Change ID/version, and applied=false for no-op.
    assert_eq!(
        created,
        json!({
            "kind": "form", "id": "AuditNote", "revision_id": null,
            "change_id": null, "run_id": null
        })
    );
    assert_eq!(
        created, noop,
        "current receipt cannot distinguish create from no-op"
    );
}
