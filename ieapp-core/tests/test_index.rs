mod common;
use _ieapp_core::index;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
/// REQ-IDX-001
async fn test_index_req_idx_001_reindex_writes_index_files() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
    let ws_path = "workspaces/test-workspace";

    index::reindex_all(&op, ws_path).await?;

    // Verify index files exist
    assert!(op.exists(&format!("{}/index/index.json", ws_path)).await?);

    Ok(())
}

#[test]
/// REQ-IDX-001
fn test_index_req_idx_001_extract_properties_returns_object() {
    let markdown = "# Title";
    let props = index::extract_properties(markdown);
    assert!(props.is_object());
}
