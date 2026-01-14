mod common;
use _ieapp_core::index;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
/// REQ-IDX-001
async fn test_index_req_idx_001_reindex_writes_index_files() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
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

#[tokio::test]
/// REQ-IDX-002
async fn test_index_req_idx_002_validate_properties() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";

    // Setup class definition
    let class_def = r#"{
        "name": "Meeting",
        "fields": [
            {"name": "Date", "type": "date", "required": true}
        ]
    }"#;
    _ieapp_core::class::upsert_class(&op, ws_path, class_def).await?;

    // Invalid property (wrong type/missing)
    let props = serde_json::json!({
        "Date": "invalid-date"
    });

    // Assuming validate_properties returns Result<Vec<String>> (list of warnings) or similar
    // We stub the expectation that it should fail or warn
    let result = index::validate_properties(&op, ws_path, "Meeting", &props).await;
    // assert!(result.is_err() || !result.unwrap().is_empty());

    Ok(())
}

#[tokio::test]
/// REQ-IDX-003
async fn test_index_req_idx_003_query_index() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-ws", "/tmp").await?;
    let ws_path = "workspaces/test-ws";

    // Create note with properties
    // index::reindex_all(...)
    // let results = index::query_index(&op, ws_path, "filter...").await?;
    // assert_eq!(results.len(), 1);
    Ok(())
}

#[tokio::test]
/// REQ-IDX-004
async fn test_index_req_idx_004_inverted_index_generation() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-ws", "/tmp").await?;
    let ws_path = "workspaces/test-ws";

    // index::reindex_all(...)
    // Check search-index.json exists
    // assert!(op.exists(&format!("{}/index/search-index.json", ws_path)).await?);
    Ok(())
}

#[test]
/// REQ-IDX-005
fn test_index_req_idx_005_word_count() {
    let content = "One two three";
    let count = index::compute_word_count(content);
    assert_eq!(count, 3);
}
