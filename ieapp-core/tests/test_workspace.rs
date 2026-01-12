mod common;
use _ieapp_core::workspace;
use common::setup_operator;
use serde_json::Value;

#[tokio::test]
/// REQ-STO-002, REQ-STO-004
async fn test_workspace_req_sto_002_create_workspace_scaffolding() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-workspace";

    // Call create_workspace
    workspace::create_workspace(&op, ws_id).await?;

    // Verify directory structure using exists()
    // OpenDAL's exists() returns bool.
    let ws_path = format!("workspaces/{}", ws_id);
    assert!(op.exists(&format!("{}/", ws_path)).await?);

    // Check meta.json
    let meta_path = format!("{}/meta.json", ws_path);
    assert!(op.exists(&meta_path).await?);

    // Check other files/folders
    assert!(op.exists(&format!("{}/settings.json", ws_path)).await?);
    assert!(op.exists(&format!("{}/classes/", ws_path)).await?);
    assert!(op.exists(&format!("{}/index/", ws_path)).await?);
    assert!(op.exists(&format!("{}/attachments/", ws_path)).await?);
    assert!(op.exists(&format!("{}/notes/", ws_path)).await?);
    assert!(op.exists(&format!("{}/index/index.json", ws_path)).await?);
    assert!(op.exists(&format!("{}/index/stats.json", ws_path)).await?);

    // Verify global.json exists at root
    assert!(op.exists("global.json").await?);

    // Verify content of global.json
    // read() returns bytes (Buffer)
    let global_bytes = op.read("global.json").await?.to_vec();
    let global_json: Value = serde_json::from_slice(&global_bytes)?;

    // assert workspace is in global_json["workspaces"]
    let workspaces = global_json.get("workspaces").and_then(|v| v.as_object());
    assert!(workspaces.is_some());
    assert!(workspaces.unwrap().contains_key(ws_id));

    // Verify meta.json content
    let meta_bytes = op.read(&meta_path).await?.to_vec();
    let meta: Value = serde_json::from_slice(&meta_bytes)?;
    assert_eq!(meta["id"], ws_id);
    assert_eq!(meta["name"], ws_id);
    assert!(meta.get("created_at").is_some());
    assert!(meta.get("storage").is_some());

    Ok(())
}

#[tokio::test]
/// REQ-STO-005
async fn test_workspace_req_sto_005_create_workspace_idempotency() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-workspace";

    workspace::create_workspace(&op, ws_id).await?;

    // Should fail (result err) when creating again
    let result = workspace::create_workspace(&op, ws_id).await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
/// REQ-STO-004
async fn test_workspace_req_sto_004_list_workspaces_from_global_json() -> anyhow::Result<()> {
    let op = setup_operator()?;

    workspace::create_workspace(&op, "ws-a").await?;
    workspace::create_workspace(&op, "ws-b").await?;

    let mut listed = workspace::list_workspaces(&op).await?;
    listed.sort();
    assert_eq!(listed, vec!["ws-a".to_string(), "ws-b".to_string()]);

    Ok(())
}
