mod common;
use _ieapp_core::asset;
use _ieapp_core::space;
use common::setup_operator;

#[tokio::test]
/// REQ-ASSET-001
async fn test_asset_req_asset_001_create_asset() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    let content = b"fake image content";
    let info = asset::save_asset(&op, ws_path, "image.png", content).await?;

    assert!(op.exists(&format!("{}/{}", ws_path, info.path)).await?);

    let listed = asset::list_assets(&op, ws_path).await?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, info.id);
    assert_eq!(listed[0].name, "image.png");

    Ok(())
}

#[tokio::test]
/// REQ-ASSET-001
async fn test_asset_req_asset_001_delete_asset() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    let info = asset::save_asset(&op, ws_path, "file.txt", b"data").await?;

    assert!(op.exists(&format!("{}/{}", ws_path, info.path)).await?);

    asset::delete_asset(&op, ws_path, &info.id).await?;

    assert!(!op.exists(&format!("{}/{}", ws_path, info.path)).await?);

    Ok(())
}
