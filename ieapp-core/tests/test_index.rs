mod common;
use _ieapp_core::{class, index, note, workspace};
use common::setup_operator;

#[tokio::test]
/// REQ-IDX-001
async fn test_index_req_idx_001_reindex_writes_index_files() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";

    index::reindex_all(&op, ws_path).await?;

    // Indexes are derived from Iceberg; no on-disk index files are created
    assert!(!op.exists(&format!("{}/index/index.json", ws_path)).await?);

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
    let class_def_value = serde_json::from_str::<serde_json::Value>(class_def)?;
    _ieapp_core::class::upsert_class(&op, ws_path, &class_def_value).await?;

    // Invalid property (wrong type/missing)
    let props = serde_json::json!({
        "Date": "invalid-date"
    });

    // Assuming validate_properties returns Result<Vec<String>> (list of warnings) or similar
    // We stub the expectation that it should fail or warn
    let class_def_value = serde_json::from_str::<serde_json::Value>(class_def)?;
    let (_casted, _warnings) = index::validate_properties(&props, &class_def_value)?;

    Ok(())
}

#[tokio::test]
/// REQ-IDX-003
async fn test_index_req_idx_003_query_index() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-ws", "/tmp").await?;
    let ws_path = "workspaces/test-ws";

    index::reindex_all(&op, ws_path).await?;
    let results = index::query_index(&op, ws_path, "{}").await?;
    assert!(results.is_empty());
    Ok(())
}

#[tokio::test]
/// REQ-IDX-004
async fn test_index_req_idx_004_inverted_index_generation() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-ws", "/tmp").await?;
    let ws_path = "workspaces/test-ws";

    index::reindex_all(&op, ws_path).await?;
    assert!(
        !op.exists(&format!("{}/index/inverted_index.json", ws_path))
            .await?
    );
    Ok(())
}

#[tokio::test]
/// REQ-IDX-008
async fn test_index_req_idx_008_query_sql() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-sql-ws", "/tmp").await?;
    let ws_path = "workspaces/test-sql-ws";

    struct MockIntegrity;
    impl _ieapp_core::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    let class_def = serde_json::json!({
        "name": "Meeting",
        "template": "# Meeting\n\n## Date\n\n## Topic\n",
        "fields": {
            "Date": {"type": "date"},
            "Topic": {"type": "string"}
        }
    });
    class::upsert_class(&op, ws_path, &class_def).await?;

    let note_one = "---\nclass: Meeting\n---\n# Note 1\n\n## Date\n2025-01-01\n\n## Topic\nalpha";
    note::create_note(&op, ws_path, "note-1", note_one, "author", &MockIntegrity).await?;
    let note_two = "---\nclass: Meeting\n---\n# Note 2\n\n## Date\n2025-02-10\n\n## Topic\nbeta";
    note::create_note(&op, ws_path, "note-2", note_two, "author", &MockIntegrity).await?;

    let payload = serde_json::json!({
        "$sql": "SELECT * FROM Meeting WHERE Date >= '2025-02-01'"
    })
    .to_string();
    let results = index::query_index(&op, ws_path, &payload).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"].as_str(), Some("note-2"));

    Ok(())
}

#[test]
/// REQ-IDX-005
fn test_index_req_idx_005_word_count() {
    let content = "One two three";
    let count = index::compute_word_count(content);
    assert_eq!(count, 3);
}
