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

    note::create_note(&op, ws_path, note_id, content, &integrity).await?;

    let note_path = format!("{}/notes/{}", ws_path, note_id);
    assert!(op.exists(&note_path).await?);
    assert!(op.exists(&format!("{}/meta.json", note_path)).await?);
    assert!(op.exists(&format!("{}/content.json", note_path)).await?);
    assert!(
        op.exists(&format!("{}/history/index.json", note_path))
            .await?
    );

    Ok(())
}
