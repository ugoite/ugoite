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
    assert!(types.contains(&"double".to_string()));
    assert!(types.contains(&"float".to_string()));
    assert!(types.contains(&"integer".to_string()));
    assert!(types.contains(&"long".to_string()));
    assert!(types.contains(&"boolean".to_string()));
    assert!(types.contains(&"date".to_string()));
    assert!(types.contains(&"time".to_string()));
    assert!(types.contains(&"timestamp".to_string()));
    assert!(types.contains(&"timestamp_tz".to_string()));
    assert!(types.contains(&"timestamp_ns".to_string()));
    assert!(types.contains(&"timestamp_tz_ns".to_string()));
    assert!(types.contains(&"uuid".to_string()));
    assert!(types.contains(&"binary".to_string()));
    assert!(types.contains(&"list".to_string()));
    Ok(())
}

#[tokio::test]
/// REQ-CLS-005
async fn test_class_req_cls_005_reject_reserved_metadata_columns() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-meta-cols", "/tmp").await?;
    let ws_path = "workspaces/test-meta-cols";

    let class_def = serde_json::json!({
        "name": "BadClass",
        "fields": {
            "title": {"type": "string"}
        }
    });

    let result = class::upsert_class(&op, ws_path, &class_def).await;
    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(message.contains("reserved"));

    Ok(())
}

#[tokio::test]
/// REQ-CLS-006
async fn test_class_req_cls_006_reject_reserved_metadata_class() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-meta-class", "/tmp").await?;
    let ws_path = "workspaces/test-meta-class";

    let class_def = serde_json::json!({
        "name": "SQL",
        "fields": {
            "sql": {"type": "string"}
        }
    });

    let result = class::upsert_class(&op, ws_path, &class_def).await;
    assert!(result.is_err());
    let message = result.unwrap_err().to_string();
    assert!(message.contains("reserved"));

    Ok(())
}
