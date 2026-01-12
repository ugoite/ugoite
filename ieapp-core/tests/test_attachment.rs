mod common;
use _ieapp_core::attachment;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
async fn test_create_attachment() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
    let ws_path = "workspaces/test-workspace";

    let content = b"fake image content";
    attachment::save_attachment(&op, ws_path, "image.png", content).await?;

    // Check if it exists in attachment folder (path might need check against implementation spec)
    assert!(
        op.exists(&format!("{}/attachments/image.png", ws_path))
            .await?
    );

    Ok(())
}

#[tokio::test]
async fn test_delete_attachment() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
    let ws_path = "workspaces/test-workspace";

    attachment::save_attachment(&op, ws_path, "file.txt", b"data").await?;

    assert!(
        op.exists(&format!("{}/attachments/file.txt", ws_path))
            .await?
    );

    attachment::delete_attachment(&op, ws_path, "file.txt").await?;

    assert!(
        !op.exists(&format!("{}/attachments/file.txt", ws_path))
            .await?
    );

    Ok(())
}
