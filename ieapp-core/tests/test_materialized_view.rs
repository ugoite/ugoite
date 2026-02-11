mod common;

use _ieapp_core::materialized_view;
use _ieapp_core::space;
use common::setup_operator;
use serde_json::json;

#[tokio::test]
/// REQ-API-008
async fn test_materialized_view_req_api_008_metadata_lifecycle() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "view-space", "/tmp").await?;
    let ws_path = "spaces/view-space";

    let sql_id = "sql-view";
    let sql = "SELECT * FROM entries";

    let meta = materialized_view::create_or_update_view(&op, ws_path, sql_id, sql).await?;
    assert_eq!(meta.get("sql_id"), Some(&json!(sql_id)));
    assert_eq!(meta.get("sql"), Some(&json!(sql)));
    assert!(meta.get("snapshot_id").and_then(|v| v.as_u64()).is_some());

    let fetched = materialized_view::read_view_meta(&op, ws_path, sql_id).await?;
    assert_eq!(fetched.get("created_at"), meta.get("created_at"));

    materialized_view::delete_view(&op, ws_path, sql_id).await?;
    let meta_path = format!("{}/materialized_views/{}/meta.json", ws_path, sql_id);
    assert!(!op.exists(&meta_path).await?);

    Ok(())
}
