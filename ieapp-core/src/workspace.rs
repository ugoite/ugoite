use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use opendal::Operator;
use pyo3::prelude::*;
use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
struct GlobalConfig {
    #[serde(default)]
    workspaces: Vec<String>,
    #[serde(default)]
    hmac_key_id: String,
    #[serde(default)]
    hmac_key: String,
    #[serde(default)]
    last_rotation: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WorkspaceMeta {
    pub id: String,
    pub name: String,
    pub created_at: f64, // Python uses time.time() which is float seconds, not ISO string
    pub storage: StorageConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: String,
    pub root: String,
}

#[pyfunction]
pub fn test_storage_connection() -> PyResult<bool> {
    Ok(true)
}

pub async fn workspace_exists(op: &Operator, name: &str) -> Result<bool> {
    let ws_path = format!("workspaces/{}/meta.json", name);
    Ok(op.exists(&ws_path).await?)
}

// Ensure global.json exists with HMAC keys
async fn ensure_global_json(op: &Operator) -> Result<()> {
    let global_path = "global.json";
    if op.exists(global_path).await? {
        return Ok(());
    }

    let now_iso = Utc::now().to_rfc3339();
    let key_id = format!("key-{}", uuid::Uuid::new_v4().simple());

    let mut key_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut key_bytes);
    let hmac_key = general_purpose::STANDARD.encode(key_bytes);

    let config = GlobalConfig {
        workspaces: Vec::new(),
        hmac_key_id: key_id,
        hmac_key,
        last_rotation: now_iso,
    };

    let json_bytes = serde_json::to_vec_pretty(&config)?;
    op.write(global_path, json_bytes).await?;
    Ok(())
}

pub async fn create_workspace(op: &Operator, name: &str, root_path: &str) -> Result<()> {
    if workspace_exists(op, name).await? {
        return Err(anyhow!("Workspace already exists: {}", name));
    }

    let ws_path = format!("workspaces/{}", name);

    // Ensure the workspace root directory exists
    op.create_dir(&format!("{}/", ws_path)).await?;

    // 1. Create directory structure
    for dir in &["classes", "index", "attachments", "notes"] {
        op.create_dir(&format!("{}/{}/", ws_path, dir)).await?;
    }

    // 2. Create meta.json
    let meta = WorkspaceMeta {
        id: name.to_string(),
        name: name.to_string(),
        created_at: Utc::now().timestamp() as f64,
        storage: StorageConfig {
            storage_type: "local".to_string(),
            root: root_path.to_string(),
        },
    };
    let meta_json = serde_json::to_vec_pretty(&meta)?;
    op.write(&format!("{}/meta.json", ws_path), meta_json)
        .await?;

    // 3. Create settings.json
    let settings = serde_json::json!({
        "default_class": "Note"
    });
    op.write(
        &format!("{}/settings.json", ws_path),
        serde_json::to_vec_pretty(&settings)?,
    )
    .await?;

    // 4. Create index files
    let index_data = serde_json::json!({
        "notes": {},
        "class_stats": {}
    });
    op.write(
        &format!("{}/index/index.json", ws_path),
        serde_json::to_vec_pretty(&index_data)?,
    )
    .await?;

    let stats_data = serde_json::json!({
        "last_indexed": 0.0,
        "note_count": 0,
        "tag_counts": {}
    });
    op.write(
        &format!("{}/index/stats.json", ws_path),
        serde_json::to_vec_pretty(&stats_data)?,
    )
    .await?;

    // 5. Update global.json
    ensure_global_json(op).await?;
    update_global_json(op, name).await?;

    Ok(())
}

async fn update_global_json(op: &Operator, ws_id: &str) -> Result<()> {
    let global_path = "global.json";

    let mut config: GlobalConfig = if op.exists(global_path).await? {
        let bytes = op.read(global_path).await?;
        serde_json::from_slice(&bytes.to_vec()).unwrap_or(GlobalConfig {
            workspaces: Vec::new(),
            hmac_key_id: String::new(),
            hmac_key: String::new(),
            last_rotation: String::new(),
        })
    } else {
        // Should have been created by ensure_global_json
        return Err(anyhow!("global.json missing"));
    };

    if !config.workspaces.contains(&ws_id.to_string()) {
        config.workspaces.push(ws_id.to_string());
        let json_bytes = serde_json::to_vec_pretty(&config)?;
        op.write(global_path, json_bytes).await?;
    }

    Ok(())
}

pub async fn list_workspaces(op: &Operator) -> Result<Vec<String>> {
    let global_path = "global.json";
    if !op.exists(global_path).await? {
        return Ok(vec![]);
    }

    let bytes = op.read(global_path).await?;
    let config: GlobalConfig = serde_json::from_slice(&bytes.to_vec())?;

    Ok(config.workspaces)
}

pub async fn get_workspace(op: &Operator, name: &str) -> Result<WorkspaceMeta> {
    if !workspace_exists(op, name).await? {
        return Err(anyhow!("Workspace not found: {}", name));
    }
    let meta_path = format!("workspaces/{}/meta.json", name);
    let bytes = op.read(&meta_path).await?;
    let meta: WorkspaceMeta = serde_json::from_slice(&bytes.to_vec())?;
    Ok(meta)
}
