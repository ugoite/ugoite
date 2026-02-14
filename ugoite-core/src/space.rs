use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use pyo3::prelude::*;
use rand::TryRng;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Serialize, Deserialize, Debug)]
pub struct SpaceMeta {
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

pub async fn space_exists(op: &Operator, name: &str) -> Result<bool> {
    let ws_path = format!("spaces/{}/meta.json", name);
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

fn generate_hmac_material() -> (String, String, String) {
    let now_iso = Utc::now().to_rfc3339();
    let key_id = format!("key-{}", uuid::Uuid::new_v4().simple());

    let mut key_bytes = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut key_bytes)
        .expect("Failed to generate secure random bytes");
    let hmac_key = general_purpose::STANDARD.encode(key_bytes);

    (key_id, hmac_key, now_iso)
}

pub async fn create_space(op: &Operator, name: &str, root_path: &str) -> Result<()> {
    if space_exists(op, name).await? {
        return Err(anyhow!("Space already exists: {}", name));
    }

    let ws_path = format!("spaces/{}", name);

    // Ensure the space root directory exists
    op.create_dir(&format!("{}/", ws_path)).await?;

    // 1. Create directory structure
    for dir in &["forms", "assets", "materialized_views", "sql_sessions"] {
        op.create_dir(&format!("{}/{}/", ws_path, dir)).await?;
    }

    // 2. Create meta.json
    let (storage_type, storage_root, _scheme) = storage_type_and_root(root_path);
    let created_at = Utc::now().timestamp_millis() as f64 / 1000.0;
    let (hmac_key_id, hmac_key, last_rotation) = generate_hmac_material();

    let meta = serde_json::json!({
        "id": name,
        "name": name,
        "created_at": created_at,
        "storage": {
            "type": storage_type,
            "root": storage_root,
        },
        "hmac_key_id": hmac_key_id,
        "hmac_key": hmac_key,
        "last_rotation": last_rotation,
    });
    let meta_json = serde_json::to_vec_pretty(&meta)?;
    op.write(&format!("{}/meta.json", ws_path), meta_json)
        .await?;

    // 3. Create settings.json
    let settings = serde_json::json!({
        "default_form": "Entry"
    });
    op.write(
        &format!("{}/settings.json", ws_path),
        serde_json::to_vec_pretty(&settings)?,
    )
    .await?;

    Ok(())
}

pub async fn list_spaces(op: &Operator) -> Result<Vec<String>> {
    let spaces_root = "spaces/";
    if !op.exists(spaces_root).await? {
        return Ok(vec![]);
    }

    let mut spaces = Vec::new();
    let mut lister = op.lister(spaces_root).await?;
    while let Some(entry) = lister.try_next().await? {
        if entry.metadata().mode() != EntryMode::DIR {
            continue;
        }
        let space_id = entry
            .name()
            .trim_end_matches('/')
            .split('/')
            .next_back()
            .unwrap_or("");
        if space_id.is_empty() {
            continue;
        }
        let meta_path = format!("spaces/{}/meta.json", space_id);
        if op.exists(&meta_path).await? {
            spaces.push(space_id.to_string());
        }
    }

    spaces.sort();
    spaces.dedup();
    Ok(spaces)
}

pub async fn get_space(op: &Operator, name: &str) -> Result<SpaceMeta> {
    if !space_exists(op, name).await? {
        return Err(anyhow!("Space not found: {}", name));
    }
    let meta_path = format!("spaces/{}/meta.json", name);
    let bytes = op.read(&meta_path).await?;
    let meta: SpaceMeta = serde_json::from_slice(&bytes.to_vec())?;
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

pub async fn get_space_raw(op: &Operator, name: &str) -> Result<serde_json::Value> {
    if !space_exists(op, name).await? {
        return Err(anyhow!("Space not found: {}", name));
    }
    let meta_path = format!("spaces/{}/meta.json", name);
    read_json(op, &meta_path).await
}

pub async fn patch_space(
    op: &Operator,
    space_id: &str,
    patch: &serde_json::Value,
) -> Result<serde_json::Value> {
    let meta_path = format!("spaces/{}/meta.json", space_id);
    let settings_path = format!("spaces/{}/settings.json", space_id);

    if !op.exists(&meta_path).await? {
        return Err(anyhow!("Space {} not found", space_id));
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
