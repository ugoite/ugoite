use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use futures::TryStreamExt;
use opendal::Operator;
use rand::TryRng;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use url::Url;

use crate::error::{AppError, ErrorCode};
use crate::form;
use crate::storage::operator_from_uri_with_endpoint;
use crate::storage::{OpendalStorage, StorageBackend};
use ugoite_domain::id::validate_space_id;
pub use ugoite_domain::space::{storage_type_and_root, SpaceMeta, StorageConfig};

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct StorageConnectionTestConfig {
    pub uri: String,
    #[serde(default)]
    pub endpoint: Option<String>,
}

async fn space_exists_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    name: &str,
) -> Result<bool> {
    validate_space_path_segment(name)?;
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

fn starter_entry_form_definition() -> serde_json::Value {
    serde_json::json!({
        "name": "Entry",
        "version": 1,
        "fields": {
            "Body": {"type": "markdown"}
        },
        "allow_extra_attributes": "allow_columns",
    })
}

#[cfg(unix)]
fn local_space_fs_path(op: &Operator, space_id: &str) -> Option<PathBuf> {
    let scheme = op.info().scheme();
    if scheme != "fs" && scheme != "file" {
        return None;
    }
    Some(
        Path::new(op.info().root().as_str())
            .join("spaces")
            .join(space_id),
    )
}

#[cfg(unix)]
fn set_owner_only_mode(path: &Path, mode: u32) -> Result<()> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    if !path.exists() {
        return Ok(());
    }
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(unix)]
fn apply_local_space_permissions(op: &Operator, space_id: &str) -> Result<()> {
    let Some(space_dir) = local_space_fs_path(op, space_id) else {
        return Ok(());
    };

    let Some(spaces_root) = space_dir.parent() else {
        return Ok(());
    };

    set_owner_only_mode(spaces_root, 0o700)?;
    set_owner_only_mode(&space_dir, 0o700)?;
    for dir in ["forms", "assets", "materialized_views", "sql_sessions"] {
        set_owner_only_mode(&space_dir.join(dir), 0o700)?;
    }
    for file in ["meta.json", "settings.json"] {
        set_owner_only_mode(&space_dir.join(file), 0o600)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_local_space_permissions(_op: &Operator, _space_id: &str) -> Result<()> {
    Ok(())
}

async fn create_space_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    name: &str,
    root_path: &str,
) -> Result<()> {
    validate_space_path_segment(name)?;
    if space_exists_with_storage(storage, name).await? {
        return Err(AppError::conflict(
            ErrorCode::SpaceAlreadyExists,
            format!("Space already exists: {name}"),
        )
        .into());
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
    create_space_with_storage(&storage, name, root_path).await?;
    let ws_path = format!("spaces/{name}");
    // Bootstrap a user-creatable starter form so first-entry authoring works immediately.
    form::upsert_form(op, &ws_path, &starter_entry_form_definition()).await?;

    // Local filesystem spaces are private by default: directories are owner-only,
    // and the metadata files created here are readable/writeable by the owner only.
    apply_local_space_permissions(op, name)?;

    Ok(())
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
        return Err(AppError::not_found(
            ErrorCode::SpaceNotFound,
            format!("Space not found: {name}"),
        )
        .into());
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
        return Err(AppError::not_found(
            ErrorCode::SpaceNotFound,
            format!("Space not found: {name}"),
        )
        .into());
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
    validate_space_path_segment(space_id)?;
    let meta_path = format!("spaces/{space_id}/meta.json");
    let settings_path = format!("spaces/{space_id}/settings.json");

    if !storage.exists(&meta_path).await? {
        return Err(AppError::not_found(
            ErrorCode::SpaceNotFound,
            format!("Space not found: {space_id}"),
        )
        .into());
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

/// Test a storage connection by checking if the proposed config is accessible.
pub async fn test_storage_connection(
    config: &StorageConnectionTestConfig,
) -> Result<serde_json::Value> {
    let trimmed = config.uri.trim();
    if trimmed.is_empty() {
        anyhow::bail!("storage URI is required");
    }
    let mode = storage_connection_mode(trimmed)?;
    let endpoint = validate_storage_endpoint(config.endpoint.as_deref())?;
    let operator = operator_from_uri_with_endpoint(trimmed, endpoint)?;
    let mut lister = operator.lister("").await.map_err(|error| {
        AppError::dependency_unavailable(
            ErrorCode::StorageConnectionFailed,
            format!("storage connection failed: {error}"),
        )
    })?;
    let _ = lister.try_next().await.map_err(|error| {
        AppError::dependency_unavailable(
            ErrorCode::StorageConnectionFailed,
            format!("storage connection failed: {error}"),
        )
    })?;
    Ok(serde_json::json!({"status": "ok", "mode": mode}))
}

fn validate_space_path_segment(name: &str) -> Result<()> {
    validate_space_id(name).map_err(|error| AppError::invalid_identifier(error.to_string()).into())
}

fn storage_connection_mode(uri: &str) -> Result<&'static str> {
    if uri.starts_with("memory://") {
        return Ok("memory");
    }
    if uri.starts_with("file://") || uri.starts_with("fs://") || uri.starts_with('/') {
        return Ok("local");
    }
    if uri.starts_with("s3://") {
        validate_remote_storage_uri(uri)?;
        return Ok("s3");
    }
    anyhow::bail!("unsupported storage URI: {uri}")
}

fn validate_storage_endpoint(endpoint: Option<&str>) -> Result<Option<&str>> {
    let Some(endpoint) = endpoint.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parsed = Url::parse(endpoint).map_err(|_| anyhow!("invalid storage endpoint"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("unsupported storage endpoint scheme: {}", parsed.scheme());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("storage endpoint host is required"))?;
    if is_blocked_storage_host(host) {
        anyhow::bail!("blocked storage endpoint host: {host}");
    }
    Ok(Some(endpoint))
}

fn validate_remote_storage_uri(uri: &str) -> Result<()> {
    let parsed = Url::parse(uri).map_err(|_| anyhow!("invalid storage URI"))?;
    if let Some(host) = parsed.host_str() {
        if is_blocked_storage_host(host) {
            anyhow::bail!("blocked storage endpoint host: {host}");
        }
    }
    Ok(())
}

fn is_blocked_storage_host(host: &str) -> bool {
    let normalized = host
        .trim_matches('[')
        .trim_matches(']')
        .to_ascii_lowercase();
    if matches!(normalized.as_str(), "localhost" | "0.0.0.0" | "::1") {
        return true;
    }
    if normalized.starts_with("127.")
        || normalized.starts_with("10.")
        || normalized.starts_with("192.168.")
        || normalized.starts_with("169.254.")
    {
        return true;
    }
    if let Some(second_octet) = normalized
        .strip_prefix("172.")
        .and_then(|rest| rest.split('.').next())
        .and_then(|value| value.parse::<u8>().ok())
    {
        return (16..=31).contains(&second_octet);
    }
    false
}
