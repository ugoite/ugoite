mod common;
use _ieapp_core::integrity::FakeIntegrityProvider;
use _ieapp_core::note;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
async fn test_create_note_basic() -> anyhow::Result<()> {
    let op = setup_operator()?;
    // We assume workspace exists
    workspace::create_workspace(&op, "test-workspace").await?;
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
async fn test_update_note_success() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
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
        &integrity,
    )
    .await?;

    // Verify update
    assert_ne!(meta.updated_at, new_meta.updated_at);

    let current_content = note::get_note_content(&op, ws_path, note_id).await?;
    assert_eq!(current_content.markdown, new_content);
    assert_eq!(current_content.parent_revision_id, Some(initial_revision));

    Ok(())
}

#[tokio::test]
async fn test_update_note_conflict() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
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
        &integrity,
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("conflict"));

    Ok(())
}
