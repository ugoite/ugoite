mod common;
use _ieapp_core::index;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
async fn test_indexer_basic() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
    let ws_path = "workspaces/test-workspace";

    index::reindex_all(&op, ws_path).await?;

    // Verify index files exist
    assert!(op.exists(&format!("{}/index/index.json", ws_path)).await?);

    Ok(())
}
