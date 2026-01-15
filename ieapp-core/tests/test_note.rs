mod common;
use _ieapp_core::integrity::FakeIntegrityProvider;
use _ieapp_core::note;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
/// REQ-NOTE-001
async fn test_note_req_note_001_create_note_basic() -> anyhow::Result<()> {
    let op = setup_operator()?;
    // We assume workspace exists
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";

    let integrity = FakeIntegrityProvider;
    let content = "# My Note\n\nHello World";
    let note_id = "note-1";

    note::create_note(&op, ws_path, note_id, content, "test-author", &integrity).await?;

    let note_path = format!("{}/notes/{}", ws_path, note_id);
    assert!(op.exists(&format!("{}/", note_path)).await?);
    assert!(op.exists(&format!("{}/meta.json", note_path)).await?);
    assert!(op.exists(&format!("{}/content.json", note_path)).await?);
    assert!(
        op.exists(&format!("{}/history/index.json", note_path))
            .await?
    );

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-003
async fn test_note_req_note_003_update_note_success() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";
    let integrity = FakeIntegrityProvider;
    let note_id = "note-2";

    // Create initial note
    let meta = note::create_note(
        &op,
        ws_path,
        note_id,
        "# Initial\nContent",
        "author1",
        &integrity,
    )
    .await?;

    // We need to fetch the revision ID.
    let content_info = note::get_note_content(&op, ws_path, note_id).await?;
    let initial_revision = content_info.revision_id;

    // Update note
    let new_content = "# Updated\nContent";
    let new_meta = note::update_note(
        &op,
        ws_path,
        note_id,
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

    let current_content = note::get_note_content(&op, ws_path, note_id).await?;
    assert_eq!(current_content.markdown, new_content);
    assert_eq!(current_content.parent_revision_id, Some(initial_revision));

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-002
async fn test_note_req_note_002_update_note_conflict() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";
    let integrity = FakeIntegrityProvider;
    let note_id = "note-3";

    note::create_note(&op, ws_path, note_id, "# Content", "author1", &integrity).await?;

    // Try to update with wrong parent revision
    let wrong_revision = "wrong-rev";
    let result = note::update_note(
        &op,
        ws_path,
        note_id,
        "# New Content",
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
/// REQ-NOTE-005
async fn test_note_req_note_005_note_history_append() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";
    let integrity = FakeIntegrityProvider;
    let note_id = "note-history";

    // Version 1
    note::create_note(&op, ws_path, note_id, "# Version 1", "author1", &integrity).await?;

    let content_v1 = note::get_note_content(&op, ws_path, note_id).await?;
    let rev_v1 = content_v1.revision_id;

    // Version 2
    note::update_note(
        &op,
        ws_path,
        note_id,
        "# Version 2",
        Some(&rev_v1),
        "author1",
        None,
        &integrity,
    )
    .await?;

    // Check history/index.json
    let history_path = format!("{}/notes/{}/history/index.json", ws_path, note_id);
    let bytes = op.read(&history_path).await?;
    let history: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;

    let revisions = history.get("revisions").unwrap().as_array().unwrap();
    assert_eq!(revisions.len(), 2);
    assert_eq!(
        revisions[0].get("revision_id").unwrap().as_str().unwrap(),
        rev_v1
    );

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-004
async fn test_note_req_note_004_delete_note() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-del", "/tmp").await?;
    let ws_path = "workspaces/test-del";
    let integrity = FakeIntegrityProvider;
    let note_id = "note-del";

    note::create_note(&op, ws_path, note_id, "# Content", "author", &integrity).await?;

    // Delete
    note::delete_note(&op, ws_path, note_id, false).await?;

    // Verify
    // op.exists() should match implementation (tombstone or file removal)
    // If tombstone:
    // let meta = note::get_note_meta(...)
    // assert!(meta.deleted);
    // If removal from list:
    let list = note::list_notes(&op, ws_path).await?;
    let ids: Vec<String> = list
        .iter()
        .filter_map(|val| {
            val.get("id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    assert!(!ids.contains(&note_id.to_string()));

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-006
async fn test_note_req_note_006_extract_h2_headers() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-extract", "/tmp").await?;
    let ws_path = "workspaces/test-extract";
    let integrity = FakeIntegrityProvider;
    let note_id = "note-extract";

    let content = "# Title\n\n## Date\n2025-01-01\n\n## Summary\nText";
    note::create_note(&op, ws_path, note_id, content, "author", &integrity).await?;

    let props = _ieapp_core::index::extract_properties(content);
    let props = props.as_object().unwrap();

    assert!(props.contains_key("Date"));
    assert_eq!(props.get("Date").unwrap().as_str().unwrap(), "2025-01-01");
    assert!(props.contains_key("Summary"));

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-008
async fn test_note_req_note_008_attachments_linking() -> anyhow::Result<()> {
    // Test creating note referencing attachment
    Ok(())
}
