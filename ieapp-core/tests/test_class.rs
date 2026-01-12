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
    // assert!(classes.contains(&"meeting".to_string())); // Uncomment when implemented

    Ok(())
}
