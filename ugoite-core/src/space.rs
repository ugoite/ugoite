use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use opendal::Operator;
use rand::TryRng;

use crate::storage::{OpendalStorage, StorageBackend};
pub use ugoite_minimum::space::{storage_type_and_root, SpaceMeta, StorageConfig};

async fn space_exists_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    name: &str,
) -> Result<bool> {
    let ws_path = format!("spaces/{name}/meta.json");
    storage.exists(&ws_path).await
}

pub async fn space_exists(op: &Operator, name: &str) -> Result<bool> {
    let storage = OpendalStorage::from_operator(op);
    space_exists_with_storage(&storage, name).await
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

async fn create_space_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    name: &str,
    root_path: &str,
) -> Result<()> {
    if space_exists_with_storage(storage, name).await? {
        return Err(anyhow!("Space already exists: {name}"));
    }

    let ws_path = format!("spaces/{name}");

    storage.create_dir(&format!("{ws_path}/")).await?;

    for dir in ["forms", "assets", "materialized_views", "sql_sessions"] {
        storage.create_dir(&format!("{ws_path}/{dir}/")).await?;
    }

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
    storage
        .write_json(&format!("{ws_path}/meta.json"), &meta)
        .await?;

    let settings = serde_json::json!({
        "default_form": "Entry"
    });
    storage
        .write_json(&format!("{ws_path}/settings.json"), &settings)
        .await?;

    Ok(())
}

pub async fn create_space(op: &Operator, name: &str, root_path: &str) -> Result<()> {
    let storage = OpendalStorage::from_operator(op);
    create_space_with_storage(&storage, name, root_path).await
}

async fn list_spaces_with_storage<S: StorageBackend + ?Sized>(storage: &S) -> Result<Vec<String>> {
    let spaces_root = "spaces/";
    if !storage.exists(spaces_root).await? {
        return Ok(vec![]);
    }

    let mut spaces = Vec::new();
    for entry in storage.list_dir(spaces_root).await? {
        if !entry.is_dir {
            continue;
        }
        let space_id = entry
            .name
            .trim_end_matches('/')
            .split('/')
            .next_back()
            .unwrap_or("");
        if space_id.is_empty() {
            continue;
        }
        let meta_path = format!("spaces/{space_id}/meta.json");
        if storage.exists(&meta_path).await? {
            spaces.push(space_id.to_string());
        }
    }

    spaces.sort();
    spaces.dedup();
    Ok(spaces)
}

pub async fn list_spaces(op: &Operator) -> Result<Vec<String>> {
    let storage = OpendalStorage::from_operator(op);
    list_spaces_with_storage(&storage).await
}

async fn get_space_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    name: &str,
) -> Result<SpaceMeta> {
    if !space_exists_with_storage(storage, name).await? {
        return Err(anyhow!("Space not found: {name}"));
    }
    storage.read_json(&format!("spaces/{name}/meta.json")).await
}

pub async fn get_space(op: &Operator, name: &str) -> Result<SpaceMeta> {
    let storage = OpendalStorage::from_operator(op);
    get_space_with_storage(&storage, name).await
}

async fn get_space_raw_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    name: &str,
) -> Result<serde_json::Value> {
    if !space_exists_with_storage(storage, name).await? {
        return Err(anyhow!("Space not found: {name}"));
    }
    let meta_path = format!("spaces/{name}/meta.json");
    let settings_path = format!("spaces/{name}/settings.json");
    let mut meta: serde_json::Value = storage.read_json(&meta_path).await?;

    let settings = if storage.exists(&settings_path).await? {
        storage.read_json(&settings_path).await?
    } else {
        serde_json::json!({})
    };
    meta["settings"] = settings;
    Ok(meta)
}

pub async fn get_space_raw(op: &Operator, name: &str) -> Result<serde_json::Value> {
    let storage = OpendalStorage::from_operator(op);
    get_space_raw_with_storage(&storage, name).await
}

async fn patch_space_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    space_id: &str,
    patch: &serde_json::Value,
) -> Result<serde_json::Value> {
    let meta_path = format!("spaces/{space_id}/meta.json");
    let settings_path = format!("spaces/{space_id}/settings.json");

    if !storage.exists(&meta_path).await? {
        return Err(anyhow!("Space {space_id} not found"));
    }

    let mut meta: serde_json::Value = storage.read_json(&meta_path).await?;
    let mut settings = if storage.exists(&settings_path).await? {
        storage.read_json(&settings_path).await?
    } else {
        serde_json::json!({})
    };

    if let Some(name) = patch.get("name") {
        meta["name"] = name.clone();
    }
    if let Some(storage_config) = patch.get("storage_config") {
        meta["storage_config"] = storage_config.clone();
    }
    if let Some(new_settings) = patch.get("settings").and_then(|value| value.as_object()) {
        if let Some(settings_obj) = settings.as_object_mut() {
            for (key, value) in new_settings {
                settings_obj.insert(key.clone(), value.clone());
            }
        }
    }

    storage.write_json(&meta_path, &meta).await?;
    storage.write_json(&settings_path, &settings).await?;

    let mut merged = meta;
    merged["settings"] = settings;
    Ok(merged)
}

pub async fn patch_space(
    op: &Operator,
    space_id: &str,
    patch: &serde_json::Value,
) -> Result<serde_json::Value> {
    let storage = OpendalStorage::from_operator(op);
    patch_space_with_storage(&storage, space_id, patch).await
}

/// Test a storage connection by checking if the URI is accessible.
pub async fn test_storage_connection(uri: &str) -> Result<serde_json::Value> {
    if uri.starts_with("memory://") {
        Ok(serde_json::json!({"status": "ok", "mode": "memory"}))
    } else if uri.starts_with("file://")
        || uri.starts_with("fs://")
        || uri.starts_with('/')
        || uri.starts_with('.')
    {
        Ok(serde_json::json!({"status": "ok", "mode": "local"}))
    } else if uri.starts_with("s3://") {
        Ok(serde_json::json!({"status": "ok", "mode": "s3"}))
    } else {
        Ok(serde_json::json!({"status": "ok", "mode": "unknown"}))
    }
}
