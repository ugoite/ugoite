use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use futures::TryStreamExt;
use opendal::Operator;
use rand::TryRng;
use std::collections::BTreeMap;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use url::Url;

use crate::form;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::id::validate_space_id;
pub use ugoite_domain::space::{storage_type_and_root, SpaceMeta, StorageConfig};
use ugoite_storage::{operator_from_uri_with_endpoint, OpendalStorage, StorageBackend};

pub(crate) const CURRENT_SPACE_SCHEMA_VERSION: u64 = 2;

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
    for dir in ["security", "forms", "assets", "sql_sessions"] {
        set_owner_only_mode(&space_dir.join(dir), 0o700)?;
    }
    for file in ["meta.json", "settings.json"] {
        set_owner_only_mode(&space_dir.join(file), 0o600)?;
    }
    Ok(())
}

#[cfg(unix)]
fn validate_local_space_permissions(op: &Operator, space_id: &str) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let Some(space_dir) = local_space_fs_path(op, space_id) else {
        return Ok(());
    };
    let Some(spaces_root) = space_dir.parent() else {
        return Ok(());
    };
    let expected = [
        (spaces_root, 0o700),
        (space_dir.as_path(), 0o700),
        (&space_dir.join("security"), 0o700),
        (&space_dir.join("forms"), 0o700),
        (&space_dir.join("assets"), 0o700),
        (&space_dir.join("sql_sessions"), 0o700),
        (&space_dir.join("meta.json"), 0o600),
        (&space_dir.join("settings.json"), 0o600),
    ];
    for (path, expected_mode) in expected {
        let actual_mode = std::fs::metadata(path)
            .with_context(|| format!("read local Space permission for {}", path.display()))?
            .permissions()
            .mode()
            & 0o777;
        if actual_mode != expected_mode {
            bail!(
                "incomplete Space bootstrap: {} has mode {actual_mode:o}, expected {expected_mode:o}",
                path.display()
            );
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_local_space_permissions(_op: &Operator, _space_id: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn apply_local_space_permissions(_op: &Operator, _space_id: &str) -> Result<()> {
    Ok(())
}

async fn create_space_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    directory_id: &str,
    space_uid: uuid::Uuid,
    slug: &str,
    root_path: &str,
) -> Result<()> {
    validate_space_path_segment(directory_id)?;
    validate_space_path_segment(slug)?;
    if space_exists_with_storage(storage, directory_id).await? {
        return Err(AppError::conflict(
            ErrorCode::SpaceAlreadyExists,
            format!("Space already exists: {directory_id}"),
        )
        .into());
    }

    let ws_path = format!("spaces/{directory_id}");

    storage.create_dir(&format!("{ws_path}/")).await?;

    for dir in ["security", "forms", "assets", "sql_sessions"] {
        storage.create_dir(&format!("{ws_path}/{dir}/")).await?;
    }

    let (storage_type, storage_root, _scheme) = storage_type_and_root(root_path);
    let created_at = Utc::now().timestamp_millis() as f64 / 1000.0;
    let (hmac_key_id, hmac_key, last_rotation) = generate_hmac_material();

    let meta = serde_json::json!({
        "schema_version": CURRENT_SPACE_SCHEMA_VERSION,
        "space_id": directory_id,
        "space_uid": space_uid,
        "slug": slug,
        "id": directory_id,
        "name": slug,
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
    create_space_with_storage(&storage, name, uuid::Uuid::now_v7(), name, root_path).await?;
    let ws_path = format!("spaces/{name}");
    // Bootstrap a user-creatable starter form so first-entry authoring works immediately.
    form::upsert_form(op, &ws_path, &starter_entry_form_definition()).await?;

    // Local filesystem spaces are private by default: directories are owner-only,
    // and the metadata files created here are readable/writeable by the owner only.
    apply_local_space_permissions(op, name)?;

    Ok(())
}

pub async fn create_space_with_identity(
    op: &Operator,
    space_id: uuid::Uuid,
    slug: &str,
    root_path: &str,
) -> Result<()> {
    let directory_id = space_id.to_string();
    let storage = OpendalStorage::from_operator(op);
    create_space_with_storage(&storage, &directory_id, space_id, slug, root_path).await?;
    let ws_path = format!("spaces/{directory_id}");
    form::upsert_form(op, &ws_path, &starter_entry_form_definition()).await?;
    apply_local_space_permissions(op, &directory_id)?;
    Ok(())
}

async fn list_spaces_with_storage<S: StorageBackend + ?Sized>(storage: &S) -> Result<Vec<String>> {
    let spaces_root = "spaces/";
    if !storage.exists(spaces_root).await? {
        return Ok(vec![]);
    }

    let mut spaces = Vec::new();
    let mut seen_uids = BTreeMap::<uuid::Uuid, String>::new();
    let mut seen_slugs = BTreeMap::<String, String>::new();
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
            validate_space_path_segment(space_id)?;
            let meta = ensure_space_identity(storage, space_id).await?;
            let space_uid = meta
                .get("space_uid")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("Space is missing immutable space_uid"))
                .and_then(|value| uuid::Uuid::parse_str(value).map_err(anyhow::Error::from))?;
            if let Some(previous_id) = seen_uids.insert(space_uid, space_id.to_string()) {
                bail!(
                    "duplicate immutable space_uid {space_uid} is used by Spaces {previous_id} and {space_id}"
                );
            }
            let slug = meta
                .get("slug")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow!("Space metadata has no slug"))?;
            if let Some(previous_id) = seen_slugs.insert(slug.to_string(), space_id.to_string()) {
                bail!("Space slug is not unique: {slug} ({previous_id}, {space_id})");
            }
            spaces.push(space_id.to_string());
        }
    }

    spaces.sort();
    spaces.dedup();
    Ok(spaces)
}

pub async fn list_spaces(op: &Operator) -> Result<Vec<String>> {
    let storage = OpendalStorage::from_operator(op);
    let spaces = list_spaces_with_storage(&storage).await?;
    // Directory listing is discovery only. Do not expose a metadata-only or
    // crash-left Space through a public enumeration result.
    for space_id in &spaces {
        validate_complete_bootstrap(op, space_id).await?;
    }
    Ok(spaces)
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
    let meta = ensure_space_identity(storage, name).await?;
    serde_json::from_value(meta).map_err(Into::into)
}

pub async fn get_space(op: &Operator, name: &str) -> Result<SpaceMeta> {
    validate_complete_bootstrap(op, name).await?;
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
    let settings_path = format!("spaces/{name}/settings.json");
    let mut meta = ensure_space_identity(storage, name).await?;

    if !storage.exists(&settings_path).await? {
        return Err(anyhow!("unsupported Space layout: missing settings.json"));
    }
    let settings: serde_json::Value = storage.read_json(&settings_path).await?;
    if !settings.is_object()
        || settings
            .get("default_form")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(anyhow!(
            "unsupported Space layout: settings.json requires default_form"
        ));
    }
    meta["settings"] = settings;
    Ok(meta)
}

async fn ensure_space_identity<S: StorageBackend + ?Sized>(
    storage: &S,
    name: &str,
) -> Result<serde_json::Value> {
    let meta_path = format!("spaces/{name}/meta.json");
    let meta: serde_json::Value = storage.read_json(&meta_path).await?;
    validate_current_space_metadata(name, &meta)?;
    Ok(meta)
}

pub(crate) fn validate_current_space_metadata(
    expected_directory_id: &str,
    meta: &serde_json::Value,
) -> Result<uuid::Uuid> {
    #[derive(serde::Deserialize)]
    struct CurrentSpaceMetadata {
        schema_version: u64,
        space_id: String,
        space_uid: uuid::Uuid,
        slug: String,
        id: String,
        name: String,
        created_at: f64,
        storage: StorageConfig,
        hmac_key_id: String,
        hmac_key: String,
        last_rotation: String,
    }

    let metadata: CurrentSpaceMetadata = serde_json::from_value(meta.clone()).map_err(|error| {
        anyhow!("unsupported Space layout: incomplete or invalid metadata: {error}")
    })?;
    if metadata.schema_version != CURRENT_SPACE_SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported Space layout: metadata schema_version must be 2"
        ));
    }
    if metadata.space_id != expected_directory_id {
        return Err(anyhow!(
            "unsupported Space layout: metadata space_id does not match its directory"
        ));
    }
    if metadata.id != metadata.space_id {
        return Err(anyhow!(
            "unsupported Space layout: metadata id does not match space_id"
        ));
    }
    for (field, value) in [
        ("space_id", metadata.space_id.as_str()),
        ("slug", metadata.slug.as_str()),
        ("id", metadata.id.as_str()),
        ("name", metadata.name.as_str()),
        ("storage.type", metadata.storage.storage_type.as_str()),
        ("hmac_key_id", metadata.hmac_key_id.as_str()),
        ("hmac_key", metadata.hmac_key.as_str()),
        ("last_rotation", metadata.last_rotation.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(anyhow!(
                "unsupported Space layout: required metadata field {field} is empty"
            ));
        }
    }
    if !metadata.created_at.is_finite()
        || chrono::DateTime::parse_from_rfc3339(&metadata.last_rotation).is_err()
    {
        return Err(anyhow!(
            "unsupported Space layout: metadata timestamps are invalid"
        ));
    }
    if metadata.space_uid.get_version() != Some(uuid::Version::SortRand) {
        return Err(anyhow!(
            "unsupported Space layout: space_uid must be a UUIDv7"
        ));
    }
    Ok(metadata.space_uid)
}

/// Verifies that a Space has completed the current bootstrap before a
/// recovery path is allowed to publish Node ownership. Metadata alone is not
/// a durable creation marker: a crash may leave it before the starter Form
/// and catalog are committed.
pub async fn validate_complete_bootstrap(op: &Operator, space_id: &str) -> Result<()> {
    let storage = OpendalStorage::from_operator(op);
    ensure_space_identity(&storage, space_id).await?;
    for directory in ["security", "forms", "assets", "sql_sessions"] {
        let path = format!("spaces/{space_id}/{directory}/");
        if !storage.exists(&path).await? {
            return Err(anyhow!(
                "incomplete Space bootstrap: missing directory {path}"
            ));
        }
    }
    let settings_path = format!("spaces/{space_id}/settings.json");
    if !storage.exists(&settings_path).await? {
        return Err(anyhow!("incomplete Space bootstrap: missing settings.json"));
    }
    let settings: serde_json::Value = storage.read_json(&settings_path).await?;
    if !settings.is_object()
        || settings
            .get("default_form")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(anyhow!(
            "incomplete Space bootstrap: settings.json requires default_form"
        ));
    }
    let workspace_path = format!("spaces/{space_id}");
    form::get_form(op, &workspace_path, "Entry")
        .await
        .context("incomplete Space bootstrap: starter Entry Form is missing")?;
    validate_local_space_permissions(op, space_id)?;
    Ok(())
}

pub async fn get_space_raw(op: &Operator, name: &str) -> Result<serde_json::Value> {
    validate_complete_bootstrap(op, name).await?;
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

    let mut meta = ensure_space_identity(storage, space_id).await?;
    if !storage.exists(&settings_path).await? {
        return Err(anyhow!("unsupported Space layout: missing settings.json"));
    }
    let mut settings: serde_json::Value = storage.read_json(&settings_path).await?;

    if let Some(name) = patch.get("name") {
        meta["name"] = name.clone();
    }
    if let Some(slug) = patch.get("slug").and_then(serde_json::Value::as_str) {
        validate_space_path_segment(slug)?;
        meta["slug"] = serde_json::Value::String(slug.to_string());
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

    if !settings.is_object()
        || settings
            .get("default_form")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(anyhow!(
            "unsupported Space layout: settings.json requires default_form"
        ));
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
    crate::authorization::ensure_authorization_write_fence().await?;
    validate_complete_bootstrap(op, space_id).await?;
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
