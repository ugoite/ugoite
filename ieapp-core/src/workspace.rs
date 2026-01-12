use anyhow::{anyhow, Result};
use chrono::Utc;
use opendal::Operator;
use pyo3::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug)]
struct GlobalConfig {
    workspaces: std::collections::HashMap<String, WorkspaceConfig>,
}

#[derive(Serialize, Deserialize, Debug)]
struct WorkspaceConfig {
    path: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct WorkspaceMeta {
    id: String,
    name: String,
    created_at: String,
    storage: String,
}

#[pyfunction]
pub fn test_storage_connection() -> PyResult<bool> {
    Ok(true)
}

pub async fn workspace_exists(op: &Operator, name: &str) -> Result<bool> {
    // Check if the workspace registration exists in global.json
    let global_path = "global.json";
    if !op.exists(global_path).await? {
        return Ok(false);
    }

    let bytes = op.read(global_path).await?;
    // Handle empty global.json or corrupt one gracefully?
    let config: GlobalConfig = match serde_json::from_slice(&bytes.to_vec()) {
        Ok(c) => c,
        Err(_) => return Ok(false),
    };

    Ok(config.workspaces.contains_key(name))
}

pub async fn create_workspace(op: &Operator, name: &str) -> Result<()> {
    if workspace_exists(op, name).await? {
        return Err(anyhow!("Workspace already exists: {}", name));
    }

    let ws_path = format!("workspaces/{}", name);

    // Ensure the workspace root directory exists
    op.create_dir(&format!("{}/", ws_path)).await?;

    // 1. Create directory structure (by creating placeholder files or just reliance on meta.json)
    // Directories: classes, index, attachments, notes
    op.create_dir(&format!("{}/classes/", ws_path)).await?;
    op.create_dir(&format!("{}/index/", ws_path)).await?;
    op.create_dir(&format!("{}/attachments/", ws_path)).await?;
    op.create_dir(&format!("{}/notes/", ws_path)).await?;

    // 2. Create meta.json
    let meta = WorkspaceMeta {
        id: name.to_string(),
        name: name.to_string(),
        created_at: Utc::now().to_rfc3339(),
        storage: "local".to_string(), // TODO: determine storage type dynamically
    };
    let meta_json = serde_json::to_vec_pretty(&meta)?;
    op.write(&format!("{}/meta.json", ws_path), meta_json)
        .await?;

    // 3. Create settings.json (empty object for now)
    op.write(&format!("{}/settings.json", ws_path), b"{}" as &[u8])
        .await?;

    // 4. Create index files
    op.write(&format!("{}/index/index.json", ws_path), b"{}" as &[u8])
        .await?;
    op.write(&format!("{}/index/stats.json", ws_path), b"{}" as &[u8])
        .await?;

    // 5. Update global.json
    update_global_json(op, name, &ws_path).await?;

    Ok(())
}

async fn update_global_json(op: &Operator, ws_id: &str, ws_path: &str) -> Result<()> {
    let global_path = "global.json";

    let mut config: GlobalConfig = if op.exists(global_path).await? {
        let bytes = op.read(global_path).await?;
        serde_json::from_slice(&bytes.to_vec()).unwrap_or(GlobalConfig {
            workspaces: std::collections::HashMap::new(),
        })
    } else {
        GlobalConfig {
            workspaces: std::collections::HashMap::new(),
        }
    };

    config.workspaces.insert(
        ws_id.to_string(),
        WorkspaceConfig {
            path: ws_path.to_string(),
        },
    );

    let json_bytes = serde_json::to_vec_pretty(&config)?;
    op.write(global_path, json_bytes).await?;

    Ok(())
}

pub async fn list_workspaces(op: &Operator) -> Result<Vec<String>> {
    let global_path = "global.json";
    if !op.exists(global_path).await? {
        return Ok(vec![]);
    }

    let bytes = op.read(global_path).await?;
    let config: GlobalConfig = serde_json::from_slice(&bytes.to_vec())?;

    Ok(config.workspaces.keys().cloned().collect())
}

pub async fn get_workspace(op: &Operator, name: &str) -> Result<()> {
    if !workspace_exists(op, name).await? {
        return Err(anyhow!("Workspace not found: {}", name));
    }
    Ok(())
}
