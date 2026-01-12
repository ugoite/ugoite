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

    class::upsert_class(&op, ws_path, class_def).await?;

    let classes = class::list_classes(&op, ws_path).await?;
    assert!(classes.contains(&"meeting".to_string()));

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
    assert!(types.contains(&"boolean".to_string()));
    Ok(())
}
