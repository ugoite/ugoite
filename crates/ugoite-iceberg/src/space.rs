use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use fs2::FileExt;
use futures::TryStreamExt;
use opendal::options::{ReadOptions, WriteOptions};
use opendal::{ErrorKind, Operator};
use rand::TryRng;
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex as AsyncMutex;
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
    if space_id.get_version() != Some(uuid::Version::SortRand) {
        return Err(anyhow!("UUID-addressed Space identity must be a UUIDv7"));
    }
    let directory_id = space_id.to_string();
    let storage = OpendalStorage::from_operator(op);
    create_space_with_storage(&storage, &directory_id, space_id, slug, root_path).await?;
    let ws_path = format!("spaces/{directory_id}");
    form::upsert_form(op, &ws_path, &starter_entry_form_definition()).await?;
    apply_local_space_permissions(op, &directory_id)?;
    Ok(())
}

/// Completes a UUID-addressed Space whose durable slug claim was written but
/// whose bootstrap was interrupted. The immutable metadata is never
/// regenerated: recovery only creates missing scaffold objects and the
/// starter Form, then reapplies local permissions.
pub async fn repair_space_with_identity(
    op: &Operator,
    space_uid: uuid::Uuid,
    slug: &str,
    root_path: &str,
) -> Result<()> {
    if space_uid.get_version() != Some(uuid::Version::SortRand) {
        return Err(anyhow!("UUID-addressed Space identity must be a UUIDv7"));
    }
    let directory_id = space_uid.to_string();
    validate_space_path_segment(&directory_id)?;
    validate_space_path_segment(slug)?;
    let storage = OpendalStorage::from_operator(op);
    let meta_path = format!("spaces/{directory_id}/meta.json");
    if !storage.exists(&meta_path).await? {
        return create_space_with_identity(op, space_uid, slug, root_path).await;
    }

    let meta = ensure_space_identity(&storage, &directory_id).await?;
    let metadata_uid = meta
        .get("space_uid")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .ok_or_else(|| anyhow!("Space metadata has no immutable space_uid"))?;
    if metadata_uid != space_uid {
        return Err(anyhow!(
            "UUID-addressed Space directory and metadata space_uid disagree"
        ));
    }
    if meta.get("slug").and_then(serde_json::Value::as_str) != Some(slug) {
        return Err(anyhow!(
            "Space slug claim does not match immutable Space metadata"
        ));
    }

    repair_space_scaffold(op, &directory_id, slug, root_path).await
}

/// Repairs a slug-addressed Space after reading its already durable metadata.
/// This is used only for legacy `spaces/{slug}` directories; a claim-only
/// record without metadata is safe to release and recreate instead.
pub async fn repair_space(
    op: &Operator,
    directory_id: &str,
    slug: &str,
    root_path: &str,
) -> Result<()> {
    validate_space_path_segment(directory_id)?;
    validate_space_path_segment(slug)?;
    let storage = OpendalStorage::from_operator(op);
    let meta = ensure_space_identity(&storage, directory_id).await?;
    if meta.get("slug").and_then(serde_json::Value::as_str) != Some(slug) {
        return Err(anyhow!(
            "Space slug claim does not match immutable Space metadata"
        ));
    }
    repair_space_scaffold(op, directory_id, slug, root_path).await
}

async fn repair_space_scaffold(
    op: &Operator,
    directory_id: &str,
    _slug: &str,
    _root_path: &str,
) -> Result<()> {
    let storage = OpendalStorage::from_operator(op);
    let ws_path = format!("spaces/{directory_id}");

    for directory in ["security", "forms", "assets", "sql_sessions"] {
        storage
            .create_dir(&format!("{ws_path}/{directory}/"))
            .await?;
    }
    let settings_path = format!("{ws_path}/settings.json");
    if !storage.exists(&settings_path).await? {
        storage
            .write_json(
                &settings_path,
                &serde_json::json!({"default_form": "Entry"}),
            )
            .await?;
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
    match form::get_form(op, &ws_path, "Entry").await {
        Ok(_) => {}
        Err(error)
            if error.chain().any(|cause| {
                cause
                    .downcast_ref::<opendal::Error>()
                    .is_some_and(|error| error.kind() == ErrorKind::NotFound)
            }) =>
        {
            form::upsert_form(op, &ws_path, &starter_entry_form_definition()).await?;
        }
        Err(error) => {
            return Err(error.context("read authoritative Entry Form during Space repair"))
        }
    }
    apply_local_space_permissions(op, &directory_id)?;
    Ok(())
}

async fn list_spaces_discovery_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
) -> Result<Vec<String>> {
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
    let spaces = list_spaces_discovery_with_storage(&storage).await?;
    // Directory listing is discovery only. Do not expose a metadata-only or
    // crash-left Space through a public enumeration result.
    for space_id in &spaces {
        validate_complete_bootstrap(op, space_id).await?;
    }
    Ok(spaces)
}

/// Discovers metadata-backed Space directories without deciding whether a
/// pending creation claim has made them publicly enumerable. Service-level
/// startup recovery uses this discovery result to skip live pending claims,
/// while still strictly validating every unclaimed or committed Space.
pub async fn list_spaces_discovery(op: &Operator) -> Result<Vec<String>> {
    let storage = OpendalStorage::from_operator(op);
    list_spaces_discovery_with_storage(&storage).await
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
    // Current UUID-addressed Spaces use their immutable UUID as the storage
    // directory. Legacy Spaces remain slug-addressed, even though their
    // metadata also carries an immutable UUID for identity binding.
    if let Ok(directory_uid) = uuid::Uuid::parse_str(expected_directory_id) {
        if directory_uid.get_version() == Some(uuid::Version::SortRand)
            && directory_uid != metadata.space_uid
        {
            return Err(anyhow!(
                "unsupported Space layout: UUID directory does not match space_uid"
            ));
        }
    }
    Ok(metadata.space_uid)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct SpacePatchJournal {
    #[serde(default = "default_space_patch_journal_status")]
    status: String,
    old_metadata: serde_json::Value,
    new_metadata: serde_json::Value,
    old_settings: serde_json::Value,
    new_settings: serde_json::Value,
}

const SPACE_PATCH_PENDING: &str = "pending";
const SPACE_PATCH_COMPLETE: &str = "complete";

fn default_space_patch_journal_status() -> String {
    SPACE_PATCH_PENDING.to_string()
}

fn space_patch_journal_path(space_id: &str) -> String {
    format!("spaces/{space_id}/.ugoite-space-patch.json")
}

fn local_space_patch_lock_path(op: &Operator, space_id: &str) -> Option<PathBuf> {
    if !matches!(op.info().scheme(), "fs" | "file") {
        return None;
    }
    Some(
        Path::new(op.info().root().as_str())
            .join("spaces")
            .join(space_id)
            .join(".ugoite-space-patch.lock"),
    )
}

async fn acquire_local_space_patch_lock(op: &Operator, space_id: &str) -> Result<Option<File>> {
    let Some(path) = local_space_patch_lock_path(op, space_id) else {
        return Ok(None);
    };
    let file = tokio::task::spawn_blocking(move || -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open Space patch lock {}", path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("lock Space patch {}", path.display()))?;
        Ok(file)
    })
    .await
    .context("join Space patch lock task")??;
    Ok(Some(file))
}

fn is_condition_not_match(error: &opendal::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::ConditionNotMatch | ErrorKind::AlreadyExists
    )
}

fn is_condition_not_match_anyhow(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<opendal::Error>()
            .is_some_and(is_condition_not_match)
            || cause
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists)
    })
}

async fn read_space_json_exact(
    op: &Operator,
    path: &str,
) -> Result<Option<(serde_json::Value, Option<String>)>> {
    let metadata = match op.stat(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let etag = metadata
        .etag()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let bytes = match etag.as_deref() {
        Some(etag) => {
            op.read_options(
                path,
                ReadOptions {
                    if_match: Some(etag.to_string()),
                    ..Default::default()
                },
            )
            .await?
        }
        None if matches!(op.info().scheme(), "memory" | "fs" | "file") => op.read(path).await?,
        None => bail!("exact read requires an ETag: {path}"),
    };
    Ok(Some((serde_json::from_slice(&bytes.to_vec())?, etag)))
}

async fn write_space_patch_value(
    op: &Operator,
    path: &str,
    value: &serde_json::Value,
    etag: Option<&str>,
) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    if let Some(etag) = etag {
        op.write_options(
            path,
            bytes,
            WriteOptions {
                if_match: Some(etag.to_string()),
                ..Default::default()
            },
        )
        .await?;
    } else {
        if matches!(op.info().scheme(), "fs" | "file") {
            write_local_space_json_atomic(op, path, &bytes, false).await?;
        } else if op.info().scheme() == "memory" {
            op.write(path, bytes).await?;
        } else {
            bail!("conditional Space patch write requires an ETag: {path}");
        }
    }
    restore_local_space_json_permissions(op, path)?;
    Ok(())
}

/// OpenDAL's local atomic writer creates its temporary file with the process
/// umask, then fixes permissions after the rename. Space metadata and settings
/// contain secrets and are validated as private files, so the mode must be
/// correct before the target becomes visible. This helper keeps that property
/// across a crash between rename and the caller's next instruction.
async fn write_local_space_json_atomic(
    op: &Operator,
    path: &str,
    bytes: &[u8],
    if_not_exists: bool,
) -> Result<()> {
    if !matches!(op.info().scheme(), "fs" | "file") {
        bail!("local atomic Space JSON writer used for non-local operator: {path}");
    }
    let target = Path::new(op.info().root().as_str()).join(path);
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("Space JSON path has no parent: {path}"))?;
    tokio::fs::create_dir_all(parent).await?;
    let file_name = target
        .file_name()
        .ok_or_else(|| anyhow!("Space JSON path has no file name: {path}"))?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.{}.tmp", uuid::Uuid::now_v7()));
    let result = async {
        let mut options = tokio::fs::OpenOptions::new();
        options.create_new(true).write(true).truncate(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);
        if if_not_exists {
            tokio::fs::hard_link(&temporary, &target).await?;
            tokio::fs::remove_file(&temporary).await?;
        } else {
            tokio::fs::rename(&temporary, &target).await?;
        }
        #[cfg(unix)]
        if let Ok(directory) = tokio::fs::File::open(parent).await {
            directory.sync_all().await?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}

fn restore_local_space_json_permissions(op: &Operator, path: &str) -> Result<()> {
    #[cfg(unix)]
    if matches!(op.info().scheme(), "fs" | "file") {
        let target = Path::new(op.info().root().as_str()).join(path);
        set_owner_only_mode(&target, 0o600)?;
    }
    Ok(())
}

async fn read_space_patch_journal_exact(
    op: &Operator,
    path: &str,
) -> Result<Option<(SpacePatchJournal, Option<String>)>> {
    let Some((value, etag)) = read_space_json_exact(op, path).await? else {
        return Ok(None);
    };
    Ok(Some((
        serde_json::from_value(value).context("decode Space patch journal")?,
        etag,
    )))
}

async fn complete_space_patch_journal(
    op: &Operator,
    path: &str,
    journal: &SpacePatchJournal,
    expected_etag: Option<&str>,
) -> Result<()> {
    let mut completed = journal.clone();
    completed.status = SPACE_PATCH_COMPLETE.to_string();
    let bytes = serde_json::to_vec_pretty(&completed)?;
    let result = if let Some(etag) = expected_etag {
        op.write_options(
            path,
            bytes,
            WriteOptions {
                if_match: Some(etag.to_string()),
                ..Default::default()
            },
        )
        .await
        .map(|_| ())
        .map_err(anyhow::Error::from)
    } else if matches!(op.info().scheme(), "fs" | "file") {
        write_local_space_json_atomic(op, path, &bytes, false).await?;
        Ok(())
    } else if op.info().scheme() == "memory" {
        op.write(path, bytes)
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)
    } else {
        bail!("completing Space patch journal requires an ETag: {path}");
    };
    match result {
        Ok(_) => Ok(()),
        Err(error) if is_condition_not_match_anyhow(&error) => {
            let Some((current, _)) = read_space_patch_journal_exact(op, path).await? else {
                bail!("Space patch journal disappeared while completing")
            };
            if current == completed {
                // Another worker completed this exact journal. A different
                // pending journal must never be treated as our completion.
                Ok(())
            } else {
                bail!("Space patch journal changed while completing")
            }
        }
        Err(error) => Err(error.into()),
    }
}

async fn write_pending_space_patch(
    op: &Operator,
    space_id: &str,
    path: &str,
    journal: &SpacePatchJournal,
) -> Result<Option<String>> {
    let bytes = serde_json::to_vec_pretty(journal)?;
    for _ in 0..4 {
        if let Some((existing, etag)) = read_space_patch_journal_exact(op, path).await? {
            if existing.status == SPACE_PATCH_PENDING {
                recover_pending_space_patch(op, space_id).await?;
                continue;
            }
            if existing.status != SPACE_PATCH_COMPLETE {
                bail!("invalid Space patch journal status: {}", existing.status);
            }
            let result = if let Some(etag) = etag {
                op.write_options(
                    path,
                    bytes.clone(),
                    WriteOptions {
                        if_match: Some(etag),
                        ..Default::default()
                    },
                )
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
            } else if matches!(op.info().scheme(), "fs" | "file") {
                write_local_space_json_atomic(op, path, &bytes, false).await
            } else if op.info().scheme() == "memory" {
                op.write(path, bytes.clone())
                    .await
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            } else {
                bail!("replacing Space patch journal requires an ETag: {path}");
            };
            match result {
                Ok(_) => {}
                Err(error) if is_condition_not_match_anyhow(&error) => continue,
                Err(error) => return Err(error.into()),
            }
        } else {
            let result = if matches!(op.info().scheme(), "fs" | "file") {
                write_local_space_json_atomic(op, path, &bytes, true).await
            } else {
                op.write_options(
                    path,
                    bytes.clone(),
                    WriteOptions {
                        if_not_exists: true,
                        ..Default::default()
                    },
                )
                .await
                .map(|_| ())
                .map_err(anyhow::Error::from)
            };
            match result {
                Ok(_) => {}
                Err(error) if is_condition_not_match_anyhow(&error) => continue,
                Err(error) => return Err(error.into()),
            }
        }
        let Some((current, etag)) = read_space_patch_journal_exact(op, path).await? else {
            continue;
        };
        if current == *journal {
            return Ok(etag);
        }
        // A concurrent writer replaced the journal between our conditional
        // write and verification. Recover or retry without ever borrowing
        // that writer's ETag for this transaction.
        if current.status == SPACE_PATCH_PENDING {
            recover_pending_space_patch(op, space_id).await?;
        }
    }
    bail!("Space patch journal changed while publishing")
}

async fn recover_pending_space_patch(op: &Operator, space_id: &str) -> Result<()> {
    let journal_path = space_patch_journal_path(space_id);
    let Some((value, journal_etag)) = read_space_json_exact(op, &journal_path).await? else {
        return Ok(());
    };
    let journal: SpacePatchJournal =
        serde_json::from_value(value).context("decode pending Space patch journal")?;
    if journal.status == SPACE_PATCH_COMPLETE {
        return Ok(());
    }
    if journal.status != SPACE_PATCH_PENDING {
        bail!("invalid Space patch journal status: {}", journal.status);
    }
    validate_current_space_metadata(space_id, &journal.new_metadata)?;
    if !journal.new_settings.is_object()
        || journal
            .new_settings
            .get("default_form")
            .and_then(serde_json::Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(anyhow!("pending Space patch settings are invalid"));
    }
    let meta_path = format!("spaces/{space_id}/meta.json");
    let settings_path = format!("spaces/{space_id}/settings.json");
    let (current_meta, metadata_etag) = read_space_json_exact(op, &meta_path)
        .await?
        .context("Space metadata disappeared during patch recovery")?;
    let (current_settings, settings_etag) = read_space_json_exact(op, &settings_path)
        .await?
        .context("Space settings disappeared during patch recovery")?;
    let metadata_is_expected =
        current_meta == journal.old_metadata || current_meta == journal.new_metadata;
    let settings_is_expected =
        current_settings == journal.old_settings || current_settings == journal.new_settings;
    if !metadata_is_expected || !settings_is_expected {
        // A different writer won the race after this journal was written. Do
        // not let a stale, incomplete transaction brick every future Space
        // read. The current values are still authoritative only after their
        // own schemas have been checked; otherwise fail closed as corruption.
        validate_current_space_metadata(space_id, &current_meta)?;
        if !current_settings.is_object()
            || current_settings
                .get("default_form")
                .and_then(serde_json::Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err(anyhow!(
                "current Space settings are invalid while recovering a stale patch journal"
            ));
        }
        complete_space_patch_journal(op, &journal_path, &journal, journal_etag.as_deref()).await?;
        return Ok(());
    }
    if current_meta != journal.new_metadata {
        if current_meta != journal.old_metadata {
            return Err(anyhow!(
                "pending Space patch journal does not match current metadata"
            ));
        }
        write_space_patch_value(
            op,
            &meta_path,
            &journal.new_metadata,
            metadata_etag.as_deref(),
        )
        .await?;
    }
    if current_settings != journal.new_settings {
        if current_settings != journal.old_settings {
            return Err(anyhow!(
                "pending Space patch journal does not match current settings"
            ));
        }
        write_space_patch_value(
            op,
            &settings_path,
            &journal.new_settings,
            settings_etag.as_deref(),
        )
        .await?;
    }
    complete_space_patch_journal(op, &journal_path, &journal, journal_etag.as_deref()).await?;
    Ok(())
}

/// Verifies that a Space has completed the current bootstrap before a
/// recovery path is allowed to publish Node ownership. Metadata alone is not
/// a durable creation marker: a crash may leave it before the starter Form
/// and catalog are committed.
pub async fn validate_complete_bootstrap(op: &Operator, space_id: &str) -> Result<()> {
    validate_space_path_segment(space_id)?;
    let patch_serializer = space_patch_serializer(op, space_id);
    let _patch_guard = patch_serializer.lock().await;
    let _local_patch_lock = acquire_local_space_patch_lock(op, space_id).await?;
    validate_complete_bootstrap_locked(op, space_id).await
}

async fn validate_complete_bootstrap_locked(op: &Operator, space_id: &str) -> Result<()> {
    recover_pending_space_patch(op, space_id).await?;
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
    // Repair permissions before validation as a compatibility guard for a
    // process that crashed after publishing an older umask-created JSON file.
    // New writes already establish 0600 before rename; this closes the
    // recovery path for values that were visible before that guarantee.
    apply_local_space_permissions(op, space_id)?;
    validate_local_space_permissions(op, space_id)?;
    Ok(())
}

pub async fn get_space_raw(op: &Operator, name: &str) -> Result<serde_json::Value> {
    validate_space_path_segment(name)?;
    if let Some(lock_path) = local_space_patch_lock_path(op, name) {
        if !lock_path.parent().is_some_and(std::path::Path::is_dir) {
            return Err(AppError::not_found(
                ErrorCode::SpaceNotFound,
                format!("Space not found: {name}"),
            )
            .into());
        }
    }
    let patch_serializer = space_patch_serializer(op, name);
    let _patch_guard = patch_serializer.lock().await;
    let _local_patch_lock = acquire_local_space_patch_lock(op, name).await?;
    validate_complete_bootstrap_locked(op, name).await?;
    let storage = OpendalStorage::from_operator(op);
    get_space_raw_with_storage(&storage, name).await
}

async fn patch_space_with_operator(
    op: &Operator,
    space_id: &str,
    patch: &serde_json::Value,
    expected_slug: Option<&str>,
) -> Result<serde_json::Value> {
    validate_space_path_segment(space_id)?;
    let patch_serializer = space_patch_serializer(op, space_id);
    let _patch_guard = patch_serializer.lock().await;
    let _local_patch_lock = acquire_local_space_patch_lock(op, space_id).await?;
    recover_pending_space_patch(op, space_id).await?;
    let meta_path = format!("spaces/{space_id}/meta.json");
    let settings_path = format!("spaces/{space_id}/settings.json");

    let metadata = match op.stat(&meta_path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(AppError::not_found(
                ErrorCode::SpaceNotFound,
                format!("Space not found: {space_id}"),
            )
            .into())
        }
        Err(error) => return Err(error.into()),
    };
    let etag = metadata
        .etag()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let metadata_bytes = match etag.as_deref() {
        Some(etag) => op
            .read_options(
                &meta_path,
                ReadOptions {
                    if_match: Some(etag.to_string()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|error| {
                if error.kind() == ErrorKind::ConditionNotMatch {
                    anyhow!("Space metadata changed before patch")
                } else {
                    error.into()
                }
            })?,
        None if matches!(op.info().scheme(), "memory" | "fs" | "file") => {
            op.read(&meta_path).await?
        }
        None => {
            return Err(anyhow!(
                "Space metadata update requires an exact storage ETag"
            ))
        }
    };
    let mut meta: serde_json::Value = serde_json::from_slice(&metadata_bytes.to_vec())?;
    validate_current_space_metadata(space_id, &meta)?;
    let current_slug = meta
        .get("slug")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow!("Space metadata has no slug"))?;
    if expected_slug.is_some_and(|expected| expected != current_slug) {
        return Err(AppError::conflict(
            ErrorCode::SpaceAlreadyExists,
            "Space metadata changed before patch",
        )
        .into());
    }
    let old_meta = meta.clone();
    let (mut settings, settings_etag) = read_space_json_exact(op, &settings_path)
        .await?
        .ok_or_else(|| {
            AppError::not_found(
                ErrorCode::SpaceNotFound,
                format!("Space settings not found: {space_id}"),
            )
        })?;
    let old_settings = settings.clone();

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

    validate_current_space_metadata(space_id, &meta)?;

    let journal_path = space_patch_journal_path(space_id);
    let journal = SpacePatchJournal {
        status: SPACE_PATCH_PENDING.to_string(),
        old_metadata: old_meta,
        new_metadata: meta.clone(),
        old_settings,
        new_settings: settings.clone(),
    };
    let journal_etag = write_pending_space_patch(op, space_id, &journal_path, &journal).await?;

    let meta_bytes = serde_json::to_vec_pretty(&meta)?;
    match etag {
        Some(etag) => op
            .write_options(
                &meta_path,
                meta_bytes,
                WriteOptions {
                    if_match: Some(etag),
                    ..Default::default()
                },
            )
            .await
            .map(|_| ())
            .map_err(|error| {
                if error.kind() == ErrorKind::ConditionNotMatch {
                    anyhow!("Space metadata changed during patch")
                } else {
                    error.into()
                }
            })?,
        None if matches!(op.info().scheme(), "fs" | "file") => {
            write_local_space_json_atomic(op, &meta_path, &meta_bytes, false).await?
        }
        None => {
            op.write(&meta_path, meta_bytes).await?;
        }
    };
    restore_local_space_json_permissions(op, &meta_path)?;
    write_space_patch_value(op, &settings_path, &settings, settings_etag.as_deref()).await?;
    // Keep a completed journal as a version-fenced tombstone. A future patch
    // can replace it with its own pending transaction, but recovery must never
    // delete a newer journal after an exact read.
    complete_space_patch_journal(op, &journal_path, &journal, journal_etag.as_deref()).await?;

    let mut merged = meta;
    merged["settings"] = settings;
    Ok(merged)
}

fn space_patch_serializer(op: &Operator, space_id: &str) -> Arc<AsyncMutex<()>> {
    static SERIALIZERS: OnceLock<StdMutex<BTreeMap<String, Arc<AsyncMutex<()>>>>> = OnceLock::new();
    let key = format!("{}:{}:{space_id}", op.info().scheme(), op.info().root());
    let serializers = SERIALIZERS.get_or_init(|| StdMutex::new(BTreeMap::new()));
    let mut serializers = serializers.lock().expect("Space patch serializer poisoned");
    serializers
        .entry(key)
        .or_insert_with(|| Arc::new(AsyncMutex::new(())))
        .clone()
}

pub async fn patch_space(
    op: &Operator,
    space_id: &str,
    patch: &serde_json::Value,
) -> Result<serde_json::Value> {
    crate::authorization::ensure_authorization_write_fence().await?;
    validate_complete_bootstrap(op, space_id).await?;
    patch_space_with_operator(op, space_id, patch, None).await
}

/// Patches one Space only if its slug is still the value observed by the
/// caller. The metadata ETag is the authoritative serialization boundary for
/// concurrent renames across processes and shared storage instances.
pub async fn patch_space_if_slug(
    op: &Operator,
    space_id: &str,
    patch: &serde_json::Value,
    expected_slug: &str,
) -> Result<serde_json::Value> {
    crate::authorization::ensure_authorization_write_fence().await?;
    validate_complete_bootstrap(op, space_id).await?;
    patch_space_with_operator(op, space_id, patch, Some(expected_slug)).await
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
