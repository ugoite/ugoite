mod common;
use _ugoite_core::asset;
use _ugoite_core::entry;
use _ugoite_core::form;
use _ugoite_core::integrity::FakeIntegrityProvider;
use _ugoite_core::space;
use common::setup_operator;

async fn ensure_entry_form(op: &opendal::Operator, ws_path: &str) -> anyhow::Result<()> {
    let form_def = serde_json::json!({
        "name": "Entry",
        "template": "# Entry\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}},
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

    let props = _ugoite_core::index::extract_properties(content);
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
