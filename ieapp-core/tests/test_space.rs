mod common;
use _ieapp_core::space;
use common::setup_operator;
use serde_json::Value;

#[tokio::test]
/// REQ-STO-002, REQ-STO-004
async fn test_space_req_sto_002_create_space_scaffolding() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-space";

    // Call create_space
    space::create_space(&op, ws_id, "/tmp/ieapp").await?;

    // Verify directory structure using exists()
    // OpenDAL's exists() returns bool.
    let ws_path = format!("spaces/{}", ws_id);
    assert!(op.exists(&format!("{}/", ws_path)).await?);

    // Check meta.json
    let meta_path = format!("{}/meta.json", ws_path);
    assert!(op.exists(&meta_path).await?);

    // Check other files/folders
    assert!(op.exists(&format!("{}/settings.json", ws_path)).await?);
    assert!(op.exists(&format!("{}/forms/", ws_path)).await?);
    assert!(op.exists(&format!("{}/assets/", ws_path)).await?);

    // Verify global.json exists at root
    assert!(op.exists("global.json").await?);

    // Verify content of global.json
    // read() returns bytes (Buffer)
    let global_bytes = op.read("global.json").await?.to_vec();
    let global_json: Value = serde_json::from_slice(&global_bytes)?;

    // assert space is in global_json["spaces"]
    let spaces = global_json.get("spaces").and_then(|v| v.as_array());
    assert!(spaces.is_some());
    let exists = spaces.unwrap().iter().any(|v| v.as_str() == Some(ws_id));
    assert!(exists);

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
async fn test_space_req_sto_005_create_space_idempotency() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-space";

    space::create_space(&op, ws_id, "/tmp").await?;

    // Should fail (result err) when creating again
    let result = space::create_space(&op, ws_id, "/tmp").await;
    assert!(result.is_err());

    Ok(())
}

#[tokio::test]
/// REQ-STO-004
async fn test_space_req_sto_004_list_spaces_from_global_json() -> anyhow::Result<()> {
    let op = setup_operator()?;

    space::create_space(&op, "sp-a", "/tmp").await?;
    space::create_space(&op, "sp-b", "/tmp").await?;

    let mut listed = space::list_spaces(&op).await?;
    listed.sort();
    assert_eq!(listed, vec!["sp-a".to_string(), "sp-b".to_string()]);

    Ok(())
}

#[tokio::test]
/// REQ-STO-008
async fn test_space_req_sto_008_list_spaces_recovers_from_corrupt_global() -> anyhow::Result<()> {
    let op = setup_operator()?;

    op.write("global.json", b"{invalid json".to_vec()).await?;

    let listed = space::list_spaces(&op).await?;
    assert!(listed.is_empty());

    let global_bytes = op.read("global.json").await?.to_vec();
    let global_json: Value = serde_json::from_slice(&global_bytes)?;
    assert!(global_json.get("spaces").is_some());

    Ok(())
}
