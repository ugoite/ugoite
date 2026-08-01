mod common;
use common::setup_operator;
use ugoite_iceberg::asset;
use ugoite_iceberg::entry;
use ugoite_iceberg::form;
use ugoite_iceberg::index;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::space;

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
        None,
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
        None,
        &integrity,
    )
    .await
    .expect_err("invalid timestamp input must be rejected");
    let app_error = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("entry validation failures must remain typed application errors");
    assert_eq!(
        app_error.code(),
        ugoite_core::error::ErrorCode::InvalidInput
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
        None,
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
        None,
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
        None,
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
        None,
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
    entry::delete_entry(&op, ws_path, entry_id, false).await?;

    // Verify
    // op.exists() should match implementation (tombstone or file removal)
    // If tombstone:
    // let meta = note::get_note_meta(...)
    // assert!(meta.deleted);
    // If removal from list:
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

    let props = ugoite_iceberg::index::extract_properties(content);
    let props = props.as_object().unwrap();

    assert!(props.contains_key("Date"));
    assert_eq!(props.get("Date").unwrap().as_str().unwrap(), "2025-01-01");
    assert!(props.contains_key("Summary"));

    Ok(())
}

#[tokio::test]
/// REQ-LNK-004
async fn test_entry_req_lnk_004_normalize_ugoite_link_uris() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-links", "/tmp").await?;
    let ws_path = "spaces/test-links";
    let integrity = FakeIntegrityProvider;

    let form_def = serde_json::json!({
        "name": "Entry",
        "fields": {
            "Body": {"type": "markdown"}
        }
    });
    form::upsert_form(&op, ws_path, &form_def).await?;

    let content = "---\nform: Entry\n---\n# Title\n\n## Body\nSee [ref](ugoite://entries/entry-123), [file](ugoite://assets/asset-456), and [query](ugoite://entry?id=entry-789).";
    entry::create_entry(&op, ws_path, "entry-links", content, "author", &integrity).await?;

    let content_info = entry::get_entry_content(&op, ws_path, "entry-links").await?;
    assert!(content_info.markdown.contains("ugoite://entry/entry-123"));
    assert!(content_info.markdown.contains("ugoite://asset/asset-456"));
    assert!(content_info.markdown.contains("ugoite://entry/entry-789"));

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

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unknown form fields"));

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

#[tokio::test]
/// REQ-ENTRY-008
async fn test_entry_req_entry_008_assets_linking() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-assets", "/tmp").await?;
    let ws_path = "spaces/test-assets";
    ensure_entry_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let info = asset::save_asset(&op, ws_path, "file.txt", b"data").await?;
    entry::create_entry(
        &op,
        ws_path,
        "entry-asset",
        "---\nform: Entry\n---\n# Assets",
        "author",
        &integrity,
    )
    .await?;

    let current = entry::get_entry_content(&op, ws_path, "entry-asset").await?;
    let assets = vec![serde_json::json!({
        "id": info.id,
        "name": info.name,
        "path": info.path,
    })];

    entry::update_entry(
        &op,
        ws_path,
        "entry-asset",
        "---\nform: Entry\n---\n# Assets\nwith file",
        Some(&current.revision_id),
        "author",
        Some(assets),
        &integrity,
    )
    .await?;

    let entry_json = entry::get_entry(&op, ws_path, "entry-asset").await?;
    let assets = entry_json
        .get("assets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(assets
        .iter()
        .any(|asset| asset.get("id").and_then(|v| v.as_str()) == Some(info.id.as_str())));

    Ok(())
}
