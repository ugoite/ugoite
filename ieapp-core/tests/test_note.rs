mod common;
use _ieapp_core::attachment;
use _ieapp_core::class;
use _ieapp_core::integrity::FakeIntegrityProvider;
use _ieapp_core::note;
use _ieapp_core::workspace;
use common::setup_operator;

async fn ensure_note_class(op: &opendal::Operator, ws_path: &str) -> anyhow::Result<()> {
    let class_def = serde_json::json!({
        "name": "Note",
        "template": "# Note\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}},
    });
    class::upsert_class(op, ws_path, &class_def).await?;
    Ok(())
}

#[tokio::test]
/// REQ-NOTE-001
async fn test_note_req_note_001_create_note_basic() -> anyhow::Result<()> {
    let op = setup_operator()?;
    // We assume workspace exists
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";
    ensure_note_class(&op, ws_path).await?;

    let integrity = FakeIntegrityProvider;
    let content = "---\nclass: Note\n---\n# My Note\n\nHello World";
    let note_id = "note-1";

    note::create_note(&op, ws_path, note_id, content, "test-author", &integrity).await?;

    let content_info = note::get_note_content(&op, ws_path, note_id).await?;
    assert!(!content_info.revision_id.is_empty());
    let history = note::get_note_history(&op, ws_path, note_id).await?;
    let revisions = history.get("revisions").and_then(|v| v.as_array()).unwrap();
    assert_eq!(revisions.len(), 1);

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-003
async fn test_note_req_note_003_update_note_success() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";
    ensure_note_class(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let note_id = "note-2";

    // Create initial note
    let meta = note::create_note(
        &op,
        ws_path,
        note_id,
        "---\nclass: Note\n---\n# Initial\n\n## Body\nContent",
        "author1",
        &integrity,
    )
    .await?;

    // We need to fetch the revision ID.
    let content_info = note::get_note_content(&op, ws_path, note_id).await?;
    let initial_revision = content_info.revision_id;

    // Update note
    let new_content = "---\nclass: Note\n---\n# Updated\n\n## Body\nContent";
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
    ensure_note_class(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let note_id = "note-3";

    note::create_note(
        &op,
        ws_path,
        note_id,
        "---\nclass: Note\n---\n# Content",
        "author1",
        &integrity,
    )
    .await?;

    // Try to update with wrong parent revision
    let wrong_revision = "wrong-rev";
    let result = note::update_note(
        &op,
        ws_path,
        note_id,
        "---\nclass: Note\n---\n# New Content",
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
    ensure_note_class(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let note_id = "note-history";

    // Version 1
    note::create_note(
        &op,
        ws_path,
        note_id,
        "---\nclass: Note\n---\n# Version 1",
        "author1",
        &integrity,
    )
    .await?;

    let content_v1 = note::get_note_content(&op, ws_path, note_id).await?;
    let rev_v1 = content_v1.revision_id;

    // Version 2
    note::update_note(
        &op,
        ws_path,
        note_id,
        "---\nclass: Note\n---\n# Version 2",
        Some(&rev_v1),
        "author1",
        None,
        &integrity,
    )
    .await?;

    let history = note::get_note_history(&op, ws_path, note_id).await?;
    let revisions = history.get("revisions").unwrap().as_array().unwrap();
    assert_eq!(revisions.len(), 2);
    assert!(revisions
        .iter()
        .any(|rev| rev.get("revision_id").and_then(|v| v.as_str()) == Some(rev_v1.as_str())));

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-004
async fn test_note_req_note_004_delete_note() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-del", "/tmp").await?;
    let ws_path = "workspaces/test-del";
    ensure_note_class(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let note_id = "note-del";

    note::create_note(
        &op,
        ws_path,
        note_id,
        "---\nclass: Note\n---\n# Content",
        "author",
        &integrity,
    )
    .await?;

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

    let class_def = serde_json::json!({
        "name": "Meeting",
        "template": "# Meeting\n\n## Date\n\n## Summary\n",
        "fields": {
            "Date": {"type": "date"},
            "Summary": {"type": "string"},
        },
    });
    class::upsert_class(&op, ws_path, &class_def).await?;
    let content = "---\nclass: Meeting\n---\n# Title\n\n## Date\n2025-01-01\n\n## Summary\nText";
    note::create_note(&op, ws_path, note_id, content, "author", &integrity).await?;

    let props = _ieapp_core::index::extract_properties(content);
    let props = props.as_object().unwrap();

    assert!(props.contains_key("Date"));
    assert_eq!(props.get("Date").unwrap().as_str().unwrap(), "2025-01-01");
    assert!(props.contains_key("Summary"));

    Ok(())
}

#[tokio::test]
/// REQ-LNK-004
async fn test_note_req_lnk_004_normalize_ieapp_link_uris() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-links", "/tmp").await?;
    let ws_path = "workspaces/test-links";
    let integrity = FakeIntegrityProvider;

    let class_def = serde_json::json!({
        "name": "Note",
        "fields": {
            "Body": {"type": "markdown"}
        }
    });
    class::upsert_class(&op, ws_path, &class_def).await?;

    let content = "---\nclass: Note\n---\n# Title\n\n## Body\nSee [ref](ieapp://notes/note-123), [file](ieapp://attachments/att-456), and [query](ieapp://note?id=note-789).";
    note::create_note(&op, ws_path, "note-links", content, "author", &integrity).await?;

    let content_info = note::get_note_content(&op, ws_path, "note-links").await?;
    assert!(content_info.markdown.contains("ieapp://note/note-123"));
    assert!(content_info.markdown.contains("ieapp://attachment/att-456"));
    assert!(content_info.markdown.contains("ieapp://note/note-789"));

    Ok(())
}

#[tokio::test]
/// REQ-CLS-004
async fn test_note_req_cls_004_deny_extra_attributes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-extra-deny", "/tmp").await?;
    let ws_path = "workspaces/test-extra-deny";
    let integrity = FakeIntegrityProvider;

    let class_def = serde_json::json!({
        "name": "Note",
        "template": "# Note\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}},
        "allow_extra_attributes": "deny",
    });
    class::upsert_class(&op, ws_path, &class_def).await?;

    let content = "---\nclass: Note\n---\n# Title\n\n## Body\nContent\n\n## Extra\nValue";
    let result = note::create_note(
        &op,
        ws_path,
        "note-extra-deny",
        content,
        "author",
        &integrity,
    )
    .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Unknown class fields"));

    Ok(())
}

#[tokio::test]
/// REQ-CLS-004
async fn test_note_req_cls_004_allow_extra_attributes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-extra-allow", "/tmp").await?;
    let ws_path = "workspaces/test-extra-allow";
    let integrity = FakeIntegrityProvider;

    for policy in ["allow_json", "allow_columns"] {
        let class_def = serde_json::json!({
            "name": "Note",
            "template": "# Note\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
            "allow_extra_attributes": policy,
        });
        class::upsert_class(&op, ws_path, &class_def).await?;

        let note_id = format!("note-extra-{}", policy);
        let content = "---\nclass: Note\n---\n# Title\n\n## Body\nContent\n\n## Extra\nValue";
        note::create_note(&op, ws_path, &note_id, content, "author", &integrity).await?;

        let content_info = note::get_note_content(&op, ws_path, &note_id).await?;
        assert!(content_info.markdown.contains("## Extra"));
        assert!(content_info.markdown.contains("Value"));

        let list = note::list_notes(&op, ws_path).await?;
        let extra_prop = list
            .iter()
            .find(|note| note.get("id").and_then(|v| v.as_str()) == Some(note_id.as_str()))
            .and_then(|note| note.get("properties"))
            .and_then(|props| props.get("Extra"));
        assert!(extra_prop.is_some());
    }

    Ok(())
}

#[tokio::test]
/// REQ-NOTE-008
async fn test_note_req_note_008_attachments_linking() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-attachments", "/tmp").await?;
    let ws_path = "workspaces/test-attachments";
    ensure_note_class(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let info = attachment::save_attachment(&op, ws_path, "file.txt", b"data").await?;
    note::create_note(
        &op,
        ws_path,
        "note-attach",
        "---\nclass: Note\n---\n# Attachments",
        "author",
        &integrity,
    )
    .await?;

    let current = note::get_note_content(&op, ws_path, "note-attach").await?;
    let attachments = vec![serde_json::json!({
        "id": info.id,
        "name": info.name,
        "path": info.path,
    })];

    note::update_note(
        &op,
        ws_path,
        "note-attach",
        "---\nclass: Note\n---\n# Attachments\nwith file",
        Some(&current.revision_id),
        "author",
        Some(attachments),
        &integrity,
    )
    .await?;

    let note_json = note::get_note(&op, ws_path, "note-attach").await?;
    let attachments = note_json
        .get("attachments")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(attachments
        .iter()
        .any(|att| att.get("id").and_then(|v| v.as_str()) == Some(info.id.as_str())));

    Ok(())
}
