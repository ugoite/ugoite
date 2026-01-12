mod common;
use _ieapp_core::attachment;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
/// REQ-ATT-001
async fn test_attachment_req_att_001_create_attachment() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";

    let content = b"fake image content";
    attachment::save_attachment(&op, ws_path, "image.png", content).await?;

    // Check if it exists in attachment folder (path might need check against implementation spec)
    assert!(
        op.exists(&format!("{}/attachments/image.png", ws_path))
            .await?
    );

    let listed = attachment::list_attachments(&op, ws_path).await?;
    assert_eq!(listed, vec!["image.png".to_string()]);

    Ok(())
}

#[tokio::test]
/// REQ-ATT-001
async fn test_attachment_req_att_001_delete_attachment() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
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
