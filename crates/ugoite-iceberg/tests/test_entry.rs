mod common;
use common::setup_operator;
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use ugoite_domain::change::ChangeCommand;
use ugoite_iceberg::asset;
use ugoite_iceberg::entry;
use ugoite_iceberg::form;
use ugoite_iceberg::iceberg_store;
use ugoite_iceberg::index;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::space;
use uuid::Uuid;

async fn ensure_entry_form(op: &opendal::Operator, ws_path: &str) -> anyhow::Result<()> {
    let form_def = serde_json::json!({
        "name": "Entry",
        "template": "# Entry\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}},
        "allow_extra_attributes": "allow_columns",
    });
    form::upsert_form(op, ws_path, &form_def).await?;
    Ok(())
}

#[tokio::test]
async fn explicit_change_command_identity_reaches_entry_history() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "explicit-change-entry", "/tmp").await?;
    let ws_path = "spaces/explicit-change-entry";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let create_change = ChangeCommand {
        change_id: "change-create-entry".into(),
        run_id: Some(ugoite_domain::change::RunId::new("run-1")?),
        actor_principal_id: "author".into(),
        message: Some("create entry".into()),
        reverts_change_id: None,
        created_at_micros: 1,
    };
    entry::create_entry_with_scopes_and_change(
        &op,
        ws_path,
        "entry-1",
        "---\nform: Entry\nBody: first\n---\n# Entry",
        "author",
        &integrity,
        None,
        Some(create_change),
    )
    .await?;
    let created = entry::get_entry_content(&op, ws_path, "entry-1").await?;

    let update_change = ChangeCommand {
        change_id: "change-update-entry".into(),
        run_id: Some(ugoite_domain::change::RunId::new("run-1")?),
        actor_principal_id: "author".into(),
        message: Some("update entry".into()),
        reverts_change_id: None,
        created_at_micros: 2,
    };
    entry::update_entry_authorized_with_change(
        &op,
        ws_path,
        "entry-1",
        "---\nform: Entry\nBody: second\n---\n# Entry",
        Some(&created.revision_id),
        "author",
        &integrity,
        None,
        Some(update_change),
    )
    .await?;

    let history = entry::get_entry_history(&op, ws_path, "entry-1").await?;
    let history = history["revisions"]
        .as_array()
        .expect("history is an array");
    assert_eq!(history[0]["change_id"], "change-create-entry");
    assert_eq!(history[1]["change_id"], "change-update-entry");
    Ok(())
}

#[tokio::test]
async fn row_reference_values_must_target_current_entries_in_the_declared_form(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "typed-reference-entry", "/tmp").await?;
    let ws_path = "spaces/typed-reference-entry";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Project",
            "fields": {"Name": {"type": "string"}},
        }),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Task",
            "fields": {
                "Parent": {"type": "row_reference", "target_form": "Project"},
                "Reviewers": {
                    "type": "list",
                    "items": {"type": "row_reference", "target_form": "Project"},
                },
            },
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;
    entry::create_entry(
        &op,
        ws_path,
        "project-1",
        "---\nform: Project\nName: Example\n---\n# Project",
        "author",
        &integrity,
    )
    .await?;

    entry::create_entry(
        &op,
        ws_path,
        "task-1",
        "---\nform: Task\nParent: project-1\nReviewers: [project-1]\n---\n# Task",
        "author",
        &integrity,
    )
    .await?;
    let reviewer_matches = index::query_index(
        &op,
        ws_path,
        &serde_json::json!({"Reviewers": "project-1"}).to_string(),
    )
    .await?;
    assert_eq!(reviewer_matches.len(), 1);
    assert_eq!(reviewer_matches[0]["id"], "task-1");
    let parent_matches = index::query_index(
        &op,
        ws_path,
        &serde_json::json!({"Parent": "project-1"}).to_string(),
    )
    .await?;
    assert_eq!(parent_matches.len(), 1);
    assert_eq!(parent_matches[0]["id"], "task-1");
    let error = entry::create_entry(
        &op,
        ws_path,
        "task-2",
        "---\nform: Task\nParent: missing\nReviewers: [missing]\n---\n# Invalid",
        "author",
        &integrity,
    )
    .await
    .expect_err("references must resolve to the declared target Form");
    assert!(error.to_string().contains("does not belong to Form"));
    Ok(())
}

#[tokio::test]
async fn malformed_asset_references_are_typed_input_errors() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "malformed-asset-entry", "/tmp").await?;
    let ws_path = "spaces/malformed-asset-entry";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "AssetEntry",
            "fields": {"Attachment": {"type": "asset_reference"}},
        }),
    )
    .await?;

    let error = entry::create_entry(
        &op,
        ws_path,
        "asset-entry-1",
        "---\nform: AssetEntry\nAttachment: {\"asset_id\":\"../bad\",\"name\":\"x\",\"media_type\":\"text/plain\",\"size_bytes\":1,\"sha256\":\"x\"}\n---\n# Invalid",
        "author",
        &FakeIntegrityProvider,
    )
    .await
    .expect_err("malformed asset IDs must be rejected before storage lookup");
    let app_error = error
        .downcast_ref::<AppError>()
        .expect("validation errors stay typed");
    assert_eq!(app_error.code(), ErrorCode::InvalidInput);
    assert!(app_error.message().contains("Attachment"));
    Ok(())
}

#[tokio::test]
async fn restore_replays_historical_references_even_when_targets_are_unavailable(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "restore-unavailable-targets", "/tmp").await?;
    let ws_path = "spaces/restore-unavailable-targets";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Target",
            "fields": {"Name": {"type": "string"}}
        }),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Source",
            "fields": {
                "Target": {"type": "row_reference", "target_form": "Target"},
                "Targets": {"type": "list", "items": {"type": "row_reference", "target_form": "Target"}},
                "Attachment": {"type": "asset_reference"},
                "Attachments": {"type": "list", "items": {"type": "asset_reference"}}
            }
        }),
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "target-1",
        "---\nform: Target\nName: Target\n---\n# Target",
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    let reference = asset::save_asset(&op, ws_path, "restore.bin", b"restore bytes").await?;
    let reference_json = serde_json::to_string(&reference)?;
    entry::create_entry(
        &op,
        ws_path,
        "source-1",
        &format!(
            "---\nform: Source\nTarget: target-1\nTargets: [target-1]\nAttachment: {reference_json}\nAttachments: [{reference_json}]\n---\n# Source"
        ),
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    let historical_revision = entry::get_entry_content(&op, ws_path, "source-1")
        .await?
        .revision_id;

    // Remove the references from the current value first. This is the only
    // state from which deleting the target Asset is valid.
    entry::update_entry(
        &op,
        ws_path,
        "source-1",
        "---\nform: Source\n---\n# Source without references",
        Some(&historical_revision),
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    entry::delete_entry(&op, ws_path, "target-1", false, "deleter").await?;
    asset::delete_asset(&op, ws_path, &reference.asset_id, &Default::default()).await?;

    entry::restore_entry(
        &op,
        ws_path,
        "source-1",
        &historical_revision,
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    let restored = entry::list_entries(&op, ws_path)
        .await?
        .into_iter()
        .find(|entry| entry["id"] == "source-1")
        .expect("restored Entry");
    assert_eq!(restored["properties"]["Target"], "target-1");
    assert_eq!(restored["properties"]["Targets"][0], "target-1");
    assert_eq!(
        restored["properties"]["Attachment"]["asset_id"],
        reference.asset_id
    );
    assert_eq!(
        restored["properties"]["Attachments"][0]["asset_id"],
        reference.asset_id
    );
    assert!(asset::read_asset(&op, ws_path, &reference.asset_id)
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
/// REQ-ENTRY-001
async fn test_entry_req_entry_001_create_entry_basic() -> anyhow::Result<()> {
    let op = setup_operator()?;
    // We assume workspace exists
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";
    ensure_entry_form(&op, ws_path).await?;

    let integrity = FakeIntegrityProvider;
    let content = "---\nform: Entry\n---\n# My Entry\n\nHello World";
    let entry_id = "entry-1";

    entry::create_entry(&op, ws_path, entry_id, content, "test-author", &integrity).await?;

    let content_info = entry::get_entry_content(&op, ws_path, entry_id).await?;
    assert!(!content_info.revision_id.is_empty());
    let history = entry::get_entry_history(&op, ws_path, entry_id).await?;
    let revisions = history.get("revisions").and_then(|v| v.as_array()).unwrap();
    assert_eq!(revisions.len(), 1);

    Ok(())
}

#[tokio::test]
async fn entry_list_supports_bounded_offset_pages_in_stable_order() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "paged-entry-list", "/tmp").await?;
    let ws_path = "spaces/paged-entry-list";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    for (entry_id, title) in [("entry-a", "A"), ("entry-b", "B"), ("entry-c", "C")] {
        entry::create_entry(
            &op,
            ws_path,
            entry_id,
            &format!("---\nform: Entry\n---\n# {title}"),
            "author",
            &integrity,
        )
        .await?;
    }

    let page = entry::list_entries_with_scopes(
        &op,
        ws_path,
        &BTreeMap::from([("entry".to_string(), EntryScope::AllCurrent)]),
        2,
        1,
    )
    .await?;
    assert_eq!(
        page.iter()
            .map(|value| value["id"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["entry-b", "entry-c"]
    );
    Ok(())
}

#[tokio::test]
async fn deleted_entry_history_revision_and_restore_remain_reachable() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "deleted-entry-history", "/tmp").await?;
    let ws_path = "spaces/deleted-entry-history";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    entry::create_entry(
        &op,
        ws_path,
        "deleted-entry",
        "---\nform: Entry\n---\n# Deleted entry\n\n## Body\nBefore delete",
        "author",
        &integrity,
    )
    .await?;
    let original = entry::get_entry_content(&op, ws_path, "deleted-entry").await?;
    entry::delete_entry(&op, ws_path, "deleted-entry", false, "deleter").await?;

    let history = entry::get_entry_history(&op, ws_path, "deleted-entry").await?;
    assert_eq!(history["revisions"].as_array().map(Vec::len), Some(2));
    let revision =
        entry::get_entry_revision(&op, ws_path, "deleted-entry", &original.revision_id).await?;
    assert_eq!(revision["revision_id"], original.revision_id);
    let historical_content =
        entry::get_entry_revision_content(&op, ws_path, "deleted-entry", &original.revision_id)
            .await?;
    assert!(historical_content.markdown.contains("Before delete"));

    entry::restore_entry(
        &op,
        ws_path,
        "deleted-entry",
        &original.revision_id,
        "author",
        &integrity,
    )
    .await?;
    let restored = entry::get_entry_content(&op, ws_path, "deleted-entry").await?;
    assert!(restored.markdown.contains("Before delete"));
    Ok(())
}

#[tokio::test]
async fn publication_restore_appends_current_head_with_provenance() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "checkpoint-restore", "/tmp").await?;
    let ws_path = "spaces/checkpoint-restore";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    entry::create_entry(
        &op,
        ws_path,
        "checkpoint-entry",
        "---\nform: Entry\n---\n# Before checkpoint\n\n## Body\nOriginal",
        "author",
        &integrity,
    )
    .await?;
    let original = entry::get_entry_content(&op, ws_path, "checkpoint-entry").await?;
    let workspace = iceberg_store::native_workspace(&op, ws_path).await?;
    let publication = workspace.current_publication().await?;
    let form_id = workspace
        .list_forms()
        .await?
        .into_iter()
        .next()
        .expect("form")
        .id;
    let entry_uuid = Uuid::new_v5(&Uuid::NAMESPACE_URL, b"checkpoint-entry").into();
    let scoped_history = entry::get_entry_history_at_publication(
        &op,
        ws_path,
        "checkpoint-entry",
        &publication,
        Some(&BTreeMap::from([(
            form_id,
            EntryScope::Only(BTreeSet::from([entry_uuid])),
        )])),
    )
    .await?;
    assert_eq!(
        scoped_history["revisions"].as_array().map(Vec::len),
        Some(1)
    );

    entry::update_entry(
        &op,
        ws_path,
        "checkpoint-entry",
        "---\nform: Entry\n---\n# After checkpoint\n\n## Body\nChanged",
        Some(&original.revision_id),
        "author",
        &integrity,
    )
    .await?;

    let restored = entry::restore_entry_from_publication_authorized(
        &op,
        ws_path,
        "checkpoint-entry",
        &original.revision_id,
        &publication,
        "restorer",
        &integrity,
        None,
    )
    .await?;
    assert_eq!(restored["source_revision_id"], original.revision_id);
    assert_eq!(restored["restored_from"], original.revision_id);

    let current = entry::get_entry_content(&op, ws_path, "checkpoint-entry").await?;
    assert!(current.markdown.contains("Original"));
    assert!(current.markdown.contains("Before checkpoint"));
    assert_ne!(current.revision_id, original.revision_id);

    let revision = entry::get_entry_revision(
        &op,
        ws_path,
        "checkpoint-entry",
        restored["revision_id"].as_str().expect("revision ID"),
    )
    .await?;
    assert_eq!(revision["source_kind"], "publication_restore");
    assert_eq!(revision["source_id"], original.revision_id);
    assert_eq!(
        revision["extension_metadata"]["restore_source_publication"],
        serde_json::to_value(&publication)?
    );
    assert_eq!(revision["author"], "author");
    assert_eq!(revision["updated_by"], "restorer");
    Ok(())
}

#[tokio::test]
async fn publication_pin_restore_appends_with_publication_provenance() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "publication-restore", "/tmp").await?;
    let ws_path = "spaces/publication-restore";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    entry::create_entry(
        &op,
        ws_path,
        "publication-entry",
        "---\nform: Entry\n---\n# Before pin\n\n## Body\nOriginal",
        "author",
        &integrity,
    )
    .await?;
    let original = entry::get_entry_content(&op, ws_path, "publication-entry").await?;
    let workspace = iceberg_store::native_workspace(&op, ws_path).await?;
    let pin = workspace
        .create_pin("before", "author", 1, "publication-pin")
        .await?;

    entry::update_entry(
        &op,
        ws_path,
        "publication-entry",
        "---\nform: Entry\n---\n# After pin\n\n## Body\nChanged",
        Some(&original.revision_id),
        "author",
        &integrity,
    )
    .await?;

    let restored = entry::restore_entry_from_publication_authorized(
        &op,
        ws_path,
        "publication-entry",
        &original.revision_id,
        &pin.coordinate,
        "restorer",
        &integrity,
        None,
    )
    .await?;
    assert_eq!(
        restored["source_publication"],
        serde_json::to_value(&pin.coordinate)?
    );

    let revision = entry::get_entry_revision(
        &op,
        ws_path,
        "publication-entry",
        restored["revision_id"].as_str().expect("revision ID"),
    )
    .await?;
    assert_eq!(revision["source_kind"], "publication_restore");
    assert_eq!(
        revision["extension_metadata"]["restore_source_publication"],
        serde_json::to_value(&pin.coordinate)?
    );
    Ok(())
}

#[tokio::test]
async fn entry_ids_are_global_across_forms_and_tombstones() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "global-entry-ids", "/tmp").await?;
    let ws_path = "spaces/global-entry-ids";
    for form_name in ["First", "Second"] {
        form::upsert_form(
            &op,
            ws_path,
            &serde_json::json!({"name": form_name, "fields": {"Body": {"type": "markdown"}}}),
        )
        .await?;
    }
    let integrity = FakeIntegrityProvider;
    entry::create_entry(
        &op,
        ws_path,
        "global-id",
        "---\nform: First\n---\n# First\n\n## Body\nOne",
        "author",
        &integrity,
    )
    .await?;
    let scopes = std::collections::BTreeMap::from([(
        "second".to_string(),
        ugoite_core::query::EntryScope::AllCurrent,
    )]);
    let unreadable_duplicate = entry::create_entry_with_scopes(
        &op,
        ws_path,
        "global-id",
        "---\nform: Second\n---\n# Second\n\n## Body\nTwo",
        "author",
        &integrity,
        Some(&scopes),
    )
    .await
    .expect_err("global ID availability must not be caller-scoped");
    let unreadable_duplicate = unreadable_duplicate
        .downcast_ref::<AppError>()
        .expect("global ID rejection must remain typed");
    assert_eq!(unreadable_duplicate.code(), ErrorCode::InvalidInput);
    assert!(unreadable_duplicate.message().contains("global-id"));

    entry::delete_entry(&op, ws_path, "global-id", false, "author").await?;
    let tombstone_duplicate = entry::create_entry(
        &op,
        ws_path,
        "global-id",
        "---\nform: Second\n---\n# Second\n\n## Body\nTwo",
        "author",
        &integrity,
    )
    .await
    .expect_err("tombstones must retain the global ID reservation");
    let tombstone_duplicate = tombstone_duplicate
        .downcast_ref::<AppError>()
        .expect("tombstone ID rejection must remain typed");
    assert_eq!(tombstone_duplicate.code(), ErrorCode::InvalidInput);
    assert!(tombstone_duplicate.message().contains("global-id"));
    Ok(())
}

#[tokio::test]
async fn entry_create_with_numeric_and_all_timestamp_fields_publishes_one_revision(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "entry-create-fields", "/tmp").await?;
    let ws_path = "spaces/entry-create-fields";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {
                "Body": {"type": "markdown"},
                "test number": {"type": "double"},
                "ts": {"type": "timestamp"},
                "ts tz": {"type": "timestamp_tz"},
                "ts ns": {"type": "timestamp_ns"},
                "ts tz ns": {"type": "timestamp_tz_ns"},
            },
            "allow_extra_attributes": "allow_columns",
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;
    let content = "---\nform: Entry\n---\n# Entry\n\n## Body\nmemememo\n\n## test number\n0\n\n## ts\n2026-08-21T10:48\n\n## ts tz\n2026-08-21T10:48:00+09:00\n\n## ts ns\n2026-08-21T10:48:00.123456789\n\n## ts tz ns\n2026-08-21T10:48:00.123456789+09:00";

    let created = entry::create_entry(
        &op,
        ws_path,
        "entry-create-fields-1",
        content,
        "author",
        &integrity,
    )
    .await?;
    let current = entry::get_entry_content(&op, ws_path, "entry-create-fields-1").await?;
    let history = entry::get_entry_history(&op, ws_path, "entry-create-fields-1").await?;
    let revisions = history["revisions"].as_array().expect("revision array");

    assert_eq!(created.id, "entry-create-fields-1");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0]["revision_id"], current.revision_id);
    assert!(current.markdown.contains("## ts\n2026-08-21T10:48:00"));
    assert!(current
        .markdown
        .contains("## ts tz\n2026-08-21T01:48:00+00:00"));
    assert!(current
        .markdown
        .contains("## ts ns\n2026-08-21T10:48:00.123456789"));
    assert!(current
        .markdown
        .contains("## ts tz ns\n2026-08-21T01:48:00.123456789Z"));

    Ok(())
}

#[tokio::test]
async fn entry_update_after_create_with_numeric_and_timestamp_fields() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "entry-form-fields", "/tmp").await?;
    let ws_path = "spaces/entry-form-fields";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {
                "Body": {"type": "markdown"},
                "test number": {"type": "double"},
                "ts": {"type": "timestamp"},
            },
            "allow_extra_attributes": "allow_columns",
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;
    let content = "---\nform: Entry\n---\n# Entry\n\n## Body\nmemememo";

    entry::create_entry(
        &op,
        ws_path,
        "entry-form-fields-1",
        content,
        "author",
        &integrity,
    )
    .await?;
    let current = entry::get_entry_content(&op, ws_path, "entry-form-fields-1").await?;

    let updated_content = format!(
        "{}\n\n## test number\n0\n\n## ts\n2026-08-21T10:48",
        current.markdown.replace("memememo", "memememo updated")
    );
    entry::update_entry(
        &op,
        ws_path,
        "entry-form-fields-1",
        &updated_content,
        Some(&current.revision_id),
        "author",
        &integrity,
    )
    .await?;

    let updated = entry::get_entry_content(&op, ws_path, "entry-form-fields-1").await?;
    assert!(updated.markdown.contains("## ts\n2026-08-21T10:48:00"));

    let invalid_content = updated
        .markdown
        .replace("2026-08-21T10:48:00", "not-a-timestamp");
    let error = entry::update_entry(
        &op,
        ws_path,
        "entry-form-fields-1",
        &invalid_content,
        Some(&updated.revision_id),
        "author",
        &integrity,
    )
    .await
    .expect_err("invalid timestamp input must be rejected");
    let app_error = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("entry validation failures must remain typed application errors");
    assert_eq!(
        app_error.code(),
        ugoite_core::error::ErrorCode::FormValidationFailed
    );

    Ok(())
}

#[tokio::test]
async fn explicit_entry_batch_creates_all_entries() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "batched-entry-space", "/tmp").await?;
    let ws_path = "spaces/batched-entry-space";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let entries = entry::create_entries(
        &op,
        ws_path,
        vec![
            entry::EntryCreateRequest::new(
                "batched-entry-1",
                "---\nform: Entry\n---\n# First\n\n## Body\nOne",
            ),
            entry::EntryCreateRequest::new(
                "batched-entry-2",
                "---\nform: Entry\n---\n# Second\n\n## Body\nTwo",
            ),
        ],
        "test-author",
        &integrity,
    )
    .await?;
    assert_eq!(entries.len(), 2);

    assert_eq!(entry::list_entries(&op, ws_path).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn entry_batch_rejects_existing_ids_before_publishing_other_forms() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "batch-id-validation", "/tmp").await?;
    let ws_path = "spaces/batch-id-validation";
    for form_name in ["A", "B"] {
        form::upsert_form(
            &op,
            ws_path,
            &serde_json::json!({
                "name": form_name,
                "fields": {"Body": {"type": "markdown"}},
            }),
        )
        .await?;
    }
    entry::create_entry(
        &op,
        ws_path,
        "taken-id",
        "---\nform: B\n---\n# Existing\n\n## Body\nExisting",
        "author",
        &FakeIntegrityProvider,
    )
    .await?;

    let error = entry::create_entries(
        &op,
        ws_path,
        vec![
            entry::EntryCreateRequest::new("new-id", "---\nform: A\n---\n# New\n\n## Body\nNew"),
            entry::EntryCreateRequest::new(
                "taken-id",
                "---\nform: B\n---\n# Duplicate\n\n## Body\nDuplicate",
            ),
        ],
        "author",
        &FakeIntegrityProvider,
    )
    .await
    .expect_err("existing IDs must be rejected before another Form is published");
    let app_error = error
        .downcast_ref::<AppError>()
        .expect("duplicate IDs must remain typed input errors");
    assert_eq!(app_error.code(), ErrorCode::InvalidInput);
    assert!(app_error.message().contains("taken-id"));
    assert!(entry::get_entry(&op, ws_path, "new-id").await.is_err());
    assert_eq!(entry::list_entries(&op, ws_path).await?.len(), 1);
    Ok(())
}

#[tokio::test]
async fn same_form_batch_may_reference_another_pending_entry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "same-batch-reference-space", "/tmp").await?;
    let ws_path = "spaces/same-batch-reference-space";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Task",
            "fields": {
                "Parent": {"type": "row_reference", "target_form": "Task"}
            }
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;
    let entries = entry::create_entries(
        &op,
        ws_path,
        vec![
            entry::EntryCreateRequest::new(
                "task-child",
                "---\nform: Task\nParent: task-parent\n---\n# Child",
            ),
            entry::EntryCreateRequest::new("task-parent", "---\nform: Task\n---\n# Parent"),
        ],
        "test-author",
        &integrity,
    )
    .await?;
    assert_eq!(entries.len(), 2);
    Ok(())
}

#[tokio::test]
async fn cross_form_forward_references_are_rejected_deterministically() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "cross-batch-reference-space", "/tmp").await?;
    let ws_path = "spaces/cross-batch-reference-space";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({"name": "Project", "fields": {"Name": {"type": "string"}}}),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Task",
            "fields": {"Project": {"type": "row_reference", "target_form": "Project"}}
        }),
    )
    .await?;
    let error = entry::create_entries(
        &op,
        ws_path,
        vec![
            entry::EntryCreateRequest::new(
                "task-forward",
                "---\nform: Task\nProject: project-forward\n---\n# Task",
            ),
            entry::EntryCreateRequest::new(
                "project-forward",
                "---\nform: Project\nName: Project\n---\n# Project",
            ),
        ],
        "test-author",
        &FakeIntegrityProvider,
    )
    .await
    .expect_err("cross-Form forward references need a coherent multi-Form commit");
    assert!(error.to_string().contains("cross-Form forward references"));
    Ok(())
}

#[tokio::test]
async fn explicit_entry_batch_rejects_unbounded_input() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let integrity = FakeIntegrityProvider;
    let requests = (0..=entry::MAX_ENTRY_CREATE_BATCH_SIZE)
        .map(|index| entry::EntryCreateRequest::new(format!("entry-{index}"), ""))
        .collect();
    let error = entry::create_entries(&op, "spaces/not-created", requests, "author", &integrity)
        .await
        .expect_err("oversized explicit batch must be rejected before I/O");
    assert!(error.to_string().contains("limited to"));
    Ok(())
}

#[tokio::test]
async fn entry_update_accepts_minute_precision_time_values() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "time-entry", "/tmp").await?;
    let ws_path = "spaces/time-entry";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {
                "Body": {"type": "markdown"},
                "time": {"type": "time"},
            },
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;

    entry::create_entry(
        &op,
        ws_path,
        "time-entry-1",
        "---\nform: Entry\n---\n# Time entry\n\n## Body\nNotes\n\n## time\n22:02",
        "author",
        &integrity,
    )
    .await?;

    let content = entry::get_entry_content(&op, ws_path, "time-entry-1").await?;
    assert!(content.markdown.contains("## time\n22:02"));
    let results = index::query_index(&op, ws_path, r#"{"form":"Entry"}"#).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["properties"]["time"], "22:02:00");
    Ok(())
}

#[tokio::test]
async fn querying_entries_survives_adding_a_time_column() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "evolved-time-entry", "/tmp").await?;
    let ws_path = "spaces/evolved-time-entry";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    entry::create_entry(
        &op,
        ws_path,
        "entry-before-time",
        "---\nform: Entry\n---\n# Before time\n\n## Body\nExisting row",
        "author",
        &integrity,
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {
                "Body": {"type": "markdown"},
                "time": {"type": "time"},
            },
        }),
    )
    .await?;

    let results = index::query_index(&op, ws_path, r#"{"form":"Entry"}"#).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"], "entry-before-time");
    assert_eq!(results[0]["properties"]["Body"], "Existing row");
    Ok(())
}

#[tokio::test]
async fn renaming_existing_field_is_rejected_before_v1() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "renamed-entry-field", "/tmp").await?;
    let ws_path = "spaces/renamed-entry-field";
    ensure_entry_form(&op, ws_path).await?;

    entry::create_entry(
        &op,
        ws_path,
        "entry-before-rename",
        "---\nform: Entry\n---\n# Before rename\n\n## Body\nExisting value",
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    let rename_error = form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {
                "Content": {"id": 100, "type": "markdown"}
            },
        }),
    )
    .await
    .expect_err("pre-v1 Form renames must be rejected");
    let rename_error = rename_error
        .downcast_ref::<AppError>()
        .expect("Form rename rejection must remain typed");
    assert_eq!(rename_error.code(), ErrorCode::FormFieldRemovalNotSupported);
    assert!(rename_error.message().contains("Body"));

    let all = index::query_index(&op, ws_path, r#"{"form":"Entry"}"#).await?;
    assert_eq!(all[0]["properties"]["Body"], "Existing value");
    Ok(())
}

#[tokio::test]
async fn typed_uuid_binary_and_list_queries_use_physical_arrow_types() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "typed-query-values", "/tmp").await?;
    let ws_path = "spaces/typed-query-values";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Typed",
            "fields": {
                "Uid": {"type": "uuid"},
                "Blob": {"type": "binary"},
                "Labels": {"type": "list", "items": {"type": "string"}},
            },
        }),
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "typed-values-1",
        "---\nform: Typed\n---\n# Typed values\n\n## Uid\nA7F9F5D2-8B7E-4DB1-9B0A-0E9A2B3F4C5D\n\n## Blob\nhex:64617461\n\n## Labels\n- Alpha\n- Beta",
        "author",
        &FakeIntegrityProvider,
    )
    .await?;

    let rows = index::query_index(&op, ws_path, r#"{"form":"Typed"}"#).await?;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["properties"]["Uid"],
        "a7f9f5d2-8b7e-4db1-9b0a-0e9a2b3f4c5d"
    );
    assert_eq!(rows[0]["properties"]["Blob"], "base64:ZGF0YQ==");

    for query in [
        serde_json::json!({"form": "Typed", "Uid": "a7f9f5d2-8b7e-4db1-9b0a-0e9a2b3f4c5d"}),
        serde_json::json!({"form": "Typed", "Blob": "base64:ZGF0YQ=="}),
        serde_json::json!({"form": "Typed", "Labels": {"$contains": "Alpha"}}),
    ] {
        assert_eq!(
            index::query_index(&op, ws_path, &query.to_string())
                .await?
                .len(),
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn markdown_typed_lists_use_the_form_item_type_for_round_trip_and_contains(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "markdown-typed-lists", "/tmp").await?;
    let ws_path = "spaces/markdown-typed-lists";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Project",
            "fields": {"Name": {"type": "string"}},
        }),
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "TypedLists",
            "fields": {
                "Integers": {"type": "list", "items": {"type": "integer"}},
                "Longs": {"type": "list", "items": {"type": "long"}},
                "Floats": {"type": "list", "items": {"type": "float"}},
                "Doubles": {"type": "list", "items": {"type": "double"}},
                "Booleans": {"type": "list", "items": {"type": "boolean"}},
                "Dates": {"type": "list", "items": {"type": "date"}},
                "Times": {"type": "list", "items": {"type": "time"}},
                "Timestamps": {"type": "list", "items": {"type": "timestamp"}},
                "TimestampTzs": {"type": "list", "items": {"type": "timestamp_tz"}},
                "TimestampNss": {"type": "list", "items": {"type": "timestamp_ns"}},
                "TimestampTzNss": {"type": "list", "items": {"type": "timestamp_tz_ns"}},
                "Uuids": {"type": "list", "items": {"type": "uuid"}},
                "Binaries": {"type": "list", "items": {"type": "binary"}},
                "Projects": {
                    "type": "list",
                    "items": {"type": "row_reference", "target_form": "Project"},
                },
                "Attachments": {"type": "list", "items": {"type": "asset_reference"}},
            },
        }),
    )
    .await?;

    entry::create_entry(
        &op,
        ws_path,
        "project-1",
        "---\nform: Project\n---\n# Project\n\n## Name\nExample",
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    let reference = asset::save_asset(&op, ws_path, "typed-list.bin", b"typed list").await?;
    let reference_json = serde_json::to_string(&reference)?;
    let content = format!(
        "---\nform: TypedLists\n---\n# Typed lists\n\n\
## Integers\n- 1\n- 2\n- null\n\n\
## Longs\n- 3000000000\n- 4000000000\n\n\
## Floats\n- 1.25\n- 2.5\n\n\
## Doubles\n- 3.5\n- 4.75\n\n\
## Booleans\n- true\n- false\n\n\
## Dates\n- 2024-01-02\n\n\
## Times\n- 03:04:05.123456\n\n\
## Timestamps\n- 2024-01-02T03:04:05.123456\n\n\
## TimestampTzs\n- 2024-01-02T03:04:05.123456+09:00\n\n\
## TimestampNss\n- 2024-01-02T03:04:05.123456789\n\n\
## TimestampTzNss\n- 2024-01-02T03:04:05.123456789+09:00\n\n\
## Uuids\n- A7F9F5D2-8B7E-4DB1-9B0A-0E9A2B3F4C5D\n\n\
## Binaries\n- base64:ZGF0YQ==\n\n\
## Projects\n- project-1\n\n\
## Attachments\n- {reference_json}"
    );
    entry::create_entry(
        &op,
        ws_path,
        "typed-list-entry",
        &content,
        "author",
        &FakeIntegrityProvider,
    )
    .await?;

    let rows = index::query_index(&op, ws_path, r#"{"form":"TypedLists"}"#).await?;
    assert_eq!(rows.len(), 1);
    let properties = &rows[0]["properties"];
    assert_eq!(properties["Integers"], serde_json::json!([1, 2, null]));
    assert_eq!(
        properties["Longs"],
        serde_json::json!([3000000000_i64, 4000000000_i64])
    );
    assert_eq!(properties["Floats"], serde_json::json!([1.25, 2.5]));
    assert_eq!(properties["Booleans"], serde_json::json!([true, false]));
    assert_eq!(properties["Dates"], serde_json::json!(["2024-01-02"]));
    assert_eq!(properties["Times"], serde_json::json!(["03:04:05.123456"]));
    assert_eq!(
        properties["TimestampTzs"],
        serde_json::json!(["2024-01-01T18:04:05.123456+00:00"])
    );
    assert_eq!(
        properties["TimestampTzNss"],
        serde_json::json!(["2024-01-01T18:04:05.123456789Z"])
    );
    assert_eq!(
        properties["Uuids"],
        serde_json::json!(["a7f9f5d2-8b7e-4db1-9b0a-0e9a2b3f4c5d"])
    );
    assert_eq!(
        properties["Binaries"],
        serde_json::json!(["base64:ZGF0YQ=="])
    );
    assert_eq!(properties["Projects"], serde_json::json!(["project-1"]));
    assert_eq!(
        properties["Attachments"][0]["asset_id"],
        reference.asset_id.to_string()
    );

    for query in [
        serde_json::json!({"form": "TypedLists", "Integers": {"$contains": 2}}),
        serde_json::json!({"form": "TypedLists", "Longs": {"$contains": 3000000000_i64}}),
        serde_json::json!({"form": "TypedLists", "Floats": {"$contains": 1.25}}),
        serde_json::json!({"form": "TypedLists", "Doubles": {"$contains": 3.5}}),
        serde_json::json!({"form": "TypedLists", "Booleans": {"$contains": true}}),
        serde_json::json!({"form": "TypedLists", "Dates": {"$contains": "2024-01-02"}}),
        serde_json::json!({"form": "TypedLists", "Times": {"$contains": "03:04:05.123456"}}),
        serde_json::json!({"form": "TypedLists", "Timestamps": {"$contains": "2024-01-02T03:04:05.123456"}}),
        serde_json::json!({"form": "TypedLists", "TimestampTzs": {"$contains": "2024-01-01T18:04:05.123456+00:00"}}),
        serde_json::json!({"form": "TypedLists", "TimestampNss": {"$contains": "2024-01-02T03:04:05.123456789"}}),
        serde_json::json!({"form": "TypedLists", "TimestampTzNss": {"$contains": "2024-01-01T18:04:05.123456789Z"}}),
        serde_json::json!({"form": "TypedLists", "Uuids": {"$contains": "a7f9f5d2-8b7e-4db1-9b0a-0e9a2b3f4c5d"}}),
        serde_json::json!({"form": "TypedLists", "Binaries": {"$contains": "base64:ZGF0YQ=="}}),
        serde_json::json!({"form": "TypedLists", "Projects": {"$contains": "project-1"}}),
        serde_json::json!({"form": "TypedLists", "Attachments": {"$contains": {"asset_id": reference.asset_id.to_string()}}}),
    ] {
        assert_eq!(
            index::query_index(&op, ws_path, &query.to_string())
                .await?
                .len(),
            1,
            "typed-list predicate must use the canonical item type: {query}"
        );
    }

    let revision_id = entry::get_entry_content(&op, ws_path, "typed-list-entry")
        .await?
        .revision_id;
    let updated = content.replace("- 2\n- null", "- 3\n- null");
    entry::update_entry(
        &op,
        ws_path,
        "typed-list-entry",
        &updated,
        Some(&revision_id),
        "author",
        &FakeIntegrityProvider,
    )
    .await?;
    let updated_rows = index::query_index(&op, ws_path, r#"{"form":"TypedLists"}"#).await?;
    assert_eq!(
        updated_rows[0]["properties"]["Integers"],
        serde_json::json!([1, 3, null])
    );
    Ok(())
}

#[tokio::test]
/// REQ-ENTRY-003
async fn test_entry_req_entry_003_update_entry_success() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let entry_id = "entry-2";

    // Create initial note
    let meta = entry::create_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Initial\n\n## Body\nContent",
        "author1",
        &integrity,
    )
    .await?;

    // We need to fetch the revision ID.
    let content_info = entry::get_entry_content(&op, ws_path, entry_id).await?;
    let initial_revision = content_info.revision_id;

    // Update note
    let new_content = "---\nform: Entry\n---\n# Updated\n\n## Body\nContent";
    let new_meta = entry::update_entry(
        &op,
        ws_path,
        entry_id,
        new_content,
        Some(&initial_revision),
        "author1",
        &integrity,
    )
    .await?;

    // Verify update
    let updated_at = new_meta.get("updated_at").and_then(|v| v.as_f64()).unwrap();
    assert_ne!(meta.updated_at, updated_at);

    let current_content = entry::get_entry_content(&op, ws_path, entry_id).await?;
    assert_eq!(current_content.markdown, new_content);
    assert_eq!(current_content.parent_revision_id, Some(initial_revision));

    Ok(())
}

#[tokio::test]
/// REQ-ENTRY-002
async fn test_entry_req_entry_002_update_entry_conflict() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let entry_id = "entry-3";

    entry::create_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Content",
        "author1",
        &integrity,
    )
    .await?;

    // Try to update with wrong parent revision
    let wrong_revision = "wrong-rev";
    let result = entry::update_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# New Content",
        Some(wrong_revision),
        "author1",
        &integrity,
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("conflict"));

    Ok(())
}

#[tokio::test]
/// REQ-ENTRY-005
async fn test_entry_req_entry_005_entry_history_append() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let entry_id = "entry-history";

    // Version 1
    entry::create_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Version 1",
        "author1",
        &integrity,
    )
    .await?;

    let content_v1 = entry::get_entry_content(&op, ws_path, entry_id).await?;
    let rev_v1 = content_v1.revision_id;

    // Version 2
    entry::update_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Version 2",
        Some(&rev_v1),
        "author1",
        &integrity,
    )
    .await?;

    let history = entry::get_entry_history(&op, ws_path, entry_id).await?;
    let revisions = history.get("revisions").unwrap().as_array().unwrap();
    assert_eq!(revisions.len(), 2);
    assert!(revisions
        .iter()
        .any(|rev| rev.get("revision_id").and_then(|v| v.as_str()) == Some(rev_v1.as_str())));

    Ok(())
}

#[tokio::test]
/// REQ-ENTRY-005
async fn test_entry_req_entry_005_revision_content_renders_requested_revision_sections(
) -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let entry_id = "entry-revision-content";

    entry::create_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Version 1\n\n## Body\nAlpha",
        "author1",
        &integrity,
    )
    .await?;

    let content_v1 = entry::get_entry_content(&op, ws_path, entry_id).await?;
    let rev_v1 = content_v1.revision_id;

    entry::update_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Version 2\n\n## Body\nBeta",
        Some(&rev_v1),
        "author1",
        &integrity,
    )
    .await?;

    let revision_content =
        entry::get_entry_revision_content(&op, ws_path, entry_id, &rev_v1).await?;
    assert_eq!(revision_content.revision_id, rev_v1);
    assert!(revision_content.markdown.contains("---\nform: Entry\n---"));
    assert!(revision_content.markdown.contains("## Body\nAlpha"));
    assert!(!revision_content.markdown.contains("## Body\nBeta"));

    Ok(())
}

#[tokio::test]
/// REQ-ENTRY-004
async fn test_entry_req_entry_004_delete_entry() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-del", "/tmp").await?;
    let ws_path = "spaces/test-del";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let entry_id = "entry-del";

    entry::create_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Content",
        "author",
        &integrity,
    )
    .await?;

    // Delete
    entry::delete_entry(&op, ws_path, entry_id, false, "deleter").await?;

    // Verify the tombstone is hidden from current listings but retained in history.
    let list = entry::list_entries(&op, ws_path).await?;
    let ids: Vec<String> = list
        .iter()
        .filter_map(|val| {
            val.get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    assert!(!ids.contains(&entry_id.to_string()));

    let history = entry::get_entry_history(&op, ws_path, entry_id).await?;
    let revisions = history["revisions"]
        .as_array()
        .expect("deleted entry history");
    assert_eq!(revisions.len(), 2);
    let tombstone = revisions.last().expect("delete revision");
    assert_eq!(tombstone["operation"], "delete");
    assert_eq!(tombstone["deleted_by"], "deleter");

    Ok(())
}

#[tokio::test]
/// REQ-FORM-009
async fn entry_attribution_is_consistent_across_lifecycle() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "entry-attribution", "/tmp").await?;
    let ws_path = "spaces/entry-attribution";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let entry_id = "attributed-entry";
    let original_content = "---\nform: Entry\n---\n# Original\n\n## Body\nCreated";

    entry::create_entry(
        &op,
        ws_path,
        entry_id,
        original_content,
        "creator",
        &integrity,
    )
    .await?;
    let created = entry::get_entry_content(&op, ws_path, entry_id).await?;
    assert_eq!(created.author, "creator");
    assert_eq!(created.updated_by, "creator");
    assert_eq!(created.deleted_by, None);

    entry::update_entry(
        &op,
        ws_path,
        entry_id,
        "---\nform: Entry\n---\n# Updated\n\n## Body\nEdited",
        Some(&created.revision_id),
        "editor",
        &integrity,
    )
    .await?;
    let updated = entry::get_entry_content(&op, ws_path, entry_id).await?;
    assert_eq!(updated.author, "creator");
    assert_eq!(updated.updated_by, "editor");
    assert_eq!(updated.deleted_by, None);

    entry::delete_entry(&op, ws_path, entry_id, false, "deleter").await?;
    let deleted_history = entry::get_entry_history(&op, ws_path, entry_id).await?;
    let deleted_revision_id = deleted_history["revisions"]
        .as_array()
        .and_then(|revisions| revisions.last())
        .and_then(|revision| revision["revision_id"].as_str())
        .expect("delete revision")
        .to_string();
    let deleted_revision =
        entry::get_entry_revision(&op, ws_path, entry_id, &deleted_revision_id).await?;
    assert_eq!(deleted_revision["author"], "creator");
    assert_eq!(deleted_revision["updated_by"], "deleter");
    assert_eq!(deleted_revision["deleted_by"], "deleter");
    assert_eq!(deleted_revision["state"]["author"], "creator");
    assert_eq!(deleted_revision["state"]["updated_by"], "deleter");
    assert_eq!(deleted_revision["state"]["deleted_by"], "deleter");

    entry::restore_entry(
        &op,
        ws_path,
        entry_id,
        &created.revision_id,
        "restorer",
        &integrity,
    )
    .await?;
    let restored = entry::get_entry_content(&op, ws_path, entry_id).await?;
    assert_eq!(restored.author, "creator");
    assert_eq!(restored.updated_by, "restorer");
    assert_eq!(restored.deleted_by, None);
    assert!(restored.markdown.contains("Created"));

    let historical_delete =
        entry::get_entry_revision(&op, ws_path, entry_id, &deleted_revision_id).await?;
    assert_eq!(historical_delete["author"], "creator");
    assert_eq!(historical_delete["updated_by"], "deleter");
    assert_eq!(historical_delete["deleted_by"], "deleter");
    Ok(())
}

#[tokio::test]
/// REQ-ENTRY-006
async fn test_entry_req_entry_006_extract_h2_headers() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-extract", "/tmp").await?;
    let ws_path = "spaces/test-extract";
    let integrity = FakeIntegrityProvider;
    let entry_id = "entry-extract";

    let class_def = serde_json::json!({
        "name": "Meeting",
        "template": "# Meeting\n\n## Date\n\n## Summary\n",
        "fields": {
            "Date": {"type": "date"},
            "Summary": {"type": "string"},
        },
    });
    form::upsert_form(&op, ws_path, &class_def).await?;
    let content = "---\nform: Meeting\n---\n# Title\n\n## Date\n2025-01-01\n\n## Summary\nText";
    entry::create_entry(&op, ws_path, entry_id, content, "author", &integrity).await?;

    let list = entry::list_entries(&op, ws_path).await?;
    let props = list
        .iter()
        .find(|entry| entry.get("id").and_then(|value| value.as_str()) == Some(entry_id))
        .and_then(|entry| entry.get("properties"))
        .and_then(|value| value.as_object())
        .expect("persisted extracted properties");

    assert!(props.contains_key("Date"));
    assert_eq!(props.get("Date").unwrap().as_str().unwrap(), "2025-01-01");
    assert!(props.contains_key("Summary"));

    Ok(())
}

#[tokio::test]
/// REQ-FORM-004
async fn test_entry_req_form_004_deny_extra_attributes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-extra-deny", "/tmp").await?;
    let ws_path = "spaces/test-extra-deny";
    let integrity = FakeIntegrityProvider;

    let form_def = serde_json::json!({
        "name": "Entry",
        "template": "# Entry\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}},
        "allow_extra_attributes": "deny",
    });
    form::upsert_form(&op, ws_path, &form_def).await?;

    let content = "---\nform: Entry\n---\n# Title\n\n## Body\nContent\n\n## Extra\nValue";
    let result = entry::create_entry(
        &op,
        ws_path,
        "entry-extra-deny",
        content,
        "author",
        &integrity,
    )
    .await;

    let error = result.expect_err("unknown form fields must be rejected");
    let app_error = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("unknown form fields must remain typed application errors");
    assert_eq!(
        app_error.code(),
        ugoite_core::error::ErrorCode::UnknownFormFields
    );
    assert_eq!(app_error.message(), "Entry contains unknown form fields");

    entry::create_entry(
        &op,
        ws_path,
        "entry-extra-deny-update",
        "---\nform: Entry\n---\n# Title\n\n## Body\nContent",
        "author",
        &integrity,
    )
    .await?;
    let current = entry::get_entry_content(&op, ws_path, "entry-extra-deny-update").await?;
    let update_result = entry::update_entry(
        &op,
        ws_path,
        "entry-extra-deny-update",
        "---\nform: Entry\n---\n# Title\n\n## Body\nUpdated\n\n## Extra\nValue",
        Some(&current.revision_id),
        "author",
        &integrity,
    )
    .await;
    let update_error = update_result.expect_err("unknown form fields must be rejected on update");
    let update_app_error = update_error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("unknown form fields must remain typed application errors on update");
    assert_eq!(
        update_app_error.code(),
        ugoite_core::error::ErrorCode::UnknownFormFields
    );

    Ok(())
}

#[tokio::test]
/// REQ-FORM-004
async fn test_entry_req_form_004_allow_extra_attributes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-extra-allow", "/tmp").await?;
    let ws_path = "spaces/test-extra-allow";
    let integrity = FakeIntegrityProvider;

    for policy in ["allow_json", "allow_columns"] {
        let form_def = serde_json::json!({
            "name": "Entry",
            "template": "# Entry\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
            "allow_extra_attributes": policy,
        });
        form::upsert_form(&op, ws_path, &form_def).await?;

        let entry_id = format!("entry-extra-{}", policy);
        let content = "---\nform: Entry\n---\n# Title\n\n## Body\nContent\n\n## Extra\nValue";
        entry::create_entry(&op, ws_path, &entry_id, content, "author", &integrity).await?;

        let content_info = entry::get_entry_content(&op, ws_path, &entry_id).await?;
        assert!(content_info.markdown.contains("## Extra"));
        assert!(content_info.markdown.contains("Value"));

        let list = entry::list_entries(&op, ws_path).await?;
        let extra_prop = list
            .iter()
            .find(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(entry_id.as_str()))
            .and_then(|entry| entry.get("properties"))
            .and_then(|props| props.get("Extra"));
        assert!(extra_prop.is_some());
    }

    Ok(())
}
