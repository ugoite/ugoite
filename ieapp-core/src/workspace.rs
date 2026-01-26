use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use opendal::Operator;
use pyo3::prelude::*;
use rand::TryRngCore;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug)]
struct GlobalConfig {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    default_storage: String,
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

fn storage_type_and_root(root_uri: &str) -> (String, String, String) {
    if let Ok(url) = Url::parse(root_uri) {
        let scheme = url.scheme().to_string();
        let root = if scheme == "fs" || scheme == "file" {
            url.path().to_string()
        } else {
            url.path().trim_start_matches('/').to_string()
        };
        let storage_type = if scheme == "fs" || scheme == "file" {
            "local".to_string()
        } else {
            scheme.clone()
        };
        return (storage_type, root, scheme);
    }

    (
        "local".to_string(),
        root_uri.to_string(),
        "file".to_string(),
    )
}

fn default_global_config(default_storage: &str) -> GlobalConfig {
    let now_iso = Utc::now().to_rfc3339();
    let key_id = format!("key-{}", uuid::Uuid::new_v4().simple());

    let mut key_bytes = [0u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut key_bytes)
        .expect("Failed to generate secure random bytes");
    let hmac_key = general_purpose::STANDARD.encode(key_bytes);

    GlobalConfig {
        version: 1,
        default_storage: default_storage.to_string(),
        workspaces: Vec::new(),
        hmac_key_id: key_id,
        hmac_key,
        last_rotation: now_iso,
    }
}

// Ensure global.json exists with HMAC keys
async fn ensure_global_json(op: &Operator, root_uri: &str) -> Result<()> {
    let global_path = "global.json";
    if op.exists(global_path).await? {
        return Ok(());
    }

    let config = default_global_config(root_uri);
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
    for dir in &["classes", "attachments"] {
        op.create_dir(&format!("{}/{}/", ws_path, dir)).await?;
    }

    // 2. Create meta.json
    let (storage_type, storage_root, scheme) = storage_type_and_root(root_path);
    let created_at = Utc::now().timestamp_millis() as f64 / 1000.0;

    let meta = WorkspaceMeta {
        id: name.to_string(),
        name: name.to_string(),
        created_at,
        storage: StorageConfig {
            storage_type,
            root: storage_root.clone(),
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

    // 4. Update global.json
    let default_storage = if scheme == "file" || scheme == "fs" {
        format!("fs://{}", storage_root)
    } else {
        root_path.to_string()
    };
    ensure_global_json(op, &default_storage).await?;
    update_global_json(op, name).await?;

    Ok(())
}

async fn update_global_json(op: &Operator, ws_id: &str) -> Result<()> {
    let global_path = "global.json";

    let mut config: GlobalConfig = if op.exists(global_path).await? {
        let bytes = op.read(global_path).await?;
        serde_json::from_slice(&bytes.to_vec()).unwrap_or_else(|_| default_global_config(""))
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

async fn read_json(op: &Operator, path: &str) -> Result<serde_json::Value> {
    let bytes = op.read(path).await?;
    Ok(serde_json::from_slice(&bytes.to_vec())?)
}

async fn write_json(op: &Operator, path: &str, value: &serde_json::Value) -> Result<()> {
    op.write(path, serde_json::to_vec_pretty(value)?).await?;
    Ok(())
}

pub async fn get_workspace_raw(op: &Operator, name: &str) -> Result<serde_json::Value> {
    if !workspace_exists(op, name).await? {
        return Err(anyhow!("Workspace not found: {}", name));
    }
    let meta_path = format!("workspaces/{}/meta.json", name);
    read_json(op, &meta_path).await
}

pub async fn patch_workspace(
    op: &Operator,
    workspace_id: &str,
    patch: &serde_json::Value,
) -> Result<serde_json::Value> {
    let meta_path = format!("workspaces/{}/meta.json", workspace_id);
    let settings_path = format!("workspaces/{}/settings.json", workspace_id);

    if !op.exists(&meta_path).await? {
        return Err(anyhow!("Workspace {} not found", workspace_id));
    }

    let mut meta = read_json(op, &meta_path).await?;
    let mut settings = if op.exists(&settings_path).await? {
        read_json(op, &settings_path).await?
    } else {
        serde_json::json!({})
    };

    if let Some(name) = patch.get("name") {
        meta["name"] = name.clone();
    }
    if let Some(storage_config) = patch.get("storage_config") {
        meta["storage_config"] = storage_config.clone();
    }
    if let Some(new_settings) = patch.get("settings").and_then(|v| v.as_object()) {
        if let Some(settings_obj) = settings.as_object_mut() {
            for (k, v) in new_settings {
                settings_obj.insert(k.clone(), v.clone());
            }
        }
    }

    write_json(op, &meta_path, &meta).await?;
    write_json(op, &settings_path, &settings).await?;

    let mut merged = meta;
    merged["settings"] = settings;
    Ok(merged)
}
