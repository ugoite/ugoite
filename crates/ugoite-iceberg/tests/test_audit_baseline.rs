//! Regression coverage for audit F01's inverse revision integrity contract.

use anyhow::Result;
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use ugoite_iceberg::{
    audit::{self, AuditListOptions},
    entry, saved_sql,
    service::UgoiteService,
};
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

/// Commit-time delivery and reopen reconciliation preserve portable Entry
/// provenance without promoting a UUID-shaped value to principal identity.
#[tokio::test]
async fn entry_commit_and_reconcile_preserve_portable_provenance() -> Result<()> {
    let root = tempfile::tempdir()?;
    let root_uri = root.path().to_string_lossy().into_owned();
    let service = UgoiteService::new(root_uri.clone())?;
    let owner = Uuid::now_v7();
    let space_slug = format!("audit-converge-{}", Uuid::now_v7().simple());
    let space_id = service
        .create_space_for_principal(&space_slug, owner, "Audit convergence")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &json!({"name": "Note", "fields": {"Body": {"type": "markdown", "required": true}}}),
        )
        .await?;
    // Free-form author that differs from the authorizing principal on purpose.
    service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "converge-entry",
            "Note".to_string(),
            Vec::new(),
            BTreeMap::from([("Body".to_string(), json!("before"))]),
            BTreeMap::new(),
            "human author label",
            &[owner],
        )
        .await?;
    let mut updated_fields = BTreeMap::new();
    updated_fields.insert("Body".to_string(), json!("after"));
    let updated = service
        .update_structured_entry_authorized_for_principals(
            &space_id,
            "converge-entry",
            None,
            None,
            updated_fields,
            BTreeMap::new(),
            None,
            "human author label",
            &[owner],
        )
        .await?;
    let update_change_id = updated["change_id"].as_str().expect("update Change");
    service
        .revert_change(
            &space_id,
            update_change_id,
            &owner.to_string(),
            None,
            Some("Recover the pre-update value"),
        )
        .await?;
    let before =
        audit::list_audit_events(service.operator(), &space_id, AuditListOptions::default())
            .await?;
    assert!(before["items"].as_array().is_some_and(|items| {
        items
            .iter()
            .all(|event| event["actor_principal_id"].is_null())
            && items
                .iter()
                .any(|event| event["subject_principal_id"] == "human author label")
    }));
    drop(service);
    // Reopen must replay the same payload without changing canonical history.
    let reopened = UgoiteService::new(root_uri)?;
    let recovery = reopened.open_space(&space_id).await?;
    assert_eq!(recovery["space_id"], space_id);
    assert_eq!(
        reopened.get_entry(&space_id, "converge-entry").await?["fields"]["Body"],
        "before"
    );
    let after =
        audit::list_audit_events(reopened.operator(), &space_id, AuditListOptions::default())
            .await?;
    let before_items = before["items"].as_array().expect("before audit items");
    let after_items = after["items"].as_array().expect("after audit items");
    assert!(after_items.len() >= before_items.len());
    for event in before_items {
        let event_id = event["event_id"].as_str().expect("deterministic event ID");
        let replayed = after_items
            .iter()
            .find(|replayed| replayed["event_id"] == event_id)
            .expect("original audit event remains present");
        assert_eq!(replayed, event, "replay must not rewrite canonical history");
    }
    assert!(after_items
        .iter()
        .all(|event| event["actor_principal_id"].is_null()));
    Ok(())
}

/// A UUID-shaped portable author is not proof of an authenticated principal.
/// Commit-time delivery and startup replay must therefore retain the same
/// unattributed actor value for deterministic Saved SQL events.
#[tokio::test]
async fn saved_sql_uuid_author_does_not_become_actor_during_reopen() -> Result<()> {
    let root = tempfile::tempdir()?;
    let root_uri = root.path().to_string_lossy().into_owned();
    let service = UgoiteService::new(root_uri.clone())?;
    let space_id = service
        .ensure_operator_space_with_name(&format!("audit-{}", Uuid::now_v7()), "Audit")
        .await?
        .space_id()
        .to_string();
    let author = Uuid::now_v7().to_string();
    let payload = saved_sql::SqlPayload {
        name: Some("query".to_string()),
        kind: saved_sql::SqlKind::UserQuery,
        metadata: None,
        sql: "SELECT 1".to_string(),
        variables: json!([]),
    };
    service
        .create_saved_sql(&space_id, Some("uuid-author"), &payload, &author)
        .await?;

    let before =
        audit::list_audit_events(service.operator(), &space_id, AuditListOptions::default())
            .await?;
    assert_eq!(before["total"], 1);
    assert_eq!(before["items"][0]["subject_principal_id"], author);
    assert!(before["items"][0]["actor_principal_id"].is_null());
    drop(service);

    let reopened = UgoiteService::new(root_uri)?;
    reopened.open_space(&space_id).await?;
    let after =
        audit::list_audit_events(reopened.operator(), &space_id, AuditListOptions::default())
            .await?;
    assert_eq!(after["total"], 1);
    assert_eq!(after["items"][0]["subject_principal_id"], author);
    assert!(after["items"][0]["actor_principal_id"].is_null());
    Ok(())
}
