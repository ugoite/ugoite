//! Regression coverage for audit F01's inverse revision integrity contract.

use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use ugoite_iceberg::{entry, service::UgoiteService};
use uuid::Uuid;

fn fields(body: &str) -> BTreeMap<String, Value> {
    BTreeMap::from([
        ("Subject".into(), json!("監査ノート")),
        ("Body".into(), json!(body)),
        ("Status".into(), json!("draft")),
    ])
}

fn independent_integrity(markdown: &str, key: &[u8]) -> (String, String) {
    let checksum = hex::encode(Sha256::digest(markdown.as_bytes()));
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts arbitrary key lengths");
    mac.update(markdown.as_bytes());
    (checksum, hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn audit_baseline_revert_retains_after_integrity() -> Result<()> {
    let root = tempfile::tempdir()?;
    let service = UgoiteService::new(root.path().to_string_lossy().into_owned())?;
    let space_id = service
        .ensure_operator_space_with_name(&format!("audit-{}", Uuid::now_v7()), "Audit")
        .await?
        .space_id()
        .to_string();
    let workspace = format!("spaces/{space_id}");
    let entry_id = Uuid::now_v7().to_string();
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "AuditNote",
                "fields": {
                    "Subject": {"type": "string", "required": true},
                    "Body": {"type": "markdown"},
                    "Status": {"type": "string"}
                }
            }),
        )
        .await?;
    let (created, _) = service
        .create_structured_entry_with_receipt(
            &space_id,
            &entry_id,
            "AuditNote".into(),
            Vec::new(),
            fields("変更前の日本語本文"),
            BTreeMap::new(),
            "local-operator",
        )
        .await?;
    let before_id = created["revision_id"].as_str().expect("create revision");
    let updated = service
        .update_structured_entry(
            &space_id,
            &entry_id,
            None,
            fields("変更後の日本語本文、追記あり"),
            BTreeMap::new(),
            Some(before_id),
            "local-operator",
        )
        .await?;
    let after_id = updated["revision_id"].as_str().expect("update revision");
    let target_change_id = updated["change_id"].as_str().expect("update Change");
    let mut later_fields = fields("変更後の日本語本文、追記あり");
    later_fields.insert("Status".into(), json!("published"));
    let later = service
        .update_structured_entry(
            &space_id,
            &entry_id,
            None,
            later_fields,
            BTreeMap::new(),
            Some(after_id),
            "local-operator",
        )
        .await?;
    let later_id = later["revision_id"].as_str().expect("later revision");
    let before =
        entry::get_entry_revision(service.operator(), &workspace, &entry_id, before_id).await?;
    let after =
        entry::get_entry_revision(service.operator(), &workspace, &entry_id, after_id).await?;
    let later_revision =
        entry::get_entry_revision(service.operator(), &workspace, &entry_id, later_id).await?;
    // Decode the real Space key only inside the test; never print it or store it
    // in test artifacts. The oracle calls SHA-256/HMAC directly, not the provider.
    let key_payload: Value = serde_json::from_slice(
        &service
            .operator()
            .read(&format!("{workspace}/meta.json"))
            .await?
            .to_vec(),
    )?;
    let key = general_purpose::STANDARD
        .decode(key_payload["hmac_key"].as_str().expect("Space HMAC key"))?;
    for (revision_id, revision) in [(before_id, &before), (after_id, &after)] {
        let content = entry::get_entry_revision_content(
            service.operator(),
            &workspace,
            &entry_id,
            revision_id,
        )
        .await?;
        let (checksum, signature) = independent_integrity(&content.markdown, &key);
        assert_eq!(revision["integrity"]["checksum"], checksum);
        assert_eq!(revision["integrity"]["signature"], signature);
    }
    let changes_before = service.list_changes(&space_id).await?;
    let receipt = service
        .revert_change(&space_id, target_change_id, "local-operator", None, None)
        .await?;
    let current = entry::get_entry_content(service.operator(), &workspace, &entry_id).await?;
    let inverse = entry::get_entry_revision(
        service.operator(),
        &workspace,
        &entry_id,
        &current.revision_id,
    )
    .await?;
    assert_eq!(current.fields["Body"], "変更前の日本語本文");
    assert_eq!(current.fields["Status"], "published");
    assert_eq!(inverse["parent_revision_id"], later_id);
    assert_eq!(inverse["change_id"], receipt["change_id"]);
    let (checksum, signature) = independent_integrity(&current.markdown, &key);
    assert_eq!(inverse["integrity"]["checksum"], checksum);
    assert_eq!(inverse["integrity"]["signature"], signature);
    assert_eq!(
        service
            .list_changes(&space_id)
            .await?
            .as_array()
            .unwrap()
            .len(),
        changes_before.as_array().unwrap().len() + 1
    );
    for (revision_id, revision) in [
        (before_id, before),
        (after_id, after),
        (later_id, later_revision),
    ] {
        assert_eq!(
            entry::get_entry_revision(service.operator(), &workspace, &entry_id, revision_id)
                .await?,
            revision,
            "revert must leave historical revisions unchanged"
        );
    }
    Ok(())
}

#[tokio::test]
async fn revert_create_tombstone_integrity_matches_canonical_revision_body() -> Result<()> {
    let root = tempfile::tempdir()?;
    let service = UgoiteService::new(root.path().to_string_lossy().into_owned())?;
    let space_id = service
        .ensure_operator_space_with_name(&format!("audit-{}", Uuid::now_v7()), "Audit")
        .await?
        .space_id()
        .to_string();
    let workspace = format!("spaces/{space_id}");
    let entry_id = Uuid::now_v7().to_string();
    service
        .upsert_form(
            &space_id,
            &json!({"name":"AuditNote","fields":{"Subject":{"type":"string","required":true}}}),
        )
        .await?;
    let (created, _) = service
        .create_structured_entry_with_receipt(
            &space_id,
            &entry_id,
            "AuditNote".into(),
            Vec::new(),
            BTreeMap::from([("Subject".into(), json!("tombstone"))]),
            BTreeMap::new(),
            "local-operator",
        )
        .await?;
    let create_change_id = created["change_id"].as_str().expect("create Change");
    let receipt = service
        .revert_change(&space_id, create_change_id, "local-operator", None, None)
        .await?;
    let revision_id = receipt["revision_ids"][0]
        .as_str()
        .expect("tombstone revision");
    let revision =
        entry::get_entry_revision(service.operator(), &workspace, &entry_id, revision_id).await?;
    let content =
        entry::get_entry_revision_content(service.operator(), &workspace, &entry_id, revision_id)
            .await?;
    assert_eq!(revision["operation"], "delete");
    assert_eq!(revision["change_id"], receipt["change_id"]);
    assert_eq!(content.operation, "delete");

    let key_payload: Value = serde_json::from_slice(
        &service
            .operator()
            .read(&format!("{workspace}/meta.json"))
            .await?
            .to_vec(),
    )?;
    let key = general_purpose::STANDARD
        .decode(key_payload["hmac_key"].as_str().expect("Space HMAC key"))?;
    let (checksum, signature) = independent_integrity(&content.markdown, &key);
    assert_eq!(revision["integrity"]["checksum"], checksum);
    assert_eq!(revision["integrity"]["signature"], signature);
    Ok(())
}
