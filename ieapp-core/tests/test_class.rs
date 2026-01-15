mod common;
use _ieapp_core::class;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
/// REQ-CLS-002
async fn test_class_req_cls_002_upsert_and_list_classes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace", "/tmp").await?;
    let ws_path = "workspaces/test-workspace";

    let class_def = r#"{
        "name": "meeting",
        "description": "Meeting notes",
        "fields": [
            {"name": "date", "type": "date"},
            {"name": "summary", "type": "markdown"}
        ]
    }"#;

    let class_value: serde_json::Value = serde_json::from_str(class_def)?;
    class::upsert_class(&op, ws_path, &class_value).await?;

    let classes = class::list_classes(&op, ws_path).await?;
    assert!(classes
        .iter()
        .any(|c| c.get("name").and_then(|v| v.as_str()) == Some("meeting")));

    Ok(())
}

#[tokio::test]
/// REQ-CLS-001
async fn test_class_req_cls_001_list_column_types() -> anyhow::Result<()> {
    let types = class::list_column_types().await?;
    assert!(types.contains(&"string".to_string()));
    assert!(types.contains(&"markdown".to_string()));
    assert!(types.contains(&"number".to_string()));
    assert!(types.contains(&"date".to_string()));
    assert!(types.contains(&"list".to_string()));
    Ok(())
}
