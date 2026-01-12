mod common;
use _ieapp_core::class;
use _ieapp_core::workspace;
use common::setup_operator;

#[tokio::test]
async fn test_upsert_and_list_classes() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
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
async fn test_list_column_types() -> anyhow::Result<()> {
    let types = class::list_column_types().await?;
    assert!(types.contains(&"markdown".to_string()));
    assert!(types.contains(&"number".to_string()));
    assert!(types.contains(&"date".to_string()));
    assert!(types.contains(&"boolean".to_string()));
    Ok(())
}

#[tokio::test]
async fn test_migrate_class_stub() -> anyhow::Result<()> {
    let op = setup_operator()?;
    workspace::create_workspace(&op, "test-workspace").await?;
    let ws_path = "workspaces/test-workspace";

    // Setup not really needed for stub but consistent style
    let new_class = r#"{"name": "test", "fields": []}"#;
    let count = class::migrate_class(&op, ws_path, new_class, None).await?;

    // Stub returns 0
    assert_eq!(count, 0);

    Ok(())
}
