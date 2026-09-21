use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use fs2::FileExt;
use futures::TryStreamExt;
use opendal::options::WriteOptions;
use opendal::{EntryMode, ErrorKind, Operator};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::integrity::RealIntegrityProvider;
use crate::{
    asset,
    authorization::{
        effective_actions_for_state, AuthorizationLease, AuthorizationState, Authorizer,
        ResourceKind, ResourceRef,
    },
    entry, form, iceberg_store, index, preferences, saved_sql, search, space, sql_session,
};
use crate::{CheckpointIntegrityError, CheckpointUnavailable, PublicationRef};
use ugoite_core::error::{AppError, ErrorCode, ErrorKind as AppErrorKind};
use ugoite_core::query::EntryScope;
use ugoite_core::sql_query::{
    SqlContinuation, SqlQueryCountRequest, SqlQueryError, SqlQueryPage, SqlQueryRequest,
};
use ugoite_domain::change::{ChangeCommand, RunId};
use ugoite_domain::form::{sql_relation_name, FormDefinition};
use ugoite_domain::id::{
    validate_asset_id, validate_entry_id, validate_form_name, validate_revision_id,
    validate_space_id, validate_sql_id, validate_sql_session_id, FormId,
};
use ugoite_domain::identity::Action;
use ugoite_storage::{
    is_local_operator, operator_from_uri, operator_from_uri_with_endpoint, OpendalStorage,
    SpaceCatalogStore, StorageBackend,
};

pub const MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS: &[&str] = &[
    "admin_user_ids",
    "invitations",
    "member_roles",
    "members",
    "member_invitations",
    "membership_version",
    "owner_user_id",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpacePermission {
    Read,
    WriteContent,
    ManageSpace,
    ManageMembers,
}

const STORAGE_CONNECTION_PROBE_TIMEOUT: Duration = Duration::from_secs(5);

impl space::StorageConnectionTestConfig {
    /// Parse either the direct request shape or the `{ "storage_config": ... }`
    /// envelope used by the REST and portable protocol adapters.
    pub fn from_payload(payload: &Value) -> Result<Self> {
        let value = payload
            .get("storage_config")
            .cloned()
            .unwrap_or_else(|| payload.clone());
        let object = value.as_object().ok_or_else(|| {
            AppError::invalid_input(ErrorCode::InvalidInput, "storage_config.uri is required")
        })?;
        let uri = object.get("uri").and_then(Value::as_str).ok_or_else(|| {
            AppError::invalid_input(ErrorCode::InvalidInput, "storage_config.uri is required")
        })?;
        let endpoint = match object.get("endpoint") {
            None | Some(Value::Null) => None,
            Some(Value::String(endpoint)) => Some(endpoint.clone()),
            Some(_) => {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    "storage_config.endpoint must be a string",
                )
                .into())
            }
        };
        Ok(Self {
            uri: uri.to_string(),
            endpoint,
        })
    }
}

/// Probe a proposed storage binding through the same Rust service boundary
/// used by the server. Adapters provide only the request payload; they do not
/// parse provider-specific configuration or construct OpenDAL operators.
pub async fn probe_storage_connection(
    config: &space::StorageConnectionTestConfig,
) -> Result<Value> {
    let trimmed = config.uri.trim();
    if trimmed.is_empty() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "storage_config.uri is required",
        )
        .into());
    }
    let mode = storage_connection_mode(trimmed)?;
    let endpoint = validate_storage_endpoint(config.endpoint.as_deref())?;
    if endpoint.is_some() && mode != "s3" {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "storage_config.endpoint is only supported for S3 storage",
        )
        .into());
    }
    let operator = operator_from_uri_with_endpoint(trimmed, endpoint)
        .map_err(map_operator_configuration_error)?;
    validate_required_storage_capabilities(&operator, mode)?;
    probe_storage_backend(&operator, mode, STORAGE_CONNECTION_PROBE_TIMEOUT).await?;
    Ok(json!({"status": "ok", "mode": mode}))
}

fn map_operator_configuration_error(error: anyhow::Error) -> anyhow::Error {
    if let Some(opendal_error) = error.downcast_ref::<opendal::Error>() {
        return map_storage_backend_error(opendal_error);
    }
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<std::io::Error>().is_some())
    {
        return AppError::dependency_unavailable(
            ErrorCode::StorageConnectionFailed,
            format!("storage connection failed: {error}"),
        )
        .into();
    }
    AppError::invalid_input(
        ErrorCode::InvalidInput,
        format!("invalid storage configuration: {error}"),
    )
    .into()
}

fn map_storage_backend_error(error: &opendal::Error) -> anyhow::Error {
    match error.kind() {
        ErrorKind::PermissionDenied => AppError::forbidden("storage authentication failed").into(),
        ErrorKind::ConfigInvalid => AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!("invalid storage configuration: {error}"),
        )
        .into(),
        ErrorKind::Unsupported => AppError::new(
            ugoite_core::error::ErrorKind::Unimplemented,
            ErrorCode::StorageMutationUnavailable,
            format!("storage backend does not support the required operation: {error}"),
        )
        .into(),
        _ => AppError::dependency_unavailable(
            ErrorCode::StorageConnectionFailed,
            format!("storage connection failed: {error}"),
        )
        .into(),
    }
}

fn map_storage_contract_error(error: anyhow::Error) -> anyhow::Error {
    if let Some(opendal_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<opendal::Error>())
    {
        return map_storage_backend_error(opendal_error);
    }
    let message = format!("{error:#}");
    if message.contains("timed out") || message.contains("timeout") {
        return AppError::dependency_unavailable(
            ErrorCode::StorageConnectionFailed,
            format!("storage connection failed: {message}"),
        )
        .into();
    }
    AppError::new(
        ugoite_core::error::ErrorKind::Unimplemented,
        ErrorCode::StorageMutationUnavailable,
        format!("storage backend does not satisfy Ugoite's contract: {message}"),
    )
    .into()
}

fn validate_required_storage_capabilities(operator: &Operator, mode: &str) -> Result<()> {
    let capabilities = operator.info().capability();
    let mut missing = Vec::new();
    for (supported, name) in [
        (capabilities.stat, "stat"),
        (capabilities.read, "read"),
        (capabilities.write, "write"),
        (capabilities.delete, "delete"),
        (capabilities.list, "list"),
    ] {
        if !supported {
            missing.push(name);
        }
    }
    if mode == "s3" {
        for (supported, name) in [
            (capabilities.read_with_if_match, "read_with_if_match"),
            (capabilities.write_with_if_match, "write_with_if_match"),
            (
                capabilities.write_with_if_not_exists,
                "write_with_if_not_exists",
            ),
        ] {
            if !supported {
                missing.push(name);
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    Err(AppError::new(
        ugoite_core::error::ErrorKind::Unimplemented,
        ErrorCode::StorageMutationUnavailable,
        format!(
            "storage backend does not satisfy Ugoite's required capabilities: {}",
            missing.join(", ")
        ),
    )
    .into())
}

async fn probe_storage_backend(operator: &Operator, mode: &str, timeout: Duration) -> Result<()> {
    match tokio::time::timeout(timeout, async {
        let mut lister = operator
            .lister("")
            .await
            .map_err(|error| map_storage_backend_error(&error))?;
        lister
            .try_next()
            .await
            .map_err(|error| map_storage_backend_error(&error))?;
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            return Err(AppError::dependency_unavailable(
                ErrorCode::StorageConnectionFailed,
                format!(
                    "storage connection failed: probe timed out after {}ms",
                    timeout.as_millis()
                ),
            )
            .into())
        }
    }

    // `verify_shared_writes` owns its shorter operation timeout and cleanup
    // phase. Do not wrap it in this same outer timeout: cancelling the inner
    // future at the deadline would skip deletion of its temporary probe
    // object.
    if mode == "s3" && !is_local_operator(operator) {
        SpaceCatalogStore::new(operator.clone(), "_ugoite/connection-probes")
            .map_err(map_storage_contract_error)?
            .verify_shared_writes()
            .await
            .map_err(map_storage_contract_error)?;
    }
    Ok(())
}

fn storage_connection_mode(uri: &str) -> Result<&'static str> {
    if uri.starts_with("memory://") {
        return Ok("memory");
    }
    if uri.starts_with("file://")
        || uri.starts_with("fs://")
        || uri.starts_with('/')
        || uri.starts_with('.')
    {
        return Ok("local");
    }
    if uri.starts_with("s3://") {
        validate_remote_storage_uri(uri)?;
        return Ok("s3");
    }
    Err(AppError::new(
        ugoite_core::error::ErrorKind::Unimplemented,
        ErrorCode::StorageMutationUnavailable,
        format!("unsupported storage backend: {uri}"),
    )
    .into())
}

pub fn validate_storage_endpoint(endpoint: Option<&str>) -> Result<Option<&str>> {
    let Some(endpoint) = endpoint.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parsed = url::Url::parse(endpoint).map_err(|_| {
        AppError::invalid_input(ErrorCode::InvalidInput, "invalid storage endpoint")
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!("unsupported storage endpoint scheme: {}", parsed.scheme()),
        )
        .into());
    }
    let host = parsed.host_str().ok_or_else(|| {
        AppError::invalid_input(ErrorCode::InvalidInput, "storage endpoint host is required")
    })?;
    if is_blocked_storage_host(host) {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!("blocked storage endpoint host: {host}"),
        )
        .into());
    }
    Ok(Some(endpoint))
}

fn validate_remote_storage_uri(uri: &str) -> Result<()> {
    let parsed = url::Url::parse(uri)
        .map_err(|_| AppError::invalid_input(ErrorCode::InvalidInput, "invalid storage URI"))?;
    let host = parsed.host_str().ok_or_else(|| {
        AppError::invalid_input(
            ErrorCode::InvalidInput,
            "S3 storage URI must include a bucket",
        )
    })?;
    if host.is_empty() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "S3 storage URI must include a bucket",
        )
        .into());
    }
    if is_blocked_storage_host(host) {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!("blocked storage endpoint host: {host}"),
        )
        .into());
    }
    Ok(())
}

fn is_blocked_storage_host(host: &str) -> bool {
    let normalized = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if normalized == "localhost" {
        return true;
    }
    normalized
        .parse::<IpAddr>()
        .is_ok_and(is_blocked_storage_address)
}

fn is_blocked_storage_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_blocked_ipv4_address(address),
        IpAddr::V6(address) => {
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unique_local()
                || address.is_unicast_link_local()
                || address.to_ipv4().is_some_and(is_blocked_ipv4_address)
        }
    }
}

fn is_blocked_ipv4_address(address: Ipv4Addr) -> bool {
    address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_unspecified()
}

/// Durable outcome of an operator-local Space create intent.
///
/// `Created` is the first successful commit; `Existing` is a retry against
/// the same fully-committed operator-local Space identity. The strict
/// [`UgoiteService::create_operator_space`] primitive never returns
/// `Existing`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpaceCreateOutcome {
    Created(Uuid),
    Existing(Uuid),
}

impl SpaceCreateOutcome {
    pub fn space_id(self) -> Uuid {
        match self {
            Self::Created(id) | Self::Existing(id) => id,
        }
    }

    pub fn created(self) -> bool {
        matches!(self, Self::Created(_))
    }
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApplyOperation {
    Create {
        id: Option<String>,
        form: String,
        tags: Vec<String>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
    },
    Update {
        id: String,
        version_token: String,
        form: Option<String>,
        tags: Option<Vec<String>>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
    },
    Remove {
        id: String,
    },
}

#[derive(Clone)]
pub struct UgoiteService {
    operator: Operator,
    root_uri: String,
    background_refresh: bool,
}

struct CurrentSqlSessionExecutionAuthorization {
    policy_hash: String,
    query_policy: index::SqlSessionQueryPolicy,
}

struct AssetTextRefreshWorker {
    notify: Notify,
    started: AtomicBool,
    pending: AtomicBool,
}

static ASSET_TEXT_REFRESH_WORKERS: OnceLock<
    StdMutex<BTreeMap<String, Arc<AssetTextRefreshWorker>>>,
> = OnceLock::new();
static SPACE_CREATION_SERIALIZER: OnceLock<Arc<AsyncMutex<()>>> = OnceLock::new();

const SPACE_SLUG_CLAIMS_DIR: &str = "spaces/.ugoite-space-slug-claims/";
const SPACE_SLUG_COMMITTED_SUFFIX: &str = ".committed";
const SPACE_SLUG_CLAIM_LEASE: Duration = Duration::from_secs(60);
const SPACE_SLUG_CLAIM_HEARTBEAT: Duration = Duration::from_secs(10);

const ASSET_TEXT_REFRESH_DEBOUNCE: Duration = Duration::from_millis(250);
const ASSET_TEXT_REFRESH_OPERATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_BACKGROUND_REFRESH_WORKERS: usize = 1024;
const MAX_BACKGROUND_REFRESH_RETRIES: usize = 8;
const MAX_AUTHORIZED_SCOPE_FORMS: usize = 100_000;
const MAX_AUTHORIZED_SCOPE_FORM_DEFINITION_BYTES: usize = 256 * 1024 * 1024;

fn is_explicit_authorization_denial(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<AppError>()
            .is_some_and(|error| error.kind() == AppErrorKind::Forbidden)
    })
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
struct SpaceSlugClaim {
    slug: String,
    space_name: String,
    space_id: String,
    state: String,
    claim_id: Uuid,
    created_at: String,
    heartbeat_at: String,
    expires_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_principal_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_display_name: Option<String>,
}

impl SpaceSlugClaim {
    fn is_expired(&self) -> Result<bool> {
        Ok(DateTime::parse_from_rfc3339(&self.expires_at)?.with_timezone(&Utc) <= Utc::now())
    }
}

struct SpaceSlugClaimLease {
    lost: Arc<AtomicBool>,
    heartbeat: Option<JoinHandle<()>>,
}

impl SpaceSlugClaimLease {
    fn ensure_held(&self) -> Result<()> {
        if self.lost.load(Ordering::Acquire) {
            bail!("Space slug claim lease was lost")
        }
        Ok(())
    }

    async fn finish(mut self) -> Result<()> {
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.abort();
            let _ = heartbeat.await;
        }
        self.ensure_held()
    }
}

impl Drop for SpaceSlugClaimLease {
    fn drop(&mut self) {
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.abort();
        }
    }
}

async fn renew_space_slug_claim(operator: &Operator, claim: &SpaceSlugClaim) -> Result<()> {
    let _claim_lock = acquire_local_space_slug_claim_lock(operator, &claim.slug).await?;
    let path = format!("{SPACE_SLUG_CLAIMS_DIR}{}.json", claim.slug);
    let metadata = operator.stat(&path).await?;
    let etag = metadata
        .etag()
        .filter(|etag| !etag.is_empty())
        .map(str::to_owned);
    if etag.is_none() && !matches!(operator.info().scheme(), "memory" | "fs" | "file") {
        bail!("shared Space slug claim renewal requires an exact ETag");
    }
    let current = operator
        .read_options(
            &path,
            opendal::options::ReadOptions {
                if_match: etag.clone(),
                ..Default::default()
            },
        )
        .await?;
    let mut current: SpaceSlugClaim = serde_json::from_slice(&current.to_vec())?;
    if current.claim_id != claim.claim_id || current.state != "pending" {
        bail!("Space slug claim ownership changed")
    }
    let now = Utc::now();
    current.heartbeat_at = now.to_rfc3339();
    current.expires_at = (now + ChronoDuration::from_std(SPACE_SLUG_CLAIM_LEASE)?).to_rfc3339();
    let bytes = serde_json::to_vec(&current)?;
    if let Some(etag) = etag {
        operator
            .write_options(
                &path,
                bytes,
                WriteOptions {
                    if_match: Some(etag.to_string()),
                    ..Default::default()
                },
            )
            .await?;
    } else if matches!(operator.info().scheme(), "memory" | "fs" | "file") {
        operator.write(&path, bytes).await?;
    } else {
        bail!("shared Space slug claim renewal requires an exact ETag")
    }
    Ok(())
}

fn local_space_slug_claim_lock_path(operator: &Operator, slug: &str) -> Option<PathBuf> {
    if !matches!(operator.info().scheme(), "fs" | "file") {
        return None;
    }
    Some(
        Path::new(operator.info().root().as_str())
            .join(SPACE_SLUG_CLAIMS_DIR.trim_end_matches('/'))
            .join(format!("{slug}.lock")),
    )
}

async fn acquire_local_space_slug_claim_lock(
    operator: &Operator,
    slug: &str,
) -> Result<Option<File>> {
    let Some(path) = local_space_slug_claim_lock_path(operator, slug) else {
        return Ok(None);
    };
    let file = tokio::task::spawn_blocking(move || -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open Space slug claim lock {}", path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("lock Space slug claim {}", path.display()))?;
        Ok(file)
    })
    .await
    .context("join Space slug claim lock task")??;
    Ok(Some(file))
}

/// The admitted write context shared by structured Entry mutations. `scopes`
/// is the authorized form/entry scope map, `integrity`
/// and `workspace` are the mutation context, and the held lease keeps the
/// authorization snapshot pinned across the write.
struct EntryAuthorizedWritePrelude {
    scopes: BTreeMap<String, EntryScope>,
    integrity: RealIntegrityProvider,
    workspace: String,
    _authorization_lease: AuthorizationLease,
}

impl UgoiteService {
    pub fn new(root_uri: impl Into<String>) -> Result<Self> {
        Self::new_with_endpoint(root_uri, None)
    }

    pub fn new_with_endpoint(root_uri: impl Into<String>, endpoint: Option<&str>) -> Result<Self> {
        let root_uri = root_uri.into();
        let operator = operator_from_uri_with_endpoint(&root_uri, endpoint)?;
        Ok(Self {
            operator,
            root_uri,
            background_refresh: true,
        })
    }

    /// Creates the local CLI service variant. A one-shot CLI process must
    /// return after the authoritative commit; its detached refresh task would
    /// otherwise be dropped at process exit. `ugoite index run` is the
    /// explicit derived-repair command for this mode.
    pub fn new_without_background_refresh(root_uri: impl Into<String>) -> Result<Self> {
        let root_uri = root_uri.into();
        let operator = operator_from_uri(&root_uri)?;
        Ok(Self {
            operator,
            root_uri,
            background_refresh: false,
        })
    }

    pub fn from_operator(operator: Operator, root_uri: impl Into<String>) -> Self {
        Self {
            operator,
            root_uri: root_uri.into(),
            background_refresh: true,
        }
    }

    pub fn operator(&self) -> &Operator {
        &self.operator
    }

    pub fn root_uri(&self) -> &str {
        &self.root_uri
    }

    pub fn workspace_path(&self, space_id: &str) -> String {
        format!("spaces/{space_id}")
    }

    fn sql_query_signing_key(&self, space_id: &str) -> Vec<u8> {
        let configured =
            std::env::var("UGOITE_QUERY_CURSOR_SECRET").unwrap_or_else(|_| self.root_uri.clone());
        let mut digest = Sha256::new();
        digest.update(configured.as_bytes());
        digest.update([0]);
        digest.update(space_id.as_bytes());
        digest.finalize().to_vec()
    }

    async fn validate_complete_space(&self, space_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        space::validate_complete_bootstrap(&self.operator, space_id).await
    }

    /// Schedules a best-effort derived refresh after an authoritative write.
    ///
    /// The authoritative Catalog Head is the durable commit-coupled refresh
    /// intent: a committed mutation changes its source coordinate, and a
    /// stale/missing Derived Head is rearmed at startup. This keeps marker
    /// storage and the process-local worker entirely out of mutation latency.
    fn schedule_asset_text_refresh(&self, space_id: &str) {
        if !self.background_refresh {
            return;
        }
        self.enqueue_asset_text_refresh(space_id);
    }

    fn enqueue_asset_text_refresh(&self, space_id: &str) {
        let key = self.asset_text_refresh_worker_key(space_id);
        let workers = ASSET_TEXT_REFRESH_WORKERS.get_or_init(|| StdMutex::new(BTreeMap::new()));
        let worker = {
            let mut workers = workers
                .lock()
                .expect("AssetText refresh worker map poisoned");
            if !workers.contains_key(&key) && workers.len() >= MAX_BACKGROUND_REFRESH_WORKERS {
                // Refresh is an optimization and must not create an unbounded
                // process-global registry. The next explicit index run can
                // repair freshness when the registry is busy.
                return;
            }
            let worker = workers
                .entry(key)
                .or_insert_with(|| {
                    Arc::new(AssetTextRefreshWorker {
                        notify: Notify::new(),
                        started: AtomicBool::new(false),
                        pending: AtomicBool::new(false),
                    })
                })
                .clone();
            // Keep lookup, coalescing flag, and notification under the same
            // map lock as idle-worker removal. A caller that already found an
            // old worker cannot enqueue work after that worker has exited.
            worker.pending.store(true, Ordering::Release);
            worker.notify.notify_one();
            worker
        };
        if worker
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let op = self.operator.clone();
            let ws_path = self.workspace_path(space_id);
            let worker_key = self.asset_text_refresh_worker_key(space_id);
            let worker = worker.clone();
            tokio::spawn(async move {
                let mut refresh_failures = 0usize;
                loop {
                    worker.notify.notified().await;
                    // Let the authoritative request and a short burst of
                    // follow-up mutations/read-backs settle before opening
                    // the derived read snapshot. Refresh remains best effort
                    // and process-local; this only avoids making an immediate
                    // post-mutation read contend with the rebuild.
                    tokio::time::sleep(ASSET_TEXT_REFRESH_DEBOUNCE).await;
                    while worker.pending.swap(false, Ordering::AcqRel) {
                        let shared = !ugoite_storage::is_local_operator(&op);
                        let result =
                            tokio::time::timeout(ASSET_TEXT_REFRESH_OPERATION_TIMEOUT, async {
                                if shared {
                                    crate::derived_relation::rebuild_asset_text_shared(
                                        &op, &ws_path,
                                    )
                                    .await
                                } else {
                                    crate::derived_relation::rebuild_asset_text(&op, &ws_path).await
                                }
                            })
                            .await
                            .map_err(|_| anyhow!("AssetText refresh operation timed out"))
                            .and_then(|result| result);
                        if result.is_err() {
                            refresh_failures = refresh_failures.saturating_add(1);
                            if refresh_failures <= MAX_BACKGROUND_REFRESH_RETRIES {
                                // The authoritative source coordinate remains
                                // a durable refresh intent. Retry transient
                                // failures, but let the worker terminate after
                                // a bounded attempt count so a permanent
                                // backend failure cannot consume a registry
                                // slot forever.
                                worker.pending.store(true, Ordering::Release);
                                worker.notify.notify_one();
                                let delay = Duration::from_secs(1u64 << refresh_failures.min(6));
                                tokio::time::sleep(delay).await;
                            } else {
                                eprintln!(
                                    "AssetText refresh abandoned after {} failures for {}",
                                    refresh_failures, ws_path
                                );
                            }
                        } else {
                            match crate::derived_relation::asset_text_refresh_needed(&op, &ws_path)
                                .await
                            {
                                Ok(true) => {
                                    // A marker created while the build was
                                    // finalizing belongs to a newer
                                    // authoritative mutation. Keep the same
                                    // worker alive and process that request.
                                    worker.pending.store(true, Ordering::Release);
                                    worker.notify.notify_one();
                                    refresh_failures = 0;
                                }
                                Ok(false) => {
                                    refresh_failures = 0;
                                }
                                Err(error) => {
                                    refresh_failures = refresh_failures.saturating_add(1);
                                    if refresh_failures <= MAX_BACKGROUND_REFRESH_RETRIES {
                                        worker.pending.store(true, Ordering::Release);
                                        worker.notify.notify_one();
                                        let delay =
                                            Duration::from_secs(1u64 << refresh_failures.min(6));
                                        tokio::time::sleep(delay).await;
                                    } else {
                                        eprintln!(
                                            "AssetText refresh freshness check abandoned after {} failures for {}: {error:#}",
                                            refresh_failures, ws_path
                                        );
                                    }
                                }
                            }
                        }
                    }
                    let should_exit = if !worker.pending.load(Ordering::Acquire) {
                        let workers = ASSET_TEXT_REFRESH_WORKERS
                            .get_or_init(|| StdMutex::new(BTreeMap::new()));
                        let mut workers = workers
                            .lock()
                            .expect("AssetText refresh worker map poisoned");
                        let is_current = workers
                            .get(&worker_key)
                            .is_some_and(|current| Arc::ptr_eq(current, &worker));
                        if is_current && !worker.pending.load(Ordering::Acquire) {
                            workers.remove(&worker_key);
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    };
                    if should_exit {
                        worker.started.store(false, Ordering::Release);
                        return;
                    }
                }
            });
        }
    }

    fn asset_text_refresh_worker_key(&self, space_id: &str) -> String {
        format!(
            "{}:{space_id}:operator={:p}",
            self.root_uri,
            Arc::as_ptr(self.operator.service()),
        )
    }

    fn ensure_authoritative_mutation_contract(&self) -> Result<()> {
        Authorizer::new(self.operator.clone()).ensure_authoritative_mutation_contract()
    }

    async fn ensure_mutation_admitted(&self, space_id: &str) -> Result<()> {
        self.ensure_authoritative_mutation_contract()?;
        crate::iceberg_store::ensure_mutation_admitted(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await
    }

    pub async fn create_space(&self, space_id: &str) -> Result<()> {
        self.ensure_authoritative_mutation_contract()?;
        validate_storage_id(validate_space_id(space_id))?;
        crate::iceberg_store::ensure_mutation_admitted(
            &self.operator,
            &format!("spaces/{space_id}"),
        )
        .await?;
        let creation_lock = SPACE_CREATION_SERIALIZER
            .get_or_init(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _creation_guard = creation_lock.lock().await;
        if self.recover_claimed_space(space_id).await?.is_some()
            || self.space_id_by_slug(space_id).await?.is_some()
        {
            return Err(AppError::conflict(
                ErrorCode::SpaceAlreadyExists,
                format!("Space slug already exists: {space_id}"),
            )
            .into());
        }
        let claim = self.claim_space_slug(space_id, space_id).await?;
        let lease = self.start_space_slug_claim_heartbeat(&claim);
        space::create_space(&self.operator, space_id, &self.root_uri).await?;
        lease.ensure_held()?;
        self.commit_space_slug_claim(space_id, space_id, claim.claim_id)
            .await?;
        lease.finish().await
    }

    /// Creates an operator-local Space with an immutable UUIDv7 directory and
    /// no application principal. A node must explicitly claim it before remote use.
    ///
    /// This is the strict primitive: an existing slug always fails with
    /// `SPACE_ALREADY_EXISTS`, even if it is the same idempotent retry. Use
    /// [`Self::ensure_operator_space`] for operator-local create-or-return.
    pub async fn create_operator_space(&self, slug: &str) -> Result<Uuid> {
        self.ensure_authoritative_mutation_contract()?;
        validate_storage_id(validate_space_id(slug))?;
        crate::iceberg_store::ensure_mutation_admitted(&self.operator, &format!("spaces/{slug}"))
            .await?;
        let creation_lock = SPACE_CREATION_SERIALIZER
            .get_or_init(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _creation_guard = creation_lock.lock().await;
        if self.recover_claimed_space(slug).await?.is_some()
            || self.space_id_by_slug(slug).await?.is_some()
        {
            return Err(AppError::conflict(
                ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                format!("Space slug already exists: {slug}"),
            )
            .into());
        }
        self.create_new_operator_space(slug).await
    }

    /// Operator-local idempotent create-or-return.
    ///
    /// The same create intent retried with the same slug converges to the same
    /// durable Space identity: first call returns `Created(id)`, retries
    /// against the same fully-committed operator-local Space return
    /// `Existing(same_id)`.
    ///
    /// Fail-closed: a live pending claim, broken bootstrap, slug/immutable-ID
    /// mismatch, or any account-owned claim never becomes "existing" and
    /// keeps failing with `SPACE_ALREADY_EXISTS` (or the underlying
    /// validation error). No fictitious local account model is introduced;
    /// only the durable outcome is shared with server account-bound retry.
    pub async fn ensure_operator_space(&self, slug: &str) -> Result<SpaceCreateOutcome> {
        self.ensure_operator_space_with_name(slug, slug).await
    }

    /// Operator-local idempotent create-or-return with an independent display
    /// name. The positional slug stays the stable filesystem/lookup key while
    /// `display_name` only seeds `meta.json:name` on first creation; retries
    /// converge to the existing durable identity without renaming.
    pub async fn ensure_operator_space_with_name(
        &self,
        slug: &str,
        display_name: &str,
    ) -> Result<SpaceCreateOutcome> {
        // Independent service-side validation: the shared domain rule trims
        // and rejects empty/whitespace-only names before any write, so direct
        // service callers fail the same way the CLI does.
        let display_name = ugoite_domain::space::normalize_space_display_name(display_name)
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
        self.ensure_authoritative_mutation_contract()?;
        validate_storage_id(validate_space_id(slug))?;
        crate::iceberg_store::ensure_mutation_admitted(&self.operator, &format!("spaces/{slug}"))
            .await?;
        let creation_lock = SPACE_CREATION_SERIALIZER
            .get_or_init(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _creation_guard = creation_lock.lock().await;
        // Fail-closed pre-check before recover can mutate anything: an
        // account-owned claim never becomes operator-local, and a committed
        // claim whose metadata slug disagrees stays a conflict instead of
        // being released + recreated under a new identity.
        if let Some(claim) = self.read_space_slug_claim(slug).await? {
            if claim.owner_principal_id.is_some() {
                return Err(AppError::conflict(
                    ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                    format!("Space slug already exists: {slug}"),
                )
                .into());
            }
            if claim.state == "committed" {
                if let Ok(metadata) = space::get_space_raw(&self.operator, &claim.space_id).await {
                    if metadata.get("slug").and_then(Value::as_str) != Some(slug) {
                        return Err(AppError::conflict(
                            ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                            format!("Space slug already exists: {slug}"),
                        )
                        .into());
                    }
                }
            }
        }
        if let Some(space_id) = self.recover_claimed_space(slug).await? {
            let existing = self
                .existing_operator_space_id(slug, &space_id)
                .await?
                .ok_or_else(|| {
                    AppError::conflict(
                        ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                        format!("Space slug already exists: {slug}"),
                    )
                })?;
            return Ok(SpaceCreateOutcome::Existing(existing));
        }
        if let Some(space_id) = self.space_id_by_slug(slug).await? {
            // No recoverable claim, but metadata already carries this slug.
            // Only a committed owner-less claim for the same identity may
            // converge to Existing; anything else (absent/released/pending
            // claim, owner, or ID mismatch) stays fail-closed so a cleaned
            // or foreign claim can never resurrect as operator-local.
            let claim = self.read_space_slug_claim(slug).await?;
            let committed_match = matches!(&claim,
                Some(claim) if claim.state == "committed"
                    && claim.owner_principal_id.is_none()
                    && claim.space_id == space_id);
            if !committed_match {
                return Err(AppError::conflict(
                    ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                    format!("Space slug already exists: {slug}"),
                )
                .into());
            }
            let existing = self
                .existing_operator_space_id(slug, &space_id)
                .await?
                .ok_or_else(|| {
                    AppError::conflict(
                        ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                        format!("Space slug already exists: {slug}"),
                    )
                })?;
            return Ok(SpaceCreateOutcome::Existing(existing));
        }
        Ok(SpaceCreateOutcome::Created(
            self.create_new_operator_space_with_name(slug, &display_name)
                .await?,
        ))
    }

    /// Validates that `space_id` is the fully-committed operator-local Space
    /// for `slug`. Returns `None` for any state that must stay fail-closed
    /// (non-UUID legacy IDs, slug mismatch, incomplete bootstrap, or
    /// account-owned claims) so callers keep returning a conflict.
    async fn existing_operator_space_id(&self, slug: &str, space_id: &str) -> Result<Option<Uuid>> {
        let Ok(space_uid) = Uuid::parse_str(space_id) else {
            return Ok(None);
        };
        if space_uid.get_version() != Some(uuid::Version::SortRand) {
            return Ok(None);
        }
        if let Some(claim) = self.read_space_slug_claim(slug).await? {
            if claim.owner_principal_id.is_some() {
                return Ok(None);
            }
            if claim.space_id != space_id && claim.state != "released" {
                return Ok(None);
            }
        }
        let metadata = space::get_space_raw(&self.operator, space_id).await?;
        if metadata.get("slug").and_then(Value::as_str) != Some(slug) {
            return Ok(None);
        }
        space::validate_complete_bootstrap(&self.operator, space_id).await?;
        Ok(Some(space_uid))
    }

    async fn create_new_operator_space(&self, slug: &str) -> Result<Uuid> {
        self.create_new_operator_space_with_name(slug, slug).await
    }

    async fn create_new_operator_space_with_name(
        &self,
        slug: &str,
        display_name: &str,
    ) -> Result<Uuid> {
        let space_id = Uuid::now_v7();
        let claim = self
            .claim_space_slug_with_owner_and_name(slug, &space_id.to_string(), None, display_name)
            .await?;
        let lease = self.start_space_slug_claim_heartbeat(&claim);
        space::create_space_with_identity_and_name(
            &self.operator,
            space_id,
            slug,
            display_name,
            &self.root_uri,
        )
        .await?;
        lease.ensure_held()?;
        self.commit_space_slug_claim(slug, &space_id.to_string(), claim.claim_id)
            .await?;
        lease.finish().await?;
        Ok(space_id)
    }

    /// Reserve a mutable slug with a storage-level conditional create before
    /// allocating the UUID directory. The process mutex only optimizes local
    /// callers; the claim is the cross-process/shared-backend uniqueness
    /// boundary. A claim is intentionally left behind if bootstrap crashes so
    /// an explicit creation-recovery path can resume the same immutable Space
    /// instead of reusing an ambiguous slug.
    async fn claim_space_slug(&self, slug: &str, space_id: &str) -> Result<SpaceSlugClaim> {
        self.claim_space_slug_with_owner_and_name(slug, space_id, None, slug)
            .await
    }

    async fn claim_space_slug_with_owner_and_name(
        &self,
        slug: &str,
        space_id: &str,
        owner: Option<(Uuid, String)>,
        space_name: &str,
    ) -> Result<SpaceSlugClaim> {
        self.operator.create_dir(SPACE_SLUG_CLAIMS_DIR).await?;
        let _claim_lock = acquire_local_space_slug_claim_lock(&self.operator, slug).await?;
        let claim_path = format!("{SPACE_SLUG_CLAIMS_DIR}{slug}.json");
        let now = Utc::now();
        let claim_record = SpaceSlugClaim {
            slug: slug.to_string(),
            space_name: space_name.to_string(),
            space_id: space_id.to_string(),
            state: "pending".to_string(),
            claim_id: Uuid::now_v7(),
            created_at: now.to_rfc3339(),
            heartbeat_at: now.to_rfc3339(),
            expires_at: (now + ChronoDuration::from_std(SPACE_SLUG_CLAIM_LEASE)?).to_rfc3339(),
            owner_principal_id: owner.as_ref().map(|(principal_id, _)| *principal_id),
            owner_display_name: owner.map(|(_, display_name)| display_name),
        };
        let claim = serde_json::to_vec(&claim_record)?;
        if let Some((existing, etag)) = self.read_space_slug_claim_exact(slug).await? {
            if existing.state != "released" {
                return Err(AppError::conflict(
                    ErrorCode::SpaceAlreadyExists,
                    format!("Space slug already exists: {slug}"),
                )
                .into());
            }
            self.write_space_slug_claim(&claim_path, claim, etag.as_deref())
                .await?;
        } else {
            OpendalStorage::from_operator(&self.operator)
                .write_if_absent(&claim_path, claim)
                .await
                .map_err(|error| {
                    if error.chain().any(|cause| {
                        cause.downcast_ref::<opendal::Error>().is_some_and(|error| {
                            matches!(
                                error.kind(),
                                opendal::ErrorKind::AlreadyExists
                                    | opendal::ErrorKind::ConditionNotMatch
                            )
                        }) || cause
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists)
                    }) {
                        return AppError::conflict(
                            ErrorCode::SpaceAlreadyExists,
                            format!("Space slug already exists: {slug}"),
                        )
                        .into();
                    }
                    error.context("claim Space slug with conditional storage create")
                })?;
        }
        Ok(claim_record)
    }

    async fn commit_space_slug_claim(
        &self,
        slug: &str,
        space_id: &str,
        claim_id: Uuid,
    ) -> Result<()> {
        let _claim_lock = acquire_local_space_slug_claim_lock(&self.operator, slug).await?;
        let Some((claim, etag)) = self.read_space_slug_claim_exact(slug).await? else {
            bail!("Space slug claim disappeared before bootstrap commit: {slug}");
        };
        if claim.state == "committed" {
            if claim.space_id == space_id && claim.claim_id == claim_id {
                return Ok(());
            }
            bail!("Space slug claim is owned by another Space: {slug}");
        }
        if claim.state != "pending"
            || claim.space_id != space_id
            || claim.claim_id != claim_id
            || claim.is_expired()?
        {
            bail!("Space slug claim is owned by another Space: {slug}");
        }
        let mut committed_claim = claim;
        committed_claim.state = "committed".to_string();
        let committed = serde_json::to_vec(&committed_claim)?;
        self.write_space_slug_claim(
            &format!("{SPACE_SLUG_CLAIMS_DIR}{slug}.json"),
            committed.clone(),
            etag.as_deref(),
        )
        .await?;
        // The claim JSON is the version-fenced commit record. The marker is a
        // derived discovery hint and may be rewritten after a released slug
        // is claimed again; it is never used to authorize the transition.
        let committed_path = format!("{SPACE_SLUG_CLAIMS_DIR}{slug}{SPACE_SLUG_COMMITTED_SUFFIX}");
        self.operator.write(&committed_path, committed).await?;
        Ok(())
    }

    async fn space_slug_claim_is_committed(
        &self,
        slug: &str,
        claim: &SpaceSlugClaim,
    ) -> Result<bool> {
        if claim.state == "committed" {
            return Ok(true);
        }
        let path = format!("{SPACE_SLUG_CLAIMS_DIR}{slug}{SPACE_SLUG_COMMITTED_SUFFIX}");
        let Some(bytes) = crate::read_object_exact_optional(&self.operator, &path).await? else {
            return Ok(false);
        };
        let marker: SpaceSlugClaim = serde_json::from_slice(&bytes)
            .map_err(|error| anyhow!("invalid Space slug commit marker for {slug}: {error}"))?;
        if marker.slug != slug
            || marker.space_id != claim.space_id
            || marker.claim_id != claim.claim_id
            || marker.state != "committed"
        {
            return Ok(false);
        }
        Ok(true)
    }

    async fn read_space_slug_claim(&self, slug: &str) -> Result<Option<SpaceSlugClaim>> {
        Ok(self
            .read_space_slug_claim_exact(slug)
            .await?
            .map(|(claim, _)| claim))
    }

    async fn read_space_slug_claim_exact(
        &self,
        slug: &str,
    ) -> Result<Option<(SpaceSlugClaim, Option<String>)>> {
        validate_storage_id(validate_space_id(slug))?;
        let path = format!("{SPACE_SLUG_CLAIMS_DIR}{slug}.json");
        let metadata = match self.operator.stat(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let etag = metadata
            .etag()
            .filter(|etag| !etag.is_empty())
            .map(str::to_owned);
        let bytes = match etag.as_deref() {
            Some(etag) => {
                self.operator
                    .read_options(
                        &path,
                        opendal::options::ReadOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await?
            }
            None if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") => {
                self.operator.read(&path).await?
            }
            None => bail!("shared Space slug claim read requires an exact ETag"),
        };
        let claim: SpaceSlugClaim = serde_json::from_slice(&bytes.to_vec())
            .map_err(|error| anyhow!("invalid Space slug claim for {slug}: {error}"))?;
        if claim.slug != slug {
            bail!("Space slug claim path and payload disagree for {slug}");
        }
        validate_storage_id(validate_space_id(&claim.space_id))?;
        if !matches!(claim.state.as_str(), "pending" | "committed" | "released") {
            bail!("invalid Space slug claim state for {slug}");
        }
        if claim.space_id != slug {
            if let Ok(space_uid) = Uuid::parse_str(&claim.space_id) {
                if space_uid.get_version() != Some(uuid::Version::SortRand) {
                    bail!("identity Space slug claim must use a UUIDv7: {slug}");
                }
            } else if !space::space_exists(&self.operator, &claim.space_id).await? {
                bail!("legacy Space slug claim points to a missing Space: {slug}");
            }
        }
        Ok(Some((claim, etag)))
    }

    async fn write_space_slug_claim(
        &self,
        path: &str,
        bytes: Vec<u8>,
        expected_etag: Option<&str>,
    ) -> Result<()> {
        if let Some(etag) = expected_etag {
            self.operator
                .write_options(
                    path,
                    bytes,
                    WriteOptions {
                        if_match: Some(etag.to_string()),
                        ..Default::default()
                    },
                )
                .await?;
        } else if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
            self.operator.write(path, bytes).await?;
        } else {
            bail!("shared Space slug claim write requires an exact ETag")
        }
        Ok(())
    }

    fn start_space_slug_claim_heartbeat(&self, claim: &SpaceSlugClaim) -> SpaceSlugClaimLease {
        let operator = self.operator.clone();
        let heartbeat_claim = claim.clone();
        let lost = Arc::new(AtomicBool::new(false));
        let heartbeat_lost = lost.clone();
        let heartbeat = tokio::spawn(async move {
            loop {
                tokio::time::sleep(SPACE_SLUG_CLAIM_HEARTBEAT).await;
                if renew_space_slug_claim(&operator, &heartbeat_claim)
                    .await
                    .is_err()
                {
                    heartbeat_lost.store(true, Ordering::Release);
                    return;
                }
            }
        });
        SpaceSlugClaimLease {
            lost,
            heartbeat: Some(heartbeat),
        }
    }

    async fn take_over_expired_space_slug_claim(
        &self,
        claim: &SpaceSlugClaim,
    ) -> Result<Option<SpaceSlugClaim>> {
        if !claim.is_expired()? {
            return Ok(None);
        }
        let _claim_lock = acquire_local_space_slug_claim_lock(&self.operator, &claim.slug).await?;
        let path = format!("{SPACE_SLUG_CLAIMS_DIR}{}.json", claim.slug);
        let metadata = self.operator.stat(&path).await?;
        let etag = metadata
            .etag()
            .filter(|etag| !etag.is_empty())
            .map(str::to_owned);
        if etag.is_none() && !matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
            bail!("shared expired Space slug claim takeover requires an exact ETag");
        }
        let current = self
            .operator
            .read_options(
                &path,
                opendal::options::ReadOptions {
                    if_match: etag.clone(),
                    ..Default::default()
                },
            )
            .await?;
        let current: SpaceSlugClaim = serde_json::from_slice(&current.to_vec())?;
        if current.state != "pending"
            || current.claim_id != claim.claim_id
            || !current.is_expired()?
        {
            return Ok(None);
        }
        let now = Utc::now();
        let mut replacement = current;
        replacement.claim_id = Uuid::now_v7();
        replacement.created_at = now.to_rfc3339();
        replacement.heartbeat_at = now.to_rfc3339();
        replacement.expires_at =
            (now + ChronoDuration::from_std(SPACE_SLUG_CLAIM_LEASE)?).to_rfc3339();
        if let Some(etag) = etag {
            self.operator
                .write_options(
                    &path,
                    serde_json::to_vec(&replacement)?,
                    WriteOptions {
                        if_match: Some(etag),
                        ..Default::default()
                    },
                )
                .await
                .map_err(anyhow::Error::from)?;
        } else if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
            self.operator
                .write(&path, serde_json::to_vec(&replacement)?)
                .await?;
        } else {
            bail!("shared expired Space slug claim takeover requires an exact ETag")
        }
        Ok(Some(replacement))
    }

    async fn release_space_slug_claim(
        &self,
        slug: &str,
        space_id: &str,
        claim_id: Option<Uuid>,
    ) -> Result<()> {
        let _claim_lock = acquire_local_space_slug_claim_lock(&self.operator, slug).await?;
        let path = format!("{SPACE_SLUG_CLAIMS_DIR}{slug}.json");
        let Some((claim, etag)) = self.read_space_slug_claim_exact(slug).await? else {
            return Ok(());
        };
        if claim.space_id == space_id && claim_id.is_none_or(|expected| expected == claim.claim_id)
        {
            let committed_path =
                format!("{SPACE_SLUG_CLAIMS_DIR}{slug}{SPACE_SLUG_COMMITTED_SUFFIX}");
            match self.operator.delete(&committed_path).await {
                Ok(()) => {}
                Err(error) if error.kind() == opendal::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            let mut released = claim;
            released.state = "released".to_string();
            if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
                // Local claim operations are protected by the inter-process
                // lock above. Removing the local tombstone keeps the
                // filesystem layout compact; shared backends retain the
                // version-fenced released record for the next conditional
                // claimant.
                match self.operator.delete(&path).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == opendal::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            } else {
                self.write_space_slug_claim(&path, serde_json::to_vec(&released)?, etag.as_deref())
                    .await?;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    async fn expire_space_slug_claim_for_test(&self, slug: &str) -> Result<()> {
        let path = format!("{SPACE_SLUG_CLAIMS_DIR}{slug}.json");
        let Some(mut claim) = self.read_space_slug_claim(slug).await? else {
            bail!("test claim does not exist: {slug}");
        };
        claim.expires_at = (Utc::now() - ChronoDuration::seconds(1)).to_rfc3339();
        self.operator
            .write(&path, serde_json::to_vec(&claim)?)
            .await?;
        Ok(())
    }

    async fn claimed_space_metadata_slug(&self, space_id: &str) -> Result<Option<String>> {
        let path = format!("spaces/{space_id}/meta.json");
        let Some(bytes) = crate::read_object_exact_optional(&self.operator, &path).await? else {
            return Ok(None);
        };
        let metadata: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("decode Space metadata for claim recovery: {space_id}"))?;
        space::validate_current_space_metadata(space_id, &metadata)?;
        metadata
            .get("slug")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .context("Space metadata has no slug")
            .map(Some)
    }

    async fn ensure_claimed_space_owner(&self, claim: &SpaceSlugClaim) -> Result<()> {
        let (Some(principal_id), Some(display_name)) = (
            claim.owner_principal_id,
            claim.owner_display_name.as_deref(),
        ) else {
            return Ok(());
        };
        let space_uid = Uuid::parse_str(&claim.space_id)
            .context("principal-backed Space slug claim does not use a UUID directory")?;
        let authorizer = Authorizer::new(self.operator.clone());
        // Apply the same legacy-layout rejection before either validating or
        // creating authorization state. Recovery must not upgrade an
        // ambiguous pre-current ownership layout merely because the claim is
        // otherwise expired and structurally repairable.
        authorizer
            .validate_current_layout(&claim.space_id, space_uid)
            .await?;
        let state_path = format!("spaces/{}/security/principals.json", claim.space_id);
        if self.operator.exists(&state_path).await? {
            Ok(())
        } else {
            self.ensure_authoritative_mutation_contract()?;
            authorizer
                .initialize_owner(&claim.space_id, space_uid, principal_id, display_name)
                .await
        }
    }

    /// Repairs a claim-backed Space before ordinary slug lookup. A claim is a
    /// durable recovery pointer, not a reason to permanently reserve a slug:
    /// complete metadata with a different current slug releases an interrupted
    /// rename claim, while incomplete bootstrap is resumed under its original
    /// immutable UUID.
    async fn recover_claimed_space(&self, slug: &str) -> Result<Option<String>> {
        let Some(claim) = self.read_space_slug_claim(slug).await? else {
            return Ok(None);
        };
        self.ensure_mutation_admitted(&claim.space_id).await?;
        let committed = self.space_slug_claim_is_committed(slug, &claim).await?;
        if committed {
            // A committed Space becoming incomplete is corruption, not an
            // interrupted create. Never turn backend/validation errors into a
            // repair decision after the durable commit marker exists.
            space::validate_complete_bootstrap(&self.operator, &claim.space_id).await?;
            self.ensure_claimed_space_owner(&claim).await?;
            let metadata = space::get_space_raw(&self.operator, &claim.space_id).await?;
            if metadata.get("slug").and_then(Value::as_str) == Some(slug) {
                return Ok(Some(claim.space_id));
            }
            self.ensure_authoritative_mutation_contract()?;
            self.release_space_slug_claim(slug, &claim.space_id, Some(claim.claim_id))
                .await?;
            return Ok(None);
        }

        // Pending claims are leases.  Never release or repair a live claim:
        // the original writer may still be between its target claim and the
        // authoritative metadata swap.  Takeover is an ETag-guarded state
        // transition after expiry, so a concurrent heartbeat or recovery
        // winner cannot be silently displaced.
        if claim.is_expired()? {
            self.ensure_authoritative_mutation_contract()?;
        }
        let Some(claim) = self.take_over_expired_space_slug_claim(&claim).await? else {
            return Ok(None);
        };
        let lease = self.start_space_slug_claim_heartbeat(&claim);
        let recovered = async {
            if let Some(metadata_slug) = self.claimed_space_metadata_slug(&claim.space_id).await? {
                if metadata_slug != slug {
                    self.release_space_slug_claim(slug, &claim.space_id, Some(claim.claim_id))
                        .await?;
                    return Ok(None);
                }
            }

            if claim.space_id != claim.slug {
                if let Ok(space_uid) = Uuid::parse_str(&claim.space_id) {
                    space::repair_space_with_identity(
                        &self.operator,
                        space_uid,
                        slug,
                        &claim.space_name,
                        &self.root_uri,
                    )
                    .await?;
                } else {
                    space::repair_space(&self.operator, &claim.space_id, slug, &self.root_uri)
                        .await?;
                }
            } else if space::space_exists(&self.operator, &claim.space_id).await? {
                space::repair_space(&self.operator, &claim.space_id, slug, &self.root_uri).await?;
            } else {
                // A legacy create can crash after claiming but before writing
                // meta.json. No authoritative object exists in that case, so
                // the expired claim can be safely released by its new owner.
                self.release_space_slug_claim(slug, &claim.space_id, Some(claim.claim_id))
                    .await?;
                return Ok(None);
            }
            space::validate_complete_bootstrap(&self.operator, &claim.space_id).await?;
            self.ensure_claimed_space_owner(&claim).await?;
            lease.ensure_held()?;
            self.commit_space_slug_claim(slug, &claim.space_id, claim.claim_id)
                .await?;
            Ok(Some(claim.space_id.clone()))
        }
        .await;
        let finish = lease.finish().await;
        match (recovered, finish) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(finish_error)) => {
                Err(error.context(format!("finish Space slug claim lease: {finish_error:#}")))
            }
        }
    }

    pub async fn create_space_for_principal(
        &self,
        slug: &str,
        principal_id: Uuid,
        display_name: &str,
    ) -> Result<Uuid> {
        self.create_space_for_principal_with_name(slug, principal_id, display_name, slug)
            .await
    }

    pub async fn create_space_for_principal_with_name(
        &self,
        slug: &str,
        principal_id: Uuid,
        owner_display_name: &str,
        space_name: &str,
    ) -> Result<Uuid> {
        let creation_lock = SPACE_CREATION_SERIALIZER
            .get_or_init(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _creation_guard = creation_lock.lock().await;
        // Space bootstrap spans the Space scaffold, the authorization owner,
        // and the Node binding performed by the server.  There is no atomic
        // multi-object fence for shared backends, so fail before the first
        // write instead of leaving a partially bootstrapped Space that cannot
        // be retried under the same slug.
        Authorizer::new(self.operator.clone()).ensure_authoritative_mutation_contract()?;
        validate_storage_id(validate_space_id(slug))?;
        crate::iceberg_store::ensure_mutation_admitted(&self.operator, &format!("spaces/{slug}"))
            .await?;
        if self.recover_claimed_space(slug).await?.is_some()
            || self.space_id_by_slug(slug).await?.is_some()
        {
            return Err(AppError::conflict(
                ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                format!("Space slug already exists: {slug}"),
            )
            .into());
        }
        let space_uid = Uuid::now_v7();
        let space_id = space_uid.to_string();
        let claim = self
            .claim_space_slug_with_owner_and_name(
                slug,
                &space_id,
                Some((principal_id, owner_display_name.to_string())),
                space_name,
            )
            .await?;
        let lease = self.start_space_slug_claim_heartbeat(&claim);
        space::create_space_with_identity_and_name(
            &self.operator,
            space_uid,
            slug,
            space_name,
            &self.root_uri,
        )
        .await?;
        Authorizer::new(self.operator.clone())
            .initialize_owner(&space_id, space_uid, principal_id, owner_display_name)
            .await?;
        lease.ensure_held()?;
        self.commit_space_slug_claim(slug, &space_id, claim.claim_id)
            .await?;
        lease.finish().await?;
        Ok(space_uid)
    }

    pub async fn space_uid(&self, space_id: &str) -> Result<Uuid> {
        let raw = self.get_space(space_id).await?;
        let space_uid = raw
            .get("space_uid")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Space is missing immutable space_uid"))
            .and_then(|value| Uuid::parse_str(value).map_err(anyhow::Error::from))?;
        for candidate_id in self.list_space_ids().await? {
            if candidate_id == space_id {
                continue;
            }
            let candidate = self.get_space(&candidate_id).await?;
            let candidate_uid = candidate
                .get("space_uid")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Space is missing immutable space_uid"))
                .and_then(|value| Uuid::parse_str(value).map_err(anyhow::Error::from))?;
            if candidate_uid == space_uid {
                bail!(
                    "duplicate immutable space_uid {space_uid} is used by Spaces {space_id} and {candidate_id}"
                );
            }
        }
        Ok(space_uid)
    }

    pub async fn list_space_ids(&self) -> Result<Vec<String>> {
        self.list_space_ids_inner()
            .await
            .map_err(|error| space::space_discovery_failure("<spaces>", "service_listing", error))
    }

    async fn list_space_ids_inner(&self) -> Result<Vec<String>> {
        let live_pending_space_ids = self.live_pending_claim_space_ids().await?;
        let discovered_space_ids = space::list_spaces_discovery(&self.operator).await?;
        let mut space_ids = Vec::with_capacity(discovered_space_ids.len());
        for space_id in discovered_space_ids {
            if live_pending_space_ids.contains(&space_id) {
                continue;
            }
            space::validate_discoverable_space(&self.operator, &space_id).await?;
            space_ids.push(space_id);
        }
        Ok(space_ids)
    }

    /// Lists complete Spaces for already-resolved Node principals. `None`
    /// denotes an explicit identity-level visibility denial; every other
    /// failure remains a typed discovery diagnostic. Keeping this decision in
    /// the application service prevents the HTTP adapter from treating
    /// corruption or storage failures as invisible Spaces.
    ///
    /// The caller supplies a validated inventory from a single
    /// `list_space_ids` call in this operation. This method never re-runs
    /// discovery/validation itself, so one listing operation performs exactly
    /// one full Space-tree scan. No cross-request cache is used: the next
    /// request always reads fresh durable state.
    pub async fn list_spaces_authorized_for_principals(
        &self,
        principal_ids_by_space: &BTreeMap<String, Option<Vec<Uuid>>>,
    ) -> Result<Vec<Value>> {
        let space_ids = self.list_space_ids().await?;
        self.list_spaces_authorized_for_inventory(space_ids, principal_ids_by_space)
            .await
    }

    pub async fn list_spaces_authorized_for_inventory(
        &self,
        space_ids: Vec<String>,
        principal_ids_by_space: &BTreeMap<String, Option<Vec<Uuid>>>,
    ) -> Result<Vec<Value>> {
        let mut spaces = Vec::with_capacity(space_ids.len());
        for space_id in space_ids {
            let principal_ids = principal_ids_by_space.get(&space_id).ok_or_else(|| {
                space::space_discovery_failure(
                    &space_id,
                    "missing_principal_scope",
                    anyhow!("Space listing did not provide a principal scope"),
                )
            })?;
            let Some(principal_ids) = principal_ids else {
                continue;
            };
            if principal_ids.is_empty() {
                return Err(space::space_discovery_failure(
                    &space_id,
                    "empty_principal_scope",
                    anyhow!("Space listing principal scope is empty"),
                ));
            }

            let id_for_read = space_id.clone();
            let principals = principal_ids.clone();
            let result = Authorizer::new(self.operator.clone())
                .with_state_lock(&space_id, |state| async move {
                    for principal_id in &principals {
                        if !effective_actions_for_state(&state, *principal_id, None)?
                            .contains(&Action::Read)
                        {
                            return Err(AppError::forbidden(
                                "principal is not authorized for the requested read",
                            )
                            .into());
                        }
                    }
                    self.get_space(&id_for_read).await
                })
                .await;
            match result {
                Ok(space) => spaces.push(space),
                Err(error) if is_explicit_authorization_denial(&error) => continue,
                Err(error) => {
                    return Err(space::space_discovery_failure(
                        &space_id,
                        "authorized_space_read",
                        error,
                    ));
                }
            }
        }
        Ok(spaces)
    }

    async fn list_space_slug_claims(&self) -> Result<Vec<(String, SpaceSlugClaim)>> {
        let mut lister = match self.operator.lister(SPACE_SLUG_CLAIMS_DIR).await {
            Ok(lister) => lister,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut claims = Vec::new();
        while let Some(entry) = lister.try_next().await? {
            if entry.metadata().mode() != EntryMode::FILE {
                continue;
            }
            let Some(slug) = entry
                .path()
                .strip_prefix(SPACE_SLUG_CLAIMS_DIR)
                .and_then(|name| name.strip_suffix(".json"))
            else {
                continue;
            };
            if let Some(claim) = self.read_space_slug_claim(slug).await? {
                claims.push((slug.to_string(), claim));
            }
        }
        Ok(claims)
    }

    async fn live_pending_claim_space_ids(&self) -> Result<BTreeSet<String>> {
        let mut space_ids = BTreeSet::new();
        for (slug, claim) in self.list_space_slug_claims().await? {
            if claim.state == "pending"
                && !claim.is_expired()?
                && !self.space_slug_claim_is_committed(&slug, &claim).await?
            {
                // Only a claim whose target metadata still names the claim
                // slug may hide a crash-left incomplete bootstrap. A rename
                // claim before metadata publication and a mismatched claim
                // must not suppress an otherwise valid Space or corruption.
                if self
                    .claimed_space_metadata_slug(&claim.space_id)
                    .await?
                    .as_deref()
                    != Some(slug.as_str())
                {
                    continue;
                }
                match space::validate_complete_bootstrap(&self.operator, &claim.space_id).await {
                    Ok(()) => {}
                    Err(error) if error.to_string().contains("incomplete Space bootstrap:") => {
                        space_ids.insert(claim.space_id);
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(space_ids)
    }

    /// Reconcile expired create/rename claims before strict Space enumeration.
    /// A pending claim is the durable recovery pointer for a crash-left
    /// bootstrap; live claims remain untouched, while committed claims are
    /// deliberately left to strict validation so corruption cannot be hidden.
    pub async fn recover_pending_space_claims(&self) -> Result<()> {
        for (slug, claim) in self.list_space_slug_claims().await? {
            if claim.state == "pending" {
                self.ensure_mutation_admitted(&claim.space_id).await?;
                let _ = self.recover_claimed_space(&slug).await?;
            }
        }
        Ok(())
    }

    pub async fn space_id_by_slug(&self, slug: &str) -> Result<Option<String>> {
        for space_id in self.list_space_ids().await? {
            let meta = self.get_space(&space_id).await?;
            if meta.get("slug").and_then(Value::as_str) == Some(slug) {
                return Ok(Some(space_id));
            }
        }
        Ok(None)
    }

    /// Explicitly repairs a claim-backed interrupted creation, then performs
    /// the normal complete-Space lookup. Ordinary reads remain side-effect
    /// free and therefore do not silently finish a crash-left bootstrap.
    pub async fn recover_space_id_by_slug(&self, slug: &str) -> Result<Option<String>> {
        if let Some(space_id) = self.recover_claimed_space(slug).await? {
            return Ok(Some(space_id));
        }
        self.space_id_by_slug(slug).await
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        space::get_space_raw(&self.operator, space_id).await
    }

    pub async fn list_pins(&self, space_id: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        Ok(serde_json::to_value(workspace.list_pins().await?)?)
    }

    pub async fn list_changes(&self, space_id: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        Ok(serde_json::to_value(workspace.list_changes().await?)?)
    }

    /// Reopen hook: converge commit-coupled audit evidence after a Space is
    /// (re)opened in this process.
    ///
    /// Crash windows between a Knowledge commit and its audit append leave
    /// committed revisions without evidence. The server startup hook covers
    /// restarts; this covers core-mode reopen. Failures propagate
    /// fail-closed instead of hiding as success. Reads
    /// ([`Self::list_space_audit`]) stay light: they report committed
    /// evidence only and never repair.
    pub async fn open_space(&self, space_id: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        let converged = self.reconcile_space_audit(space_id).await?;
        Ok(json!({
            "space_id": space_id,
            "audit_targets_converged": converged,
        }))
    }

    /// Light audit read: reports committed evidence only, never repairs.
    ///
    /// Crash-missing evidence heals via the server startup hook or
    /// [`Self::open_space`], plus the bounded pre-read converge on the
    /// server audit route for same-process commit→delivery gaps.
    pub async fn list_space_audit(
        &self,
        space_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        crate::audit::list_audit_events(
            &self.operator,
            space_id,
            crate::audit::AuditListOptions {
                offset,
                limit,
                action: None,
                actor_principal_id: None,
                outcome: None,
            },
        )
        .await
    }

    pub async fn revert_change(
        &self,
        space_id: &str,
        target_change_id: &str,
        actor_principal_id: &str,
        run_id: Option<&str>,
        message: Option<&str>,
    ) -> Result<Value> {
        if target_change_id.trim().is_empty() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "target_change_id must not be blank",
            )
            .into());
        }
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        let workspace = iceberg_store::native_mutation_workspace(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        let command = ChangeCommand {
            change_id: Uuid::new_v4().to_string(),
            run_id: run_id.map(RunId::new).transpose().map_err(|error| {
                AppError::invalid_input(ErrorCode::InvalidInput, error.to_string())
            })?,
            actor_principal_id: actor_principal_id.to_owned(),
            message: message.map(str::to_owned),
            reverts_change_id: Some(target_change_id.to_owned()),
            created_at_micros: Utc::now().timestamp_micros(),
        };
        let receipt = workspace.revert_change(target_change_id, &command).await?;
        Ok(json!({
            "change_id": receipt.command_id,
            "reverts_change_id": target_change_id,
            "catalog_generation": receipt.catalog_generation,
            "revision_ids": receipt.committed_revision_ids,
            "run_id": command.run_id,
        }))
    }

    /// Undo every Change correlated to a Run in reverse publication order.
    /// Each inverse is its own append-only Change; the Run itself has no
    /// durable status record and can be resumed by repeating this request.
    pub async fn undo_run(
        &self,
        space_id: &str,
        run_id: &str,
        actor_principal_id: &str,
    ) -> Result<Value> {
        let run_id = RunId::new(run_id)
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let changes = workspace.list_changes().await?;
        let already_reverted = changes
            .iter()
            .filter_map(|change| change.change.reverts_change_id.clone())
            .collect::<std::collections::HashSet<_>>();
        let mut changes = changes
            .into_iter()
            .filter(|change| {
                change.change.run_id.as_ref() == Some(&run_id)
                    && change.change.reverts_change_id.is_none()
                    && !already_reverted.contains(change.change_id.as_str())
            })
            .collect::<Vec<_>>();
        changes.sort_by(|left, right| right.generation.cmp(&left.generation));
        let mut inverses = Vec::with_capacity(changes.len());
        for change in changes {
            inverses.push(
                self.revert_change(
                    space_id,
                    &change.change_id,
                    actor_principal_id,
                    Some(run_id.as_str()),
                    Some("Undo Run"),
                )
                .await?,
            );
        }
        Ok(json!({
            "run_id": run_id,
            "reverted_change_count": inverses.len(),
            "inverses": inverses,
        }))
    }

    /// Apply a portable batch using the same entry mutation use cases as the
    /// individual REST operations. Each operation remains an append-only
    /// Change; cross-Form atomicity is deliberately not promised in v1.
    ///
    /// Batch safety: `operations` is fully pre-validated (non-empty, entry
    /// IDs, version tokens, destructive-mix) before the first write, so a
    /// rejected batch leaves no partial mutation. Semantic failures
    /// mid-batch (stale version token, missing target) still leave the
    /// committed prefix by design: there is no cross-operation rollback.
    ///
    /// Authorization: Create/Update arms run through the authorized
    /// principal-gated use cases, so `principal_ids` must be non-empty.
    /// The Remove arm threads the same `principal_ids` to audit delivery
    /// but performs no service-level authorization itself: a batch Remove
    /// always arrives adapter-authorized (the dangerous-action approval
    /// bound to the exact Entry, or the resource-scoped Delete wrapper),
    /// and it may execute under an already-held human-approval lease that
    /// must not be re-acquired (the process lease is not reentrant).
    /// There is intentionally no operator-local (core CLI) batch path: a
    /// thin core batch cannot reuse this shared op without inventing a
    /// parallel mutation implementation, which the architecture forbids.
    /// `run_id` is grouping metadata only, not an idempotency key:
    /// repeating a batch appends new Changes under the same Run, and the
    /// idempotent path for a repeated Run is [`Self::undo_run`], which
    /// resumes (second call reverts zero Changes).
    pub async fn apply_operations(
        &self,
        space_id: &str,
        operations: Vec<ApplyOperation>,
        actor_principal_id: &str,
        principal_ids: &[Uuid],
        run_id: Option<&str>,
        _message: Option<&str>,
    ) -> Result<Value> {
        if operations.is_empty() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "operations must not be empty",
            )
            .into());
        }
        // Mirror the server destructive-mix guard pre-persistence: a Remove
        // must be submitted alone so its human approval stays bound to the
        // exact Entry. Rejecting here (like the server 422 INVALID_INPUT)
        // leaves no partial mutation.
        if operations.len() > 1
            && operations
                .iter()
                .any(|operation| matches!(operation, ApplyOperation::Remove { .. }))
        {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "apply remove must be submitted as one operation so its human approval is bound to the exact Entry",
            )
            .into());
        }
        // Pre-validate every operation's identifiers before the first write.
        for operation in &operations {
            match operation {
                ApplyOperation::Create { id, .. } => {
                    if let Some(id) = id {
                        validate_storage_id(validate_entry_id(id))?;
                    }
                }
                ApplyOperation::Update {
                    id, version_token, ..
                } => {
                    validate_storage_id(validate_entry_id(id))?;
                    validate_storage_id(validate_revision_id(version_token))?;
                }
                ApplyOperation::Remove { id } => {
                    validate_storage_id(validate_entry_id(id))?;
                }
            }
        }
        let run_id = run_id
            .map(RunId::new)
            .transpose()
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
        // Keep the minimal public approval response stable. Batch callers
        // that provide run metadata receive the durable mutation receipt
        // fields; the approval-only form remains the historical `{kind,id}`
        // response used to bind and confirm the exact public intent.
        let include_receipt = run_id.is_some() || _message.is_some();
        let mut results = Vec::with_capacity(operations.len());
        for operation in operations {
            match operation {
                ApplyOperation::Create {
                    id,
                    form,
                    tags,
                    fields,
                    extra_attributes,
                } => {
                    let entry_id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                    let change = ChangeCommand {
                        change_id: Uuid::new_v4().to_string(),
                        run_id: run_id.clone(),
                        actor_principal_id: actor_principal_id.to_owned(),
                        message: _message.map(str::to_owned),
                        reverts_change_id: None,
                        created_at_micros: Utc::now().timestamp_micros(),
                    };
                    let value = self
                        .create_structured_entry_authorized_for_principals_with_change(
                            space_id,
                            &entry_id,
                            form,
                            tags,
                            fields,
                            extra_attributes,
                            actor_principal_id,
                            principal_ids,
                            Some(change),
                        )
                        .await?;
                    results.push(json!({
                        "kind": "create",
                        "id": entry_id,
                        "revision_id": value["revision_id"],
                        "change_id": value["change_id"],
                    }));
                }
                ApplyOperation::Update {
                    id,
                    version_token,
                    form,
                    tags,
                    fields,
                    extra_attributes,
                } => {
                    let change = ChangeCommand {
                        change_id: Uuid::new_v4().to_string(),
                        run_id: run_id.clone(),
                        actor_principal_id: actor_principal_id.to_owned(),
                        message: _message.map(str::to_owned),
                        reverts_change_id: None,
                        created_at_micros: Utc::now().timestamp_micros(),
                    };
                    let value = self
                        .update_structured_entry_authorized_for_principals_with_change(
                            space_id,
                            &id,
                            form,
                            tags,
                            fields,
                            extra_attributes,
                            Some(version_token.as_str()),
                            actor_principal_id,
                            principal_ids,
                            Some(change),
                        )
                        .await?;
                    results.push(json!({
                        "kind": "update",
                        "id": id,
                        "revision_id": value["revision_id"],
                        "change_id": value["change_id"],
                    }));
                }
                ApplyOperation::Remove { id } => {
                    let change = ChangeCommand {
                        change_id: Uuid::new_v4().to_string(),
                        run_id: run_id.clone(),
                        actor_principal_id: actor_principal_id.to_owned(),
                        message: _message.map(str::to_owned),
                        reverts_change_id: None,
                        created_at_micros: Utc::now().timestamp_micros(),
                    };
                    // Adapter-authorized Remove: Delete on the exact Entry
                    // was established by the request adapter (dangerous-action
                    // approval or resource-scoped wrapper), which may hold the
                    // authorization lease across this call. Principals still
                    // thread to audit delivery below.
                    let value = self
                        .delete_entry_with_change_receipt_for_principals(
                            space_id,
                            &id,
                            actor_principal_id,
                            principal_ids,
                            Some(change),
                        )
                        .await?;
                    let mut result = json!({"kind": "remove", "id": id});
                    if include_receipt {
                        if let Some(revision_id) = value.get("revision_id") {
                            result["revision_id"] = revision_id.clone();
                        }
                        if let Some(change_id) = value.get("change_id") {
                            result["change_id"] = change_id.clone();
                        }
                    }
                    results.push(result);
                }
            }
        }
        Ok(json!({
            "run_id": run_id,
            "operations": results,
        }))
    }

    pub async fn create_pin(
        &self,
        space_id: &str,
        name: &str,
        created_by_principal_id: &str,
        command_id: &str,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        let workspace = iceberg_store::native_mutation_workspace(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        Ok(serde_json::to_value(
            workspace
                .create_pin(
                    name,
                    created_by_principal_id,
                    Utc::now().timestamp_micros(),
                    command_id,
                )
                .await?,
        )?)
    }

    pub async fn delete_pin(&self, space_id: &str, name: &str, command_id: &str) -> Result<()> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        let workspace = iceberg_store::native_mutation_workspace(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await?;
        workspace.delete_pin(name, command_id).await
    }

    /// Returns read-only Catalog Head and Iceberg metadata evidence for one
    /// Space. Checkpoint names are caller-supplied because listing storage is
    /// not a source of Catalog or orphan authority.
    pub async fn space_health(&self, space_id: &str, checkpoint_names: &[String]) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        Ok(serde_json::to_value(
            workspace.health_report(checkpoint_names).await?,
        )?)
    }

    pub async fn patch_space(&self, space_id: &str, patch: &Value) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_public_space_patch(patch)?;
        let current = space::get_space_raw(&self.operator, space_id).await?;
        let current_slug = current
            .get("slug")
            .and_then(Value::as_str)
            .context("Space metadata has no slug")?;
        let next_slug = patch.get("slug").and_then(Value::as_str);
        if let Some(next_slug) = next_slug {
            validate_storage_id(validate_space_id(next_slug))?;
            if next_slug != current_slug {
                if self.recover_space_id_by_slug(next_slug).await?.is_some() {
                    return Err(AppError::conflict(
                        ErrorCode::SpaceAlreadyExists,
                        format!("Space slug already exists: {next_slug}"),
                    )
                    .into());
                }
                let claim = self.claim_space_slug(next_slug, space_id).await?;
                let lease = self.start_space_slug_claim_heartbeat(&claim);
                lease.ensure_held()?;
                let result =
                    space::patch_space_if_slug(&self.operator, space_id, patch, current_slug)
                        .await?;
                lease.ensure_held()?;
                self.commit_space_slug_claim(next_slug, space_id, claim.claim_id)
                    .await?;
                lease.finish().await?;
                // A failed process between metadata publication and this
                // cleanup is repaired by recover_claimed_space on the old
                // slug; releasing it here keeps normal rename semantics.
                self.release_space_slug_claim(current_slug, space_id, None)
                    .await?;
                return Ok(result);
            }
        }
        space::patch_space_if_slug(&self.operator, space_id, patch, current_slug).await
    }

    pub async fn ensure_space(&self, space_id: &str) -> Result<()> {
        self.validate_complete_space(space_id).await
    }

    pub async fn list_forms(&self, space_id: &str) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        form::list_forms(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn list_forms_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Vec<Value>> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let readable_forms = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                Ok(
                    form::list_forms(&self.operator, &self.workspace_path(space_id))
                        .await?
                        .into_iter()
                        .filter(|value| {
                            value
                                .get("name")
                                .and_then(Value::as_str)
                                .is_some_and(|name| {
                                    readable_forms.contains_key(&name.to_ascii_lowercase())
                                })
                        })
                        .collect(),
                )
            })
            .await
    }

    pub async fn get_form(&self, space_id: &str, form_name: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_form_name(form_name))?;
        form::get_form(&self.operator, &self.workspace_path(space_id), form_name).await
    }

    pub async fn get_form_authorized_for_principals(
        &self,
        space_id: &str,
        form_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_form_name(form_name))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                for principal_id in principal_ids {
                    let actions = effective_actions_for_state(
                        &state,
                        *principal_id,
                        Some(&ResourceRef {
                            kind: ResourceKind::Form,
                            id: form_name.to_string(),
                            parent: None,
                        }),
                    )?;
                    if !actions.contains(&Action::Read) {
                        return Err(AppError::not_found(
                            ErrorCode::FormNotFound,
                            format!("Form not found: {form_name}"),
                        )
                        .into());
                    }
                }
                form::get_form(&self.operator, &self.workspace_path(space_id), form_name).await
            })
            .await
    }

    pub async fn upsert_form(&self, space_id: &str, form_def: &Value) -> Result<()> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        form::upsert_form(&self.operator, &self.workspace_path(space_id), form_def).await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(())
    }

    /// Shared admission/auth prelude for structured Entry writes.
    ///
    async fn entry_authorized_write_prelude(
        &self,
        space_id: &str,
        entry_id: &str,
        action: Action,
        parent_revision_id: Option<&str>,
        principal_ids: &[Uuid],
    ) -> Result<EntryAuthorizedWritePrelude> {
        // (1) Space/request admission.
        require_nonempty_authorized_principals(principal_ids)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        // (2) Entry identity normalization.
        validate_storage_id(validate_entry_id(entry_id))?;
        // (3) Common parent revision checks (updates only; creates pass None).
        if let Some(parent_revision_id) = parent_revision_id {
            validate_storage_id(validate_revision_id(parent_revision_id))?;
        }
        // (4) Principal/scope authorization under one held lease.
        let (state, authorization_lease) = Authorizer::new(self.operator.clone())
            .acquire_state_lease(space_id)
            .await?;
        self.require_action_for_principals_in_state(
            &state,
            entry_id,
            ResourceKind::Entry,
            action,
            principal_ids,
        )?;
        let scopes = self
            .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
            .await?;
        // (5) Common mutation context creation.
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let workspace = self.workspace_path(space_id);
        Ok(EntryAuthorizedWritePrelude {
            scopes,
            integrity,
            workspace,
            _authorization_lease: authorization_lease,
        })
    }

    /// Create a Form-backed Entry without regenerating Markdown.
    ///
    /// Structured create converges on the shared draft path. `fields`
    /// is the complete initial field map; there is no metadata-only patch
    /// variant. Unknown keys are preserved as extra attributes only when
    /// the Form allows them, otherwise `UnknownFormFields` rejects
    /// pre-persistence. A key in both `fields` and `extra_attributes` is
    /// an `InvalidInput` diagnostic, never a silent precedence.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_structured_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        form_name: String,
        tags: Vec<String>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        self.create_structured_entry_authorized_for_principals_with_change(
            space_id,
            entry_id,
            form_name,
            tags,
            fields,
            extra_attributes,
            author,
            principal_ids,
            None,
        )
        .await
    }

    /// Create a Form-backed Entry carrying an existing mutation/Change
    /// context (Change ID propagation and Run grouping). This is the
    /// structured counterpart to
    /// the authorized mutation path:
    /// admission, authorization, and audit are identical, and the `change`
    /// is forwarded to the same shared entry boundary.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_structured_entry_authorized_for_principals_with_change(
        &self,
        space_id: &str,
        entry_id: &str,
        form_name: String,
        tags: Vec<String>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
        author: &str,
        principal_ids: &[Uuid],
        change: Option<ChangeCommand>,
    ) -> Result<Value> {
        let prelude = self
            .entry_authorized_write_prelude(space_id, entry_id, Action::Create, None, principal_ids)
            .await?;
        let (_, receipt) = entry::create_structured_entry_with_scopes_and_change_with_receipt(
            &self.operator,
            &prelude.workspace,
            entry_id,
            form_name,
            tags,
            fields,
            extra_attributes,
            author,
            &prelude.integrity,
            Some(&prelude.scopes),
            change,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        let mut result = entry::get_entry(&self.operator, &prelude.workspace, entry_id).await?;
        result["change_id"] = json!(receipt.command_id);
        self.record_committed_entry_revision(
            space_id,
            entry_id,
            crate::mutation_audit::ENTRY_CREATED_ACTION,
            principal_ids,
            author,
        )
        .await;
        Ok(result)
    }

    /// Create a Form-backed Entry in core (local filesystem) mode and return
    /// the durable commit receipt alongside the existing Entry
    /// representation. No CLI-side coercion happens here; the shared
    /// core normalization owns field semantics for both core and remote
    /// paths.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_structured_entry_with_receipt(
        &self,
        space_id: &str,
        entry_id: &str,
        form_name: String,
        tags: Vec<String>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
        author: &str,
    ) -> Result<(Value, crate::CommitReceipt)> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let workspace = self.workspace_path(space_id);
        let (_, receipt) = entry::create_structured_entry_with_scopes_and_change_with_receipt(
            &self.operator,
            &workspace,
            entry_id,
            form_name,
            tags,
            fields,
            extra_attributes,
            author,
            &integrity,
            None,
            None,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        let mut result = entry::get_entry(&self.operator, &workspace, entry_id).await?;
        result["change_id"] = json!(receipt.command_id);
        self.record_committed_entry_revision(
            space_id,
            entry_id,
            crate::mutation_audit::ENTRY_CREATED_ACTION,
            &[],
            author,
        )
        .await;
        Ok((result, receipt))
    }

    pub async fn list_entries(&self, space_id: &str) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        entry::list_entries(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn list_entries_page(
        &self,
        space_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        entry::list_entries(&self.operator, &self.workspace_path(space_id))
            .await
            .map(|entries| entries.into_iter().skip(offset).take(limit).collect())
    }

    pub async fn get_entry(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::get_entry(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn get_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                entry::get_entry_authorized(
                    &self.operator,
                    &self.workspace_path(space_id),
                    entry_id,
                    &scopes,
                )
                .await
            })
            .await
    }

    /// Update a Form-backed Entry in core (local filesystem) mode without
    /// regenerating Markdown. `fields` is the complete post-update field map;
    /// omitted fields are intentionally cleared. Callers do read → modify →
    /// full update; there is no metadata-only patch variant. `extra_attributes` is
    /// carried through unchanged because the CLI has no extra-attribute
    /// editing surface (an explicit field value replaces a preserved extra
    /// with the same key before calling). Form identity is immutable;
    /// mismatches surface the canonical InvalidInput error from the shared
    /// boundary. A key in both `fields` and `extra_attributes` is an
    /// `InvalidInput` diagnostic, never a silent precedence.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_structured_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        form_name: Option<String>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
        parent_revision_id: Option<&str>,
        author: &str,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        if let Some(parent_revision_id) = parent_revision_id {
            validate_storage_id(validate_revision_id(parent_revision_id))?;
        }
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let result = entry::update_structured_entry_authorized_with_change(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            form_name,
            None,
            fields,
            extra_attributes,
            parent_revision_id,
            author,
            &integrity,
            None,
            None,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        self.record_committed_entry_revision(
            space_id,
            entry_id,
            crate::mutation_audit::ENTRY_UPDATED_ACTION,
            &[],
            author,
        )
        .await;
        Ok(result)
    }

    /// Update a Form-backed Entry without regenerating Markdown. `fields`
    /// is the complete post-update field map (full replacement: omitted
    /// fields clear, never patch); tags fall back to stored values when
    /// `None`. Form identity is immutable. A key in both `fields` and
    /// `extra_attributes` is an `InvalidInput` diagnostic, never a silent
    /// precedence.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_structured_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        form_name: Option<String>,
        tags: Option<Vec<String>>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
        parent_revision_id: Option<&str>,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        self.update_structured_entry_authorized_for_principals_with_change(
            space_id,
            entry_id,
            form_name,
            tags,
            fields,
            extra_attributes,
            parent_revision_id,
            author,
            principal_ids,
            None,
        )
        .await
    }

    /// Update a Form-backed Entry carrying an existing mutation/Change
    /// context (Change ID propagation, revision parentage, Run grouping).
    /// Admission, authorization, and audit are identical to ordinary
    /// structured updates, and the `change` is forwarded to the same shared
    /// entry boundary.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_structured_entry_authorized_for_principals_with_change(
        &self,
        space_id: &str,
        entry_id: &str,
        form_name: Option<String>,
        tags: Option<Vec<String>>,
        fields: std::collections::BTreeMap<String, Value>,
        extra_attributes: std::collections::BTreeMap<String, Value>,
        parent_revision_id: Option<&str>,
        author: &str,
        principal_ids: &[Uuid],
        change: Option<ChangeCommand>,
    ) -> Result<Value> {
        let prelude = self
            .entry_authorized_write_prelude(
                space_id,
                entry_id,
                Action::Update,
                parent_revision_id,
                principal_ids,
            )
            .await?;
        let result = entry::update_structured_entry_authorized_with_change(
            &self.operator,
            &prelude.workspace,
            entry_id,
            form_name,
            tags,
            fields,
            extra_attributes,
            parent_revision_id,
            author,
            &prelude.integrity,
            Some(&prelude.scopes),
            change,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        self.record_committed_entry_revision(
            space_id,
            entry_id,
            crate::mutation_audit::ENTRY_UPDATED_ACTION,
            principal_ids,
            author,
        )
        .await;
        Ok(result)
    }

    pub async fn delete_entry(&self, space_id: &str, entry_id: &str, actor: &str) -> Result<()> {
        self.delete_entry_with_receipt(space_id, entry_id, actor)
            .await
            .map(|_| ())
    }

    pub async fn delete_entry_with_receipt(
        &self,
        space_id: &str,
        entry_id: &str,
        actor: &str,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let receipt = entry::delete_entry_with_change_receipt(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            actor,
            None,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        self.record_committed_entry_delete(space_id, entry_id, &[], actor)
            .await;
        let mut result = json!({"deleted": true});
        if let Some(receipt) = receipt {
            result["change_id"] = json!(receipt.command_id);
            if let Some(revision_id) = receipt.committed_revision_ids.first() {
                result["revision_id"] = json!(revision_id.to_string());
            }
        }
        Ok(result)
    }

    pub async fn delete_entry_with_change(
        &self,
        space_id: &str,
        entry_id: &str,
        actor: &str,
        change: Option<ChangeCommand>,
    ) -> Result<()> {
        self.delete_entry_with_change_receipt(space_id, entry_id, actor, change)
            .await
            .map(|_| ())
    }

    pub async fn delete_entry_with_change_receipt(
        &self,
        space_id: &str,
        entry_id: &str,
        actor: &str,
        change: Option<ChangeCommand>,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let receipt = entry::delete_entry_with_change_receipt(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            actor,
            change,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        self.record_committed_entry_delete(space_id, entry_id, &[], actor)
            .await;
        let mut result = json!({"deleted": true});
        if let Some(receipt) = receipt {
            result["change_id"] = json!(receipt.command_id);
            if let Some(revision_id) = receipt.committed_revision_ids.first() {
                result["revision_id"] = json!(revision_id.to_string());
            }
        }
        Ok(result)
    }

    /// Operator-local authorized delete is intentionally absent: deletes
    /// with caller identity go through the principal-gated variants below,
    /// like creates and updates. The operator-local path above keeps `&[]`
    /// delivery attribution with the author fallback; the variants below
    /// deliver with the caller principals.
    pub async fn delete_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        self.delete_entry_authorized_for_principals_with_change(
            space_id,
            entry_id,
            author,
            principal_ids,
            None,
        )
        .await
    }

    /// Delete an Entry carrying an existing mutation/Change context (Change
    /// ID propagation and Run grouping). This is the delete counterpart to
    /// [`Self::create_entry_authorized_for_principals_with_change`]:
    /// admission, authorization (`Delete` on the Entry resource), and audit
    /// are identical, and the `change` is forwarded to the same shared
    /// entry boundary the operator-local path uses (no separate mutation
    /// implementation).
    ///
    /// Deletes always append a tombstone revision: history is append-only, so
    /// a delete never removes evidence and reconcile still converges
    /// `entry.deleted`.
    pub async fn delete_entry_authorized_for_principals_with_change(
        &self,
        space_id: &str,
        entry_id: &str,
        author: &str,
        principal_ids: &[Uuid],
        change: Option<ChangeCommand>,
    ) -> Result<Value> {
        self.delete_entry_authorized_for_principals_with_change_receipt(
            space_id,
            entry_id,
            author,
            principal_ids,
            change,
        )
        .await
    }

    pub async fn delete_entry_authorized_for_principals_with_change_receipt(
        &self,
        space_id: &str,
        entry_id: &str,
        author: &str,
        principal_ids: &[Uuid],
        change: Option<ChangeCommand>,
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let _authorization_lease = {
            let (state, lease) = Authorizer::new(self.operator.clone())
                .acquire_state_lease(space_id)
                .await?;
            self.require_action_for_principals_in_state(
                &state,
                entry_id,
                ResourceKind::Entry,
                Action::Delete,
                principal_ids,
            )?;
            lease
        };
        self.delete_entry_with_change_receipt_for_principals(
            space_id,
            entry_id,
            author,
            principal_ids,
            change,
        )
        .await
    }

    /// Principal-threaded delete for adapter-authorized callers only.
    ///
    /// Performs admission, identifier validation, the shared entry delete,
    /// and audit delivery attributed to `principal_ids` — but no
    /// authorization check and no lease acquisition. The batch Remove arm
    /// uses this because Delete on the exact Entry was already established
    /// by the request adapter, which may hold the (non-reentrant)
    /// authorization lease across the call. Direct callers must use
    /// [`Self::delete_entry_authorized_for_principals_with_change_receipt`].
    ///
    /// Crate-internal: every direct caller is inside this service module, so
    /// the boundary stays `pub(crate)` instead of growing a public
    /// authorization-bypassing surface.
    pub(crate) async fn delete_entry_with_change_receipt_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        author: &str,
        principal_ids: &[Uuid],
        change: Option<ChangeCommand>,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let receipt = entry::delete_entry_with_change_receipt(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            author,
            change,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        self.record_committed_entry_delete(space_id, entry_id, principal_ids, author)
            .await;
        let mut result = json!({"deleted": true});
        if let Some(receipt) = receipt {
            result["change_id"] = json!(receipt.command_id);
            if let Some(revision_id) = receipt.committed_revision_ids.first() {
                result["revision_id"] = json!(revision_id.to_string());
            }
        }
        Ok(result)
    }

    pub async fn entry_history(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::get_entry_history(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn entry_history_page(
        &self,
        space_id: &str,
        entry_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::get_entry_history_paged(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            limit,
            offset,
        )
        .await
    }

    pub async fn entry_history_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        self.entry_history_authorized_for_principals_page(
            space_id,
            entry_id,
            principal_ids,
            usize::MAX,
            0,
        )
        .await
    }

    pub async fn entry_history_authorized_for_principals_page(
        &self,
        space_id: &str,
        entry_id: &str,
        principal_ids: &[Uuid],
        limit: usize,
        offset: usize,
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                let mut history = entry::get_entry_history_authorized_paged(
                    &self.operator,
                    &self.workspace_path(space_id),
                    entry_id,
                    &scopes,
                    limit,
                    offset,
                )
                .await?;
                history["access_policy_history"] = serde_json::to_value(
                    state
                        .policy_history
                        .get(
                            &ResourceRef {
                                kind: ResourceKind::Entry,
                                id: entry_id.to_string(),
                                parent: None,
                            }
                            .key(),
                        )
                        .cloned()
                        .unwrap_or_default(),
                )?;
                Ok(history)
            })
            .await
    }

    pub async fn entry_history_at_pin(
        &self,
        space_id: &str,
        entry_id: &str,
        pin_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        self.entry_history_at_pin_page(space_id, entry_id, pin_name, principal_ids, usize::MAX, 0)
            .await
    }

    pub async fn entry_history_at_pin_page(
        &self,
        space_id: &str,
        entry_id: &str,
        pin_name: &str,
        principal_ids: &[Uuid],
        limit: usize,
        offset: usize,
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let publication = self.load_named_pin(space_id, pin_name).await?;
                let scopes = self
                    .checkpoint_form_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                let mut history = entry::get_entry_history_at_publication_paged(
                    &self.operator,
                    &self.workspace_path(space_id),
                    entry_id,
                    &publication,
                    scopes.as_ref(),
                    limit,
                    offset,
                )
                .await
                .map_err(map_checkpoint_error)?;
                history["access_policy_history"] = serde_json::to_value(
                    state
                        .policy_history
                        .get(
                            &ResourceRef {
                                kind: ResourceKind::Entry,
                                id: entry_id.to_string(),
                                parent: None,
                            }
                            .key(),
                        )
                        .cloned()
                        .unwrap_or_default(),
                )?;
                Ok(history)
            })
            .await
    }

    pub async fn entry_revision(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
    ) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        Ok(serde_json::to_value(
            entry::get_entry_revision_content(
                &self.operator,
                &self.workspace_path(space_id),
                entry_id,
                revision_id,
            )
            .await?,
        )?)
    }

    pub async fn entry_revision_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                let mut visible = false;
                for form_name in
                    entry::list_form_names(&self.operator, &self.workspace_path(space_id)).await?
                {
                    let Some(scope) = scopes.get(&form_name.to_ascii_lowercase()) else {
                        continue;
                    };
                    if entry::read_entry_row_authorized(
                        &self.operator,
                        &self.workspace_path(space_id),
                        &form_name,
                        entry_id,
                        scope,
                    )
                    .await
                    .is_ok()
                    {
                        visible = true;
                        break;
                    }
                }
                if !visible {
                    return Err(AppError::not_found(
                        ErrorCode::EntryNotFound,
                        format!("Entry not found: {entry_id}"),
                    )
                    .into());
                }
                let mut revision = serde_json::to_value(
                    entry::get_entry_revision_content(
                        &self.operator,
                        &self.workspace_path(space_id),
                        entry_id,
                        revision_id,
                    )
                    .await?,
                )?;
                revision["access_policy_history"] = serde_json::to_value(
                    state
                        .policy_history
                        .get(
                            &ResourceRef {
                                kind: ResourceKind::Entry,
                                id: entry_id.to_string(),
                                parent: None,
                            }
                            .key(),
                        )
                        .cloned()
                        .unwrap_or_default(),
                )?;
                Ok(revision)
            })
            .await
    }

    pub async fn entry_revision_at_pin(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        pin_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let publication = self.load_named_pin(space_id, pin_name).await?;
                let scopes = self
                    .checkpoint_form_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                let mut revision = entry::get_entry_revision_at_publication(
                    &self.operator,
                    &self.workspace_path(space_id),
                    entry_id,
                    revision_id,
                    &publication,
                    scopes.as_ref(),
                )
                .await
                .map_err(map_checkpoint_error)?;
                revision["access_policy_history"] = serde_json::to_value(
                    state
                        .policy_history
                        .get(
                            &ResourceRef {
                                kind: ResourceKind::Entry,
                                id: entry_id.to_string(),
                                parent: None,
                            }
                            .key(),
                        )
                        .cloned()
                        .unwrap_or_default(),
                )?;
                Ok(revision)
            })
            .await
    }

    pub async fn restore_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        author: &str,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let result = entry::restore_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
            author,
            &integrity,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(result)
    }

    pub async fn restore_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        let (state, _authorization_lease) = if principal_ids.is_empty() {
            (None, None)
        } else {
            let (state, lease) = Authorizer::new(self.operator.clone())
                .acquire_state_lease(space_id)
                .await?;
            self.require_action_for_principals_in_state(
                &state,
                entry_id,
                ResourceKind::Entry,
                Action::Update,
                principal_ids,
            )?;
            (Some(state), Some(lease))
        };
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let scopes = if principal_ids.is_empty() {
            None
        } else {
            Some(
                self.authorized_form_entry_scopes_for_state(
                    space_id,
                    state.as_ref().expect("authorized state is present"),
                    principal_ids,
                )
                .await?,
            )
        };
        let result = entry::restore_entry_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
            author,
            &integrity,
            scopes.as_ref(),
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(result)
    }

    pub async fn restore_entry_from_pin_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        pin_name: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        let (state, _authorization_lease) = {
            let (state, lease) = Authorizer::new(self.operator.clone())
                .acquire_state_lease(space_id)
                .await?;
            self.require_action_for_principals_in_state(
                &state,
                entry_id,
                ResourceKind::Entry,
                Action::Update,
                principal_ids,
            )?;
            (state, lease)
        };
        let publication = self.load_named_pin(space_id, pin_name).await?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let scopes = self
            .checkpoint_form_scopes_for_state(space_id, &state, principal_ids)
            .await?;
        let result = entry::restore_entry_from_publication_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
            &publication,
            author,
            &integrity,
            scopes.as_ref(),
        )
        .await
        .map_err(map_checkpoint_error)?;
        self.schedule_asset_text_refresh(space_id);
        Ok(result)
    }

    pub async fn entry_at_pin_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        pin_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let publication = self.load_named_pin(space_id, pin_name).await?;
                let scopes = self
                    .checkpoint_form_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                entry::get_entry_at_publication(
                    &self.operator,
                    &self.workspace_path(space_id),
                    entry_id,
                    &publication,
                    scopes.as_ref(),
                )
                .await
                .map_err(map_checkpoint_error)
            })
            .await
    }

    pub async fn diff_pins_authorized_for_principals(
        &self,
        space_id: &str,
        from_name: &str,
        to_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        let mut diff = authorizer
            .with_state_lock(space_id, |state| async move {
                let from = self.load_named_pin(space_id, from_name).await?;
                let to = self.load_named_pin(space_id, to_name).await?;
                let scopes = self
                    .checkpoint_form_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                let workspace =
                    iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id))
                        .await?;
                let diff = workspace
                    .diff_publications_with_scopes(&from, &to, scopes.as_ref())
                    .await
                    .map_err(map_checkpoint_error)?;
                Ok::<Value, anyhow::Error>(serde_json::to_value(diff)?)
            })
            .await?;
        project_pin_diff_entry_ids(&mut diff);
        Ok(diff)
    }

    /// Compares two named Pins as the local operator with full visibility.
    ///
    /// Both Pins are named explicitly: no implicit latest/current revision is
    /// ever selected. Read-only; Entry history is untouched.
    pub async fn diff_pins(&self, space_id: &str, from_name: &str, to_name: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let from = self.load_named_pin(space_id, from_name).await?;
        let to = self.load_named_pin(space_id, to_name).await?;
        let mut diff = serde_json::to_value(
            workspace
                .diff_publications(&from, &to)
                .await
                .map_err(map_checkpoint_error)?,
        )?;
        project_pin_diff_entry_ids(&mut diff);
        Ok(diff)
    }

    async fn load_named_pin(&self, space_id: &str, pin_name: &str) -> Result<PublicationRef> {
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        workspace
            .resolve_pin(pin_name)
            .await
            .map_err(map_checkpoint_error)
    }

    async fn checkpoint_form_scopes_for_state(
        &self,
        space_id: &str,
        state: &AuthorizationState,
        principal_ids: &[Uuid],
    ) -> Result<Option<BTreeMap<FormId, EntryScope>>> {
        require_nonempty_authorized_principals(principal_ids)?;
        let scopes = self
            .authorized_form_entry_scopes_for_state(space_id, state, principal_ids)
            .await?;
        let saved_sql_scope = Self::saved_sql_entry_scope_for_state(state, principal_ids)?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let form_scopes = workspace
            .list_forms()
            .await?
            .into_iter()
            .filter_map(|form| {
                let scope = if form.name.eq_ignore_ascii_case("SQL") {
                    saved_sql_scope.clone()
                } else {
                    scopes.get(&form.name.to_ascii_lowercase()).cloned()?
                };
                scopes
                    .get(&form.name.to_ascii_lowercase())
                    .map(|_| (form.id, scope))
            })
            .collect();
        Ok(Some(form_scopes))
    }

    fn require_action_for_principals_in_state(
        &self,
        state: &AuthorizationState,
        entry_id: &str,
        resource_kind: ResourceKind,
        action: Action,
        principal_ids: &[Uuid],
    ) -> Result<()> {
        require_nonempty_authorized_principals(principal_ids)?;
        let resource = ResourceRef {
            kind: resource_kind,
            id: entry_id.to_string(),
            parent: None,
        };
        for principal_id in principal_ids {
            if !effective_actions_for_state(state, *principal_id, Some(&resource))?
                .contains(&action)
            {
                return Err(
                    AppError::forbidden("resource is not authorized for this action").into(),
                );
            }
        }
        Ok(())
    }

    pub async fn list_entry_options(
        &self,
        space_id: &str,
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        self.validate_complete_space(space_id).await?;
        if let Some(form) = form {
            validate_storage_id(validate_form_name(form))?;
        }
        entry::list_entry_summaries(
            &self.operator,
            &self.workspace_path(space_id),
            form,
            query,
            limit,
        )
        .await
    }

    pub async fn list_entry_options_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, &[principal_id])
                    .await?;
                entry::list_entry_summaries_with_scopes(
                    &self.operator,
                    &self.workspace_path(space_id),
                    form,
                    query,
                    limit,
                    &scopes,
                )
                .await
            })
            .await
    }

    pub async fn list_entry_options_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                entry::list_entry_summaries_with_scopes(
                    &self.operator,
                    &self.workspace_path(space_id),
                    form,
                    query,
                    limit,
                    &scopes,
                )
                .await
            })
            .await
    }

    pub async fn search_entries(
        &self,
        space_id: &str,
        query: &str,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        ugoite_core::query::validate_keyword_query(query)?;
        self.validate_complete_space(space_id).await?;
        search::search_entries(
            &self.operator,
            &self.workspace_path(space_id),
            query,
            crate::MAX_NORMAL_READ_ROWS,
        )
        .await
    }

    pub async fn search_entries_page(
        &self,
        space_id: &str,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        ugoite_core::query::validate_keyword_query(query)?;
        self.validate_complete_space(space_id).await?;
        search::search_entries_paged(
            &self.operator,
            &self.workspace_path(space_id),
            query,
            limit,
            offset,
        )
        .await
    }

    pub async fn query_entries(&self, space_id: &str, filter: &Value) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        index::query_index(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
        )
        .await
    }

    /// Typed structured Search through the authorized query policy. Logical
    /// criteria only; relation/column resolution and escaping stay in the
    /// trusted adapter.
    pub async fn search_structured(
        &self,
        space_id: &str,
        criteria: &ugoite_core::structured_search::StructuredSearch,
    ) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        crate::structured_search::search_structured(
            &self.operator,
            &self.workspace_path(space_id),
            criteria,
        )
        .await
    }

    pub async fn execute_sql_query(&self, space_id: &str, sql: &str) -> Result<Vec<Value>> {
        index::validate_read_only_sql(sql)?;
        self.validate_complete_space(space_id).await?;
        index::execute_sql_query(&self.operator, &self.workspace_path(space_id), sql).await
    }

    pub async fn require_resource_action(
        &self,
        space_id: &str,
        principal_id: Uuid,
        action: Action,
        kind: ResourceKind,
        resource_id: &str,
        parent: Option<ResourceRef>,
    ) -> Result<()> {
        Authorizer::new(self.operator.clone())
            .require(
                space_id,
                principal_id,
                action,
                Some(&ResourceRef {
                    kind,
                    id: resource_id.to_string(),
                    parent: parent.map(Box::new),
                }),
            )
            .await
    }

    pub async fn get_access_policy_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        resource: ResourceRef,
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                for principal_id in principal_ids {
                    if !effective_actions_for_state(&state, *principal_id, Some(&resource))?
                        .contains(&Action::Read)
                    {
                        return Err(AppError::forbidden("resource is not authorized").into());
                    }
                }
                serde_json::to_value(state.policies.get(&resource.key())).map_err(Into::into)
            })
            .await
    }

    /// The only scope accepted by reference/existence checks. It is derived
    /// from the caller's authorization state before a closed DataFusion
    /// context is built; an omitted Form is not an all-rows fallback.
    pub async fn authorized_form_entry_scopes(
        &self,
        space_id: &str,
        principal_id: Uuid,
    ) -> Result<BTreeMap<String, EntryScope>> {
        self.authorized_form_entry_scopes_for_principals(space_id, &[principal_id])
            .await
    }

    /// Derives the provider-side Entry scope from one authorization snapshot.
    /// Entry policies are sparse: Space read is inherited by every current
    /// Entry unless a policy removes it, so the closed query context can use
    /// `AllExcept` without listing or scanning Entry rows in Rust.
    pub async fn authorized_form_entry_scopes_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<BTreeMap<String, EntryScope>> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        Authorizer::new(self.operator.clone())
            .with_state_lock(space_id, |state| async move {
                self.authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await
            })
            .await
    }

    async fn authorized_form_entry_scopes_for_state(
        &self,
        space_id: &str,
        state: &AuthorizationState,
        principal_ids: &[Uuid],
    ) -> Result<BTreeMap<String, EntryScope>> {
        require_nonempty_authorized_principals(principal_ids)?;
        for principal_id in principal_ids {
            if !effective_actions_for_state(state, *principal_id, None)?.contains(&Action::Read) {
                return Ok(BTreeMap::new());
            }
        }
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let readable_forms = workspace
            .list_forms_bounded(
                MAX_AUTHORIZED_SCOPE_FORMS,
                MAX_AUTHORIZED_SCOPE_FORM_DEFINITION_BYTES,
            )
            .await?
            .into_iter()
            .collect::<Vec<_>>();
        Self::authorized_form_entry_scopes_for_forms(state, principal_ids, readable_forms)
    }

    fn authorized_form_entry_scopes_for_forms(
        state: &AuthorizationState,
        principal_ids: &[Uuid],
        forms: Vec<FormDefinition>,
    ) -> Result<BTreeMap<String, EntryScope>> {
        let readable_forms = forms
            .into_iter()
            .filter(|form| {
                let resource = ResourceRef {
                    kind: ResourceKind::Form,
                    id: form.name.clone(),
                    parent: None,
                };
                principal_ids.iter().all(|principal_id| {
                    effective_actions_for_state(state, *principal_id, Some(&resource))
                        .map(|actions| actions.contains(&Action::Read))
                        .unwrap_or(false)
                })
            })
            .collect::<Vec<_>>();
        let mut denied_entry_ids = BTreeSet::new();
        for resource_key in state.policies.keys() {
            let Some(entry_id) = resource_key.strip_prefix("entry:") else {
                continue;
            };
            let resource = ResourceRef {
                kind: ResourceKind::Entry,
                id: entry_id.to_string(),
                parent: None,
            };
            let mut readable_by_every_principal = true;
            for principal_id in principal_ids {
                if !effective_actions_for_state(state, *principal_id, Some(&resource))?
                    .contains(&Action::Read)
                {
                    readable_by_every_principal = false;
                    break;
                }
            }
            if !readable_by_every_principal {
                denied_entry_ids.insert(
                    Uuid::parse_str(entry_id)
                        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, entry_id.as_bytes()))
                        .into(),
                );
            }
        }
        let entry_scope = if denied_entry_ids.is_empty() {
            EntryScope::AllCurrent
        } else {
            EntryScope::AllExcept(denied_entry_ids)
        };
        Ok(readable_forms
            .into_iter()
            .map(|form| (form.name.to_ascii_lowercase(), entry_scope.clone()))
            .collect())
    }

    /// Derives the Saved SQL resource ACL as a provider-side Entry scope from
    /// one loaded authorization snapshot.
    pub async fn authorized_saved_sql_entry_scope_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<EntryScope> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        Authorizer::new(self.operator.clone())
            .with_state_lock(space_id, |state| async move {
                Self::saved_sql_entry_scope_for_state(&state, principal_ids)
            })
            .await
    }

    pub(crate) fn saved_sql_entry_scope_for_state(
        state: &AuthorizationState,
        principal_ids: &[Uuid],
    ) -> Result<EntryScope> {
        require_nonempty_authorized_principals(principal_ids)?;
        for principal_id in principal_ids {
            if !effective_actions_for_state(state, *principal_id, None)?.contains(&Action::Read) {
                return Ok(EntryScope::Only(BTreeSet::new()));
            }
        }
        let mut denied_entry_ids = BTreeSet::new();
        for resource_key in state.policies.keys() {
            let Some(sql_id) = resource_key.strip_prefix("saved_sql:") else {
                continue;
            };
            let resource = ResourceRef {
                kind: ResourceKind::SavedSql,
                id: sql_id.to_string(),
                parent: None,
            };
            let readable_by_every_principal = principal_ids.iter().all(|principal_id| {
                effective_actions_for_state(state, *principal_id, Some(&resource))
                    .map(|actions| actions.contains(&Action::Read))
                    .unwrap_or(false)
            });
            if !readable_by_every_principal {
                denied_entry_ids.insert(
                    Uuid::parse_str(sql_id)
                        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, sql_id.as_bytes()))
                        .into(),
                );
            }
        }
        Ok(EntryScope::AllExcept(denied_entry_ids))
    }

    pub async fn list_entries_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Value>> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                entry::list_entries_with_scopes(
                    &self.operator,
                    &self.workspace_path(space_id),
                    &scopes,
                    limit,
                    offset,
                )
                .await
            })
            .await
    }

    pub async fn list_entries_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
    ) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, &[principal_id])
                    .await?;
                entry::list_entries_with_scopes(
                    &self.operator,
                    &self.workspace_path(space_id),
                    &scopes,
                    crate::MAX_NORMAL_READ_ROWS,
                    0,
                )
                .await
            })
            .await
    }

    pub async fn filter_json_resources_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        kind: ResourceKind,
        id_field: &str,
        values: Vec<Value>,
    ) -> Result<Vec<Value>> {
        require_nonempty_authorized_principals(&[principal_id])?;
        self.validate_complete_space(space_id).await?;
        Authorizer::new(self.operator.clone())
            .with_state_lock(space_id, |state| async move {
                Self::filter_json_resources_for_state(&state, principal_id, kind, id_field, values)
            })
            .await
    }

    fn filter_json_resources_for_state(
        state: &AuthorizationState,
        principal_id: Uuid,
        kind: ResourceKind,
        id_field: &str,
        values: Vec<Value>,
    ) -> Result<Vec<Value>> {
        let mut resources = Vec::new();
        for value in &values {
            let Some(id) = value.get(id_field).and_then(Value::as_str) else {
                continue;
            };
            resources.push(ResourceRef {
                kind: kind.clone(),
                id: id.to_string(),
                parent: None,
            });
        }
        let mut allowed = BTreeSet::new();
        for resource in resources {
            if effective_actions_for_state(state, principal_id, Some(&resource))?
                .contains(&Action::Read)
            {
                allowed.insert(resource.id);
            }
        }
        Ok(values
            .into_iter()
            .filter(|value| {
                value
                    .get(id_field)
                    .and_then(Value::as_str)
                    .is_some_and(|id| allowed.contains(id))
            })
            .collect())
    }

    pub async fn filter_json_resources_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        kind: ResourceKind,
        id_field: &str,
        values: Vec<Value>,
    ) -> Result<Vec<Value>> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        Authorizer::new(self.operator.clone())
            .with_state_lock(space_id, |state| async move {
                let mut filtered = values;
                for principal_id in principal_ids {
                    filtered = Self::filter_json_resources_for_state(
                        &state,
                        *principal_id,
                        kind.clone(),
                        id_field,
                        filtered,
                    )?;
                }
                Ok(filtered)
            })
            .await
    }

    pub async fn search_entries_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        query: &str,
        limit: usize,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        self.search_entries_authorized_for_principals_page(space_id, principal_ids, query, limit, 0)
            .await
    }

    pub async fn search_entries_authorized_for_principals_page(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        ugoite_core::query::validate_keyword_query(query)?;
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        for _ in 0..3 {
            let (revision, stable, result) = authorizer
                .with_state_lock(space_id, |state| async {
                    let revision = state.revision;
                    let scopes = self
                        .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                        .await?;
                    let asset_authorization = search::AssetAuthorization::new(state, principal_ids);
                    let result = search::search_entries_with_scopes_paged_authorized(
                        &self.operator,
                        &self.workspace_path(space_id),
                        query,
                        &scopes,
                        limit,
                        offset,
                        Some(asset_authorization),
                    )
                    .await;
                    let current_revision = Authorizer::new(self.operator.clone())
                        .state(space_id)
                        .await?
                        .revision;
                    Ok((revision, current_revision == revision, result))
                })
                .await?;
            if stable {
                return result;
            }
            let _ = revision;
        }
        Err(anyhow!(
            "authorization changed while executing the protected search"
        ))
    }

    pub async fn search_entries_authorized_for_principals_after(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        query: &str,
        limit: usize,
        after: Option<(&str, &str)>,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        ugoite_core::query::validate_keyword_query(query)?;
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        for _ in 0..3 {
            let (revision, stable, result) = authorizer
                .with_state_lock(space_id, |state| async {
                    let revision = state.revision;
                    let scopes = self
                        .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                        .await?;
                    let asset_authorization = search::AssetAuthorization::new(state, principal_ids);
                    let result = search::search_entries_with_scopes_after_authorized(
                        &self.operator,
                        &self.workspace_path(space_id),
                        query,
                        &scopes,
                        limit,
                        after,
                        Some(asset_authorization),
                    )
                    .await;
                    let current_revision = Authorizer::new(self.operator.clone())
                        .state(space_id)
                        .await?
                        .revision;
                    Ok((revision, current_revision == revision, result))
                })
                .await?;
            if stable {
                return result;
            }
            let _ = revision;
        }
        Err(anyhow!(
            "authorization changed while executing the protected search"
        ))
    }

    /// Authorized structured Search. Permission filtering is applied before
    /// query execution via form scopes; core direct and authorized paths
    /// return the same Entry set/order for identical criteria.
    pub async fn search_structured_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        criteria: &ugoite_core::structured_search::StructuredSearch,
    ) -> Result<Vec<Value>> {
        // Syntax-only validation is independent of Space state and Form
        // existence. Admit it before principal, storage, or authorization
        // reads so all service callers share the same cheap failure boundary.
        ugoite_core::structured_search::validate_structured_search_syntax(criteria)?;
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        // Clone criteria for the state-lock closure.
        let criteria = criteria.clone();
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                crate::structured_search::search_structured_with_scopes(
                    &self.operator,
                    &self.workspace_path(space_id),
                    &criteria,
                    &scopes,
                )
                .await
            })
            .await
    }

    pub async fn create_sql_session_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        sql: &str,
    ) -> Result<Value> {
        self.create_sql_session_authorized_for_principals_with_parameters(
            space_id,
            principal_ids,
            sql,
            serde_json::Map::new(),
            BTreeMap::new(),
        )
        .await
    }

    pub async fn create_sql_session_authorized_for_principals_with_parameters(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        sql: &str,
        parameters: serde_json::Map<String, Value>,
        parameter_types: BTreeMap<String, String>,
    ) -> Result<Value> {
        // Shared read-only admission runs before session planning, checkpoint
        // resolution, or mutation admission so write/DDL/multi-statement input
        // fails with READ_ONLY_SQL_REQUIRED on every entry point.
        index::validate_read_only_sql(sql)?;
        let relation = index::sql_session_page_relation(sql).map_err(|error| {
            AppError::invalid_input(
                ugoite_core::error::ErrorCode::InvalidInput,
                error.to_string(),
            )
        })?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        require_sql_session_principals(principal_ids)?;
        let (state, _authorization_lease) = Authorizer::new(self.operator.clone())
            .acquire_state_lease(space_id)
            .await?;
        for principal_id in principal_ids {
            if !effective_actions_for_state(&state, *principal_id, None)?.contains(&Action::Read) {
                return Err(
                    AppError::forbidden("principal is not authorized to read this Space").into(),
                );
            }
        }
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = workspace.current_publication().await?;
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let entry_scope = sql_session_entry_scope(&state, principal_ids)?;
        let saved_sql_entry_scope = Self::saved_sql_entry_scope_for_state(&state, principal_ids)?;
        let query_policy = index::sql_session_query_policy_at_checkpoint(
            &self.operator,
            &self.workspace_path(space_id),
            &relation,
            entry_scope,
            &checkpoint,
        )
        .await?;
        let authorization_policy_hash = sql_session_policy_hash(&state, principal_ids)?;
        let authorization = sql_session::SqlSessionAuthorization {
            principal_ids,
            policy_hash: &authorization_policy_hash,
        };
        let bound_parameters = index::datafusion_parameters(&parameters, &parameter_types)
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
        sql_session::create_sql_session_authorized_for_principals_with_frozen_policy_and_saved_sql_scope(
            &self.operator,
            &self.workspace_path(space_id),
            sql,
            parameters,
            parameter_types,
            authorization,
            bound_parameters,
            publication,
            checkpoint,
            query_policy,
            &saved_sql_entry_scope,
        )
        .await
    }

    pub async fn get_sql_session_authorized_for_principals(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let current_authorization = self
                    .sql_session_current_execution_authorization(
                        space_id,
                        session_id,
                        principal_ids,
                        &state,
                    )
                    .await?;
                sql_session::get_sql_session_status_authorized(
                    &self.operator,
                    &self.workspace_path(space_id),
                    session_id,
                    sql_session::SqlSessionExecutionAuthorization {
                        authorization: sql_session::SqlSessionAuthorization {
                            principal_ids,
                            policy_hash: &current_authorization.policy_hash,
                        },
                        query_policy: &current_authorization.query_policy,
                    },
                )
                .await
            })
            .await
    }

    /// Rebuilds the execution policy from immutable publication metadata and
    /// the current authorization state. Durable session policy JSON is only a
    /// cache: every use compares it against this independently derived value.
    async fn sql_session_current_execution_authorization(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
        state: &AuthorizationState,
    ) -> Result<CurrentSqlSessionExecutionAuthorization> {
        require_sql_session_principals(principal_ids)?;
        let workspace_path = self.workspace_path(space_id);
        let inputs =
            sql_session::get_session_execution_inputs(&self.operator, &workspace_path, session_id)
                .await
                .map_err(sql_session_metadata_authorization_error)?;
        let relation = index::sql_session_page_relation(&inputs.sql)
            .map_err(sql_session_metadata_authorization_error)?;
        let entry_scope = sql_session_entry_scope(state, principal_ids)?;
        let workspace = iceberg_store::native_workspace(&self.operator, &workspace_path)
            .await
            .map_err(sql_session_metadata_authorization_error)?;
        let checkpoint = workspace
            .resolve_publication(&inputs.publication)
            .await
            .map_err(sql_session_metadata_authorization_error)?;
        let query_policy = index::sql_session_query_policy_at_checkpoint(
            &self.operator,
            &workspace_path,
            &relation,
            entry_scope,
            &checkpoint,
        )
        .await
        .map_err(sql_session_metadata_authorization_error)?;
        Ok(CurrentSqlSessionExecutionAuthorization {
            policy_hash: sql_session_policy_hash(state, principal_ids)?,
            query_policy,
        })
    }

    pub async fn search_entries_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        query: &str,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        self.search_entries_authorized_for_principals(
            space_id,
            &[principal_id],
            query,
            crate::MAX_NORMAL_READ_ROWS,
        )
        .await
    }

    pub async fn query_entries_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        filter: &Value,
    ) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, &[principal_id])
                    .await?;
                index::query_index_authorized_by_form_scopes(
                    &self.operator,
                    &self.workspace_path(space_id),
                    &filter.to_string(),
                    &scopes,
                )
                .await
            })
            .await
    }

    pub async fn execute_sql_query_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        sql: &str,
    ) -> Result<Vec<Value>> {
        // Reject writes and malformed SQL before storage discovery or
        // authorization reads. This keeps the authorized service path on the
        // same admission contract as direct SQL execution and SQL sessions.
        index::validate_read_only_sql(sql)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, &[principal_id])
                    .await?;
                index::execute_sql_query_authorized_by_form_scopes(
                    &self.operator,
                    &self.workspace_path(space_id),
                    sql,
                    &scopes,
                )
                .await
            })
            .await
    }

    /// Executes one stateless, read-only SQL page. The publication is fixed
    /// by the first request and carried by an opaque continuation; every
    /// request still rebuilds authorization from the current state.
    pub async fn query_sql_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        request: SqlQueryRequest,
    ) -> Result<SqlQueryPage> {
        request.validate().map_err(sql_query_contract_error)?;
        require_nonempty_authorized_principals(principal_ids)?;
        let normalized_sql = index::normalize_sql_template(&request.sql)?;
        index::validate_read_only_sql(&normalized_sql)?;
        self.validate_complete_space(space_id).await?;

        let signing_key = self.sql_query_signing_key(space_id);
        let continuation = request
            .continuation
            .as_deref()
            .map(|value| {
                SqlContinuation::decode(value, &signing_key).map_err(sql_query_contract_error)
            })
            .transpose()?;
        let effective_parameter_types =
            effective_sql_parameter_types(&request.parameters, &request.parameter_types)?;
        let parameter_fingerprint = SqlQueryRequest {
            parameter_types: effective_parameter_types.clone(),
            ..request.clone()
        }
        .parameter_fingerprint()
        .map_err(sql_query_contract_error)?;
        let sql_fingerprint = request
            .sql_fingerprint(&normalized_sql)
            .map_err(sql_query_contract_error)?;
        let parameters =
            index::datafusion_parameters(&request.parameters, &effective_parameter_types).map_err(
                |error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()),
            )?;

        let (state, _authorization_lease) = Authorizer::new(self.operator.clone())
            .acquire_state_lease(space_id)
            .await?;
        for principal_id in principal_ids {
            if !effective_actions_for_state(&state, *principal_id, None)?.contains(&Action::Read) {
                return Err(
                    AppError::forbidden("principal is not authorized to read this Space").into(),
                );
            }
        }
        let authorization_fingerprint = sql_query_authorization_fingerprint(&state, principal_ids)?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let (publication, offset) = match continuation {
            Some(cursor) => {
                if cursor.space_id.as_uuid() != state.space_uid {
                    return Err(AppError::invalid_input(
                        ErrorCode::InvalidInput,
                        "SQL continuation belongs to another Space",
                    )
                    .into());
                }
                if cursor.sql_fingerprint != sql_fingerprint
                    || cursor.parameter_fingerprint != parameter_fingerprint
                {
                    return Err(AppError::invalid_input(
                        ErrorCode::InvalidInput,
                        "SQL or parameter fingerprint does not match the continuation",
                    )
                    .into());
                }
                cursor
                    .authorize(&authorization_fingerprint)
                    .map_err(sql_query_contract_error)?;
                let publication = cursor.publication.clone();
                workspace.resolve_publication(&publication).await?;
                (publication, cursor.offset)
            }
            None => {
                let publication = workspace.current_publication().await?;
                (publication, 0)
            }
        };
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
        let named_scopes =
            Self::authorized_form_entry_scopes_for_forms(&state, principal_ids, forms.clone())?;
        let relation_scopes = forms
            .iter()
            .filter_map(|form| {
                named_scopes
                    .get(&form.name.to_ascii_lowercase())
                    .cloned()
                    .map(|scope| (sql_relation_name(form.id), scope))
            })
            .collect::<BTreeMap<_, _>>();
        let fetch_limit = request
            .limit
            .checked_add(1)
            .ok_or_else(|| anyhow!("SQL query page limit overflows"))?;
        let (columns, mut rows, has_order) =
            index::execute_sql_query_authorized_by_form_page_at_checkpoint_stateless(
                &self.operator,
                &self.workspace_path(space_id),
                &normalized_sql,
                &relation_scopes,
                parameters,
                offset,
                fetch_limit,
                checkpoint,
            )
            .await?;
        let has_more = has_order && rows.len() > request.limit;
        rows.truncate(request.limit);
        let next = if has_more {
            let next_offset = offset
                .checked_add(rows.len())
                .ok_or_else(|| anyhow!("SQL continuation offset overflows"))?;
            Some(
                SqlContinuation::new(
                    state.space_uid.into(),
                    publication,
                    sql_fingerprint,
                    parameter_fingerprint,
                    authorization_fingerprint,
                    next_offset,
                )?
                .encode(&signing_key)?,
            )
        } else {
            None
        };
        Ok(SqlQueryPage {
            columns,
            rows,
            has_more,
            next,
        })
    }

    /// Explicit SQL count operation. It starts a fresh checkpoint and never
    /// creates or updates execution state in the Space.
    pub async fn count_sql_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        request: SqlQueryCountRequest,
    ) -> Result<u64> {
        request.validate().map_err(sql_query_contract_error)?;
        require_nonempty_authorized_principals(principal_ids)?;
        let normalized_sql = index::normalize_sql_template(&request.sql)?;
        index::validate_read_only_sql(&normalized_sql)?;
        self.validate_complete_space(space_id).await?;
        let effective_parameter_types =
            effective_sql_parameter_types(&request.parameters, &request.parameter_types)?;
        let parameters =
            index::datafusion_parameters(&request.parameters, &effective_parameter_types).map_err(
                |error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()),
            )?;
        let (state, _authorization_lease) = Authorizer::new(self.operator.clone())
            .acquire_state_lease(space_id)
            .await?;
        for principal_id in principal_ids {
            if !effective_actions_for_state(&state, *principal_id, None)?.contains(&Action::Read) {
                return Err(
                    AppError::forbidden("principal is not authorized to read this Space").into(),
                );
            }
        }
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let publication = workspace.current_publication().await?;
        let checkpoint = workspace.resolve_publication(&publication).await?;
        let forms = workspace.forms_at_checkpoint(&checkpoint).await?;
        let named_scopes =
            Self::authorized_form_entry_scopes_for_forms(&state, principal_ids, forms.clone())?;
        let relation_scopes = forms
            .iter()
            .filter_map(|form| {
                named_scopes
                    .get(&form.name.to_ascii_lowercase())
                    .cloned()
                    .map(|scope| (sql_relation_name(form.id), scope))
            })
            .collect::<BTreeMap<_, _>>();
        index::execute_sql_query_authorized_by_form_count_at_checkpoint_stateless(
            &self.operator,
            &self.workspace_path(space_id),
            &normalized_sql,
            &relation_scopes,
            parameters,
            checkpoint,
        )
        .await
    }

    pub async fn reindex(&self, space_id: &str) -> Result<()> {
        self.validate_complete_space(space_id).await?;
        index::reindex_all(&self.operator, &self.workspace_path(space_id)).await
    }

    /// Performs the synchronous cleanup pass used by explicit local index
    /// maintenance. Server workers can keep the delayed grace-period task
    /// alive; `ugoite index run` invokes this pass before it exits.
    pub async fn garbage_collect_asset_text_builds(&self, space_id: &str) -> Result<Vec<String>> {
        self.validate_complete_space(space_id).await?;
        crate::derived_relation::garbage_collect_asset_text(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await
    }

    /// Rehydrates derived GC after a server restart. Derived cleanup is
    /// best-effort and never blocks authoritative startup recovery.
    pub async fn rearm_asset_text_gc(&self, space_id: &str) -> Result<()> {
        self.validate_complete_space(space_id).await?;
        crate::derived_relation::rearm_asset_text_gc(&self.operator, &self.workspace_path(space_id))
            .await
    }

    /// Rehydrates and executes one bounded AssetText refresh from the durable
    /// authoritative source coordinate. Startup maintenance owns its permit
    /// across this call, so the actual rebuild—not only enqueueing detached
    /// work—is included in the node-wide concurrency bound. Authoritative
    /// mutation paths continue to use the process-local best-effort worker and
    /// never await this method.
    pub async fn rearm_asset_text_refresh(&self, space_id: &str) -> Result<()> {
        self.validate_complete_space(space_id).await?;
        let ws_path = self.workspace_path(space_id);
        if crate::derived_relation::asset_text_refresh_needed(&self.operator, &ws_path).await? {
            let shared = !ugoite_storage::is_local_operator(&self.operator);
            if shared {
                crate::derived_relation::rebuild_asset_text_shared(&self.operator, &ws_path)
                    .await?;
            } else {
                crate::derived_relation::rebuild_asset_text(&self.operator, &ws_path).await?;
            }
        }
        Ok(())
    }

    pub async fn space_stats(&self, space_id: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        index::get_space_stats(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn save_asset(
        &self,
        space_id: &str,
        filename: &str,
        content: &[u8],
    ) -> Result<ugoite_domain::entry::AssetReference> {
        self.save_asset_with_media_type(space_id, filename, content, "application/octet-stream")
            .await
    }

    pub async fn save_asset_with_media_type(
        &self,
        space_id: &str,
        filename: &str,
        content: &[u8],
        media_type: &str,
    ) -> Result<ugoite_domain::entry::AssetReference> {
        // This is the canonical service boundary used by REST multipart upload.
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        asset::save_asset_with_media_type(
            &self.operator,
            &self.workspace_path(space_id),
            filename,
            content,
            media_type,
        )
        .await
    }

    pub async fn read_asset(&self, space_id: &str, asset_id: &str) -> Result<asset::AssetContent> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_asset_id(asset_id))?;
        asset::read_asset(&self.operator, &self.workspace_path(space_id), asset_id).await
    }

    pub async fn read_asset_authorized_for_principals(
        &self,
        space_id: &str,
        form_name: &str,
        entry_id: &str,
        asset_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<asset::AssetContent> {
        // The service enforces both the containing Entry and the Asset edge.
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_form_name(form_name))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_asset_id(asset_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let entry_resource = ResourceRef {
                    kind: ResourceKind::Entry,
                    id: entry_id.to_string(),
                    parent: None,
                };
                let asset_resource = ResourceRef {
                    kind: ResourceKind::Asset,
                    id: asset_id.to_string(),
                    parent: Some(Box::new(entry_resource.clone())),
                };
                for principal_id in principal_ids {
                    if !effective_actions_for_state(&state, *principal_id, Some(&entry_resource))?
                        .contains(&Action::Read)
                        || !effective_actions_for_state(
                            &state,
                            *principal_id,
                            Some(&asset_resource),
                        )?
                        .contains(&Action::Read)
                    {
                        return Err(AppError::not_found(
                            ErrorCode::AssetNotFound,
                            format!("Asset {asset_id} not found"),
                        )
                        .into());
                    }
                }
                let scopes = self
                    .authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
                    .await?;
                let Some(form_scope) = scopes.get(&form_name.to_ascii_lowercase()) else {
                    return Err(AppError::not_found(
                        ErrorCode::AssetNotFound,
                        format!("Asset {asset_id} not found"),
                    )
                    .into());
                };
                entry::read_entry_row_authorized(
                    &self.operator,
                    &self.workspace_path(space_id),
                    form_name,
                    entry_id,
                    form_scope,
                )
                .await
                .map_err(|_| {
                    anyhow::Error::from(AppError::not_found(
                        ErrorCode::AssetNotFound,
                        format!("Asset {asset_id} not found"),
                    ))
                })?;
                let entry_uuid = Uuid::parse_str(entry_id)
                    .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, entry_id.as_bytes()));
                let references = BTreeMap::from([(
                    form_name.to_ascii_lowercase(),
                    EntryScope::Only(BTreeSet::from([entry_uuid.into()])),
                )]);
                if !asset::current_asset_reference_exists_in_workspace(
                    &iceberg_store::native_workspace(
                        &self.operator,
                        &self.workspace_path(space_id),
                    )
                    .await?,
                    asset_id,
                    &references,
                )
                .await?
                {
                    return Err(AppError::not_found(
                        ErrorCode::AssetNotFound,
                        format!("Asset {asset_id} not found"),
                    )
                    .into());
                }
                asset::read_asset(&self.operator, &self.workspace_path(space_id), asset_id).await
            })
            .await
    }

    /// Lists Form-owned Asset reference metadata visible in a Space.
    ///
    /// Metadata only, never bytes: names, media types, and sizes belong to
    /// the referencing Entry fields. Tombstoned entries are excluded because
    /// their references are no longer visible Knowledge.
    pub async fn list_assets(&self, space_id: &str) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        let entries =
            crate::entry::list_entries(self.operator(), &self.workspace_path(space_id)).await?;
        Ok(crate::asset::collect_asset_references(&entries))
    }

    /// Lists Form-owned Asset references through per-Entry authorization.
    ///
    /// Only entries the principals may read contribute references; anything
    /// else is excluded rather than reported. Same projection as
    /// [`Self::list_assets`].
    pub async fn list_assets_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Vec<Value>> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let mut entries = Vec::new();
        let mut offset = 0;
        loop {
            let page = self
                .list_entries_authorized_for_principals(space_id, principal_ids, 1000, offset)
                .await?;
            let page_len = page.len();
            entries.extend(page);
            if page_len < 1000 {
                break;
            }
            offset += page_len;
        }
        Ok(crate::asset::collect_asset_references(&entries))
    }

    pub async fn ensure_asset_reference_is_readable(
        &self,
        space_id: &str,
        form_name: &str,
        entry_id: &str,
        asset_id: &str,
    ) -> Result<()> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_form_name(form_name))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_asset_id(asset_id))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let form = workspace
            .list_forms()
            .await?
            .into_iter()
            .find(|form| form.name == form_name)
            .ok_or_else(|| {
                AppError::not_found(
                    ErrorCode::AssetNotFound,
                    format!("Asset {asset_id} not found"),
                )
            })?;
        let entry_uuid = Uuid::parse_str(entry_id)
            .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, entry_id.as_bytes()));
        if !asset::current_asset_reference_exists_in_workspace(
            &workspace,
            asset_id,
            &BTreeMap::from([(
                form.name.to_ascii_lowercase(),
                EntryScope::Only(BTreeSet::from([entry_uuid.into()])),
            )]),
        )
        .await?
        {
            return Err(AppError::not_found(
                ErrorCode::AssetNotFound,
                format!("Asset {asset_id} not found"),
            )
            .into());
        }
        Ok(())
    }

    pub async fn delete_asset(&self, space_id: &str, asset_id: &str) -> Result<()> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_asset_id(asset_id))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let scopes = workspace
            .list_forms()
            .await?
            .into_iter()
            .map(|form| (form.name.to_ascii_lowercase(), EntryScope::AllCurrent))
            .collect();
        asset::delete_asset(
            &self.operator,
            &self.workspace_path(space_id),
            asset_id,
            &scopes,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(())
    }

    pub async fn delete_asset_with_principal(
        &self,
        space_id: &str,
        asset_id: &str,
        principal_id: Option<Uuid>,
    ) -> Result<()> {
        match principal_id {
            Some(principal_id) => {
                self.delete_asset_with_principals(space_id, asset_id, &[principal_id])
                    .await
            }
            None => self.delete_asset(space_id, asset_id).await,
        }
    }

    pub async fn delete_asset_with_principals(
        &self,
        space_id: &str,
        asset_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<()> {
        // Deletion remains a Catalog Head publication after authorization scopes are fixed.
        require_nonempty_authorized_principals(principal_ids)?;
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_asset_id(asset_id))?;
        let (state, _authorization_lease) = if principal_ids.is_empty() {
            (None, None)
        } else {
            let (state, lease) = Authorizer::new(self.operator.clone())
                .acquire_state_lease(space_id)
                .await?;
            self.require_action_for_principals_in_state(
                &state,
                asset_id,
                ResourceKind::Asset,
                Action::Delete,
                principal_ids,
            )?;
            (Some(state), Some(lease))
        };
        let scopes = if principal_ids.is_empty() {
            let workspace =
                iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id))
                    .await?;
            workspace
                .list_forms()
                .await?
                .into_iter()
                .map(|form| (form.name.to_ascii_lowercase(), EntryScope::AllCurrent))
                .collect()
        } else {
            self.authorized_form_entry_scopes_for_state(
                space_id,
                state.as_ref().expect("authorized state is present"),
                principal_ids,
            )
            .await?
        };
        asset::delete_asset(
            &self.operator,
            &self.workspace_path(space_id),
            asset_id,
            &scopes,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(())
    }

    pub async fn get_user_preferences(
        &self,
        user_id: &str,
    ) -> Result<preferences::UserPreferences> {
        preferences::get_user_preferences(&self.operator, user_id).await
    }

    pub async fn patch_user_preferences(
        &self,
        user_id: &str,
        patch: &Value,
    ) -> Result<preferences::UserPreferences> {
        ugoite_storage::verify_publication_mutation_contract(&self.operator)
            .await
            .map_err(crate::iceberg_store::storage_mutation_unavailable)?;
        preferences::patch_user_preferences(&self.operator, user_id, patch).await
    }

    pub async fn get_sql_session_count_authorized_for_principals(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<u64> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let current_authorization = self
                    .sql_session_current_execution_authorization(
                        space_id,
                        session_id,
                        principal_ids,
                        &state,
                    )
                    .await?;
                let authorization = sql_session::SqlSessionExecutionAuthorization {
                    authorization: sql_session::SqlSessionAuthorization {
                        principal_ids,
                        policy_hash: &current_authorization.policy_hash,
                    },
                    query_policy: &current_authorization.query_policy,
                };
                sql_session::get_sql_session_count_authorized_by_form(
                    &self.operator,
                    &self.workspace_path(space_id),
                    session_id,
                    authorization,
                )
                .await
            })
            .await
    }

    pub async fn get_sql_session_rows_authorized_for_principals(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let current_authorization = self
                    .sql_session_current_execution_authorization(
                        space_id,
                        session_id,
                        principal_ids,
                        &state,
                    )
                    .await?;
                let authorization = sql_session::SqlSessionExecutionAuthorization {
                    authorization: sql_session::SqlSessionAuthorization {
                        principal_ids,
                        policy_hash: &current_authorization.policy_hash,
                    },
                    query_policy: &current_authorization.query_policy,
                };
                sql_session::get_sql_session_rows_authorized_by_form(
                    &self.operator,
                    &self.workspace_path(space_id),
                    session_id,
                    authorization,
                    offset,
                    limit,
                )
                .await
            })
            .await
    }

    /// Lists Saved SQL without resource filtering for operator-local/admin
    /// tooling. Server-backed user requests use the authorized variant below.
    pub async fn list_saved_sql_operator_unscoped(&self, space_id: &str) -> Result<Vec<Value>> {
        self.validate_complete_space(space_id).await?;
        saved_sql::list_sql(
            &self.operator,
            &self.workspace_path(space_id),
            EntryScope::AllCurrent,
        )
        .await
    }

    pub async fn list_saved_sql_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Vec<Value>> {
        require_nonempty_authorized_principals(principal_ids)?;
        self.validate_complete_space(space_id).await?;
        let authorizer = Authorizer::new(self.operator.clone());
        authorizer
            .with_state_lock(space_id, |state| async move {
                let entry_scope = Self::saved_sql_entry_scope_for_state(&state, principal_ids)?;
                saved_sql::list_sql(&self.operator, &self.workspace_path(space_id), entry_scope)
                    .await
            })
            .await
    }

    pub async fn create_saved_sql(
        &self,
        space_id: &str,
        sql_id: &str,
        payload: &saved_sql::SqlPayload,
        author: &str,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_sql_id(sql_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let created = saved_sql::create_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            author,
            &integrity,
        )
        .await?;
        if let Some(revision_id) = created
            .get("revision_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            self.record_saved_sql_audit(
                space_id,
                crate::mutation_audit::SAVED_SQL_CREATED_ACTION,
                sql_id,
                &revision_id,
                &[],
                author,
            )
            .await;
        }
        Ok(created)
    }

    pub async fn get_saved_sql(&self, space_id: &str, sql_id: &str) -> Result<Value> {
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_sql_id(sql_id))?;
        saved_sql::get_sql(&self.operator, &self.workspace_path(space_id), sql_id).await
    }

    pub async fn update_saved_sql(
        &self,
        space_id: &str,
        sql_id: &str,
        payload: &saved_sql::SqlPayload,
        parent_revision_id: &str,
        author: &str,
    ) -> Result<Value> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_sql_id(sql_id))?;
        if parent_revision_id.trim().is_empty() {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "parent_revision_id must not be blank",
            )
            .into());
        }
        validate_storage_id(validate_revision_id(parent_revision_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let updated = saved_sql::update_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            parent_revision_id,
            author,
            &integrity,
        )
        .await?;
        if let Some(revision_id) = updated
            .get("revision_id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            self.record_saved_sql_audit(
                space_id,
                crate::mutation_audit::SAVED_SQL_UPDATED_ACTION,
                sql_id,
                &revision_id,
                &[],
                author,
            )
            .await;
        }
        Ok(updated)
    }

    pub async fn delete_saved_sql(&self, space_id: &str, sql_id: &str, actor: &str) -> Result<()> {
        self.ensure_mutation_admitted(space_id).await?;
        self.validate_complete_space(space_id).await?;
        validate_storage_id(validate_sql_id(sql_id))?;
        saved_sql::delete_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            actor,
        )
        .await?;
        // Evidence uses the committed tombstone revision only: a pre-commit
        // snapshot would name the wrong revision and never converge with
        // reconcile. An unreadable tombstone means storage itself failed, in
        // which case delivery would fail too; reconcile closes the gap.
        if let Some((revision_id, _, _, _)) = saved_sql::read_sql_row_for_audit(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
        )
        .await
        .ok()
        .flatten()
        {
            self.record_saved_sql_audit(
                space_id,
                crate::mutation_audit::SAVED_SQL_DELETED_ACTION,
                sql_id,
                &revision_id,
                &[],
                actor,
            )
            .await;
        }
        Ok(())
    }

    pub async fn test_storage_connection(
        &self,
        config: &space::StorageConnectionTestConfig,
    ) -> Result<Value> {
        probe_storage_connection(config).await
    }

    pub async fn test_storage_connection_payload(&self, payload: &Value) -> Result<Value> {
        let config = space::StorageConnectionTestConfig::from_payload(payload)?;
        self.test_storage_connection(&config).await
    }
}

fn require_sql_session_principals(principal_ids: &[Uuid]) -> Result<()> {
    if principal_ids.is_empty() {
        return Err(
            AppError::forbidden("SQL session requires at least one authorized principal").into(),
        );
    }
    Ok(())
}

fn sql_query_contract_error(error: SqlQueryError) -> anyhow::Error {
    AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()).into()
}

fn effective_sql_parameter_types(
    values: &serde_json::Map<String, Value>,
    declared: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>> {
    let mut types = declared.clone();
    for (name, value) in values {
        if types.contains_key(name) {
            continue;
        }
        let inferred = match value {
            Value::String(_) => "string",
            Value::Bool(_) => "boolean",
            Value::Number(number) if number.is_i64() || number.is_u64() => "long",
            Value::Number(_) => "double",
            Value::Null => {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    format!("SQL null parameter {name} requires a declared parameter type"),
                )
                .into())
            }
            Value::Array(_) | Value::Object(_) => {
                return Err(AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    format!("SQL parameter {name} has an unsupported JSON type"),
                )
                .into())
            }
        };
        types.insert(name.clone(), inferred.to_string());
    }
    Ok(types)
}

fn require_nonempty_authorized_principals(principal_ids: &[Uuid]) -> Result<()> {
    if principal_ids.is_empty() {
        return Err(
            AppError::forbidden("authorized operation requires at least one principal").into(),
        );
    }
    Ok(())
}

fn sql_session_metadata_authorization_error(error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<AppError>().is_some() {
        error
    } else {
        AppError::forbidden("SQL session execution metadata is invalid").into()
    }
}

/// Builds a sparse provider-side authorization predicate from the authoritative
/// policy state. SQL sessions require every principal to have Space-level
/// read, so only Entry policies that remove that inherited read need to be
/// carried into the frozen checkpoint policy.
fn sql_session_entry_scope(
    state: &AuthorizationState,
    principal_ids: &[Uuid],
) -> Result<index::SqlSessionEntryScope> {
    validate_sql_session_principal_access(state, principal_ids)?;
    let mut denied_entry_ids = std::collections::BTreeSet::new();
    for resource_key in state.policies.keys() {
        let Some(entry_id) = resource_key.strip_prefix("entry:") else {
            continue;
        };
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: entry_id.to_string(),
            parent: None,
        };
        let mut readable_by_every_principal = true;
        for principal_id in principal_ids {
            if !effective_actions_for_state(state, *principal_id, Some(&resource))?
                .contains(&Action::Read)
            {
                readable_by_every_principal = false;
                break;
            }
        }
        if !readable_by_every_principal {
            if denied_entry_ids.len() == index::SQL_SESSION_MAX_AUTHORIZATION_SCOPE_IDS {
                return Err(AppError::invalid_input(
                    ugoite_core::error::ErrorCode::InvalidInput,
                    "SQL session authorization scope exceeds the configured maximum",
                )
                .into());
            }
            denied_entry_ids.insert(entry_id.to_string());
        }
    }
    Ok(index::SqlSessionEntryScope::AllExcept(denied_entry_ids))
}

fn validate_sql_session_principal_access(
    state: &AuthorizationState,
    principal_ids: &[Uuid],
) -> Result<()> {
    require_sql_session_principals(principal_ids)?;
    for principal_id in principal_ids {
        if !effective_actions_for_state(state, *principal_id, None)?.contains(&Action::Read) {
            return Err(AppError::forbidden(
                "principal is not currently allowed to read this Space",
            )
            .into());
        }
    }
    Ok(())
}

fn sql_session_policy_hash(state: &AuthorizationState, principal_ids: &[Uuid]) -> Result<String> {
    require_sql_session_principals(principal_ids)?;
    let principal_ids = principal_ids
        .iter()
        .map(Uuid::to_string)
        .collect::<std::collections::BTreeSet<_>>();
    let membership_roles = principal_ids
        .iter()
        .map(|principal_id| {
            let principal_id = Uuid::parse_str(principal_id)
                .expect("principal IDs were serialized from UUID values");
            (
                principal_id.to_string(),
                state
                    .memberships
                    .get(&principal_id)
                    .map(|membership| membership.role.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let agent_grants = principal_ids
        .iter()
        .map(|principal_id| {
            let principal_id = Uuid::parse_str(principal_id)
                .expect("principal IDs were serialized from UUID values");
            (
                principal_id.to_string(),
                state.agent_grants.get(&principal_id).cloned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let entry_policies = state
        .policies
        .iter()
        .filter(|(resource_key, _)| resource_key.starts_with("entry:"))
        .map(|(resource_key, policy)| (resource_key.clone(), policy.clone()))
        .collect::<BTreeMap<_, _>>();
    let canonical = serde_json::to_vec(&json!({
        "space_uid": state.space_uid,
        "principal_ids": principal_ids,
        "membership_roles": membership_roles,
        "agent_grants": agent_grants,
        "entry_policies": entry_policies,
    }))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(canonical))))
}

fn sql_query_authorization_fingerprint(
    state: &AuthorizationState,
    principal_ids: &[Uuid],
) -> Result<String> {
    require_sql_session_principals(principal_ids)?;
    let principal_ids = principal_ids
        .iter()
        .map(Uuid::to_string)
        .collect::<BTreeSet<_>>();
    // AuthorizationState::revision is the durable monotonic revision for all
    // ACL and membership changes. Rechecking the current state on every page
    // remains mandatory; this fingerprint only detects that a continuation
    // must be restarted rather than acting as an authorization credential.
    let canonical = serde_json::to_vec(&json!({
        "space_uid": state.space_uid,
        "authorization_revision": state.revision,
        "principal_ids": principal_ids,
    }))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(canonical))))
}

fn validate_storage_id(
    result: std::result::Result<(), ugoite_domain::id::IdentifierError>,
) -> Result<()> {
    result.map_err(|error| AppError::invalid_identifier(error.to_string()).into())
}

fn map_checkpoint_error(error: anyhow::Error) -> anyhow::Error {
    if error
        .chain()
        .any(|cause| cause.to_string().contains("entry revision conflict"))
    {
        return AppError::conflict(
            ErrorCode::RevisionConflict,
            "Entry changed while the checkpoint restore was being published",
        )
        .into();
    }
    if error.chain().any(|cause| {
        cause.downcast_ref::<opendal::Error>().is_some_and(|error| {
            matches!(
                error.kind(),
                opendal::ErrorKind::AlreadyExists | opendal::ErrorKind::ConditionNotMatch
            )
        })
    }) {
        return AppError::conflict(
            ErrorCode::CheckpointAlreadyExists,
            "A checkpoint with this name already exists",
        )
        .into();
    }
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<CheckpointUnavailable>().is_some())
    {
        return AppError::not_found(
            ErrorCode::CheckpointUnavailable,
            "Requested checkpoint is unavailable",
        )
        .into();
    }
    if error
        .chain()
        .any(|cause| cause.downcast_ref::<CheckpointIntegrityError>().is_some())
    {
        return AppError::invalid_input(
            ErrorCode::CheckpointIntegrity,
            "Requested checkpoint failed integrity validation",
        )
        .into();
    }
    error
}

/// Projects stable entry identities onto pin diff changes.
///
/// Checkpoint revisions carry internal coordinates; automation needs the
/// portable Entry ID, taken from either diff side without preferring one.
fn project_pin_diff_entry_ids(diff: &mut Value) {
    if let Some(changes) = diff.get_mut("changes").and_then(Value::as_array_mut) {
        for change in changes {
            let external_id = ["to", "from"].into_iter().find_map(|side| {
                change
                    .get(side)
                    .and_then(|revision| revision.get("entry"))
                    .and_then(|entry| entry.get("external_id"))
                    .and_then(Value::as_str)
                    .filter(|external_id| !external_id.is_empty())
            });
            if let Some(external_id) = external_id {
                change["entry_id"] = Value::String(external_id.to_string());
            }
        }
    }
}

/// Top-level fields accepted by the public Space patch contract. This allow-list
/// is part of the shared validation error contract: unknown fields must be
/// reported as a typed 4xx error, never as an internal error.
pub const ALLOWED_PUBLIC_SPACE_PATCH_FIELDS: &[&str] =
    &["name", "slug", "storage_config", "settings"];

fn invalid_space_patch(code: ErrorCode, message: impl Into<String>) -> anyhow::Error {
    AppError::invalid_input(code, message).into()
}

fn unsupported_space_patch_field_error(unsupported: &[String]) -> anyhow::Error {
    AppError::invalid_input_with_detail(
        ErrorCode::UnsupportedSpacePatchField,
        format!(
            "space patch contains unsupported field(s): {}. Allowed fields: {}",
            unsupported.join(", "),
            ALLOWED_PUBLIC_SPACE_PATCH_FIELDS.join(", ")
        ),
        serde_json::json!({
            "unsupported_fields": unsupported,
            "allowed_fields": ALLOWED_PUBLIC_SPACE_PATCH_FIELDS,
        }),
    )
    .into()
}

pub fn validate_public_space_patch(patch: &Value) -> Result<()> {
    let Some(object) = patch.as_object() else {
        return Err(invalid_space_patch(
            ErrorCode::InvalidInput,
            "space patch must be a JSON object",
        ));
    };
    let mut unsupported: Vec<String> = object
        .keys()
        .filter(|key| !ALLOWED_PUBLIC_SPACE_PATCH_FIELDS.contains(&key.as_str()))
        .cloned()
        .collect();
    unsupported.sort();
    if !unsupported.is_empty() {
        return Err(unsupported_space_patch_field_error(&unsupported));
    }
    if let Some(name) = object.get("name") {
        if name.as_str().is_none_or(|value| value.trim().is_empty()) {
            return Err(invalid_space_patch(
                ErrorCode::InvalidInput,
                "space patch name must be a non-empty string",
            ));
        }
    }
    if let Some(slug) = object.get("slug") {
        let Some(slug) = slug.as_str().filter(|value| !value.trim().is_empty()) else {
            return Err(invalid_space_patch(
                ErrorCode::InvalidInput,
                "space patch slug must be a non-empty string",
            ));
        };
        validate_storage_id(validate_space_id(slug))?;
    }
    if let Some(storage_config) = object.get("storage_config") {
        if !storage_config.is_object() {
            return Err(invalid_space_patch(
                ErrorCode::InvalidInput,
                "space patch storage_config must be an object",
            ));
        }
        if let Some(uri) = storage_config.get("uri") {
            if uri.as_str().is_none_or(|value| value.trim().is_empty()) {
                return Err(invalid_space_patch(
                    ErrorCode::InvalidInput,
                    "space patch storage_config.uri must be a non-empty string",
                ));
            }
        }
    }
    if let Some(settings) = object.get("settings") {
        if !settings.is_object() {
            return Err(invalid_space_patch(
                ErrorCode::InvalidInput,
                "space patch settings must be an object",
            ));
        }
    }
    let reserved_keys: Vec<&str> = patch
        .as_object()
        .map(|object| {
            object
                .keys()
                .map(String::as_str)
                .filter(|key| MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS.contains(key))
                .collect()
        })
        .unwrap_or_default();

    let mut reserved_keys = reserved_keys;
    if let Some(settings_obj) = patch.get("settings").and_then(Value::as_object) {
        for key in settings_obj.keys().map(String::as_str) {
            if MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS.contains(&key) {
                reserved_keys.push(key);
            }
        }
    }
    reserved_keys.sort_unstable();
    reserved_keys.dedup();
    if reserved_keys.is_empty() {
        return Ok(());
    }

    Err(invalid_space_patch(
        ErrorCode::InvalidInput,
        format!(
            "space patch does not allow membership-managed settings keys: {}. Use the dedicated member commands instead.",
            reserved_keys.join(", ")
        ),
    ))
}

#[cfg(test)]
mod public_space_patch_validation_tests {
    use super::{validate_public_space_patch, ALLOWED_PUBLIC_SPACE_PATCH_FIELDS};
    use ugoite_core::error::{ErrorCode, ErrorKind};

    fn typed_error(patch: serde_json::Value) -> ugoite_core::error::AppError {
        let error = validate_public_space_patch(&patch).expect_err("patch must be rejected");
        error
            .downcast::<ugoite_core::error::AppError>()
            .expect("space patch validation must be a typed AppError")
    }

    #[test]
    fn unknown_top_level_field_is_typed_unsupported_field() {
        let error = typed_error(serde_json::json!({"unknown_field": "x"}));
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert_eq!(error.code(), ErrorCode::UnsupportedSpacePatchField);
        let detail = error.detail().expect("detail must be present");
        assert_eq!(
            detail["unsupported_fields"],
            serde_json::json!(["unknown_field"])
        );
        assert_eq!(
            detail["allowed_fields"],
            serde_json::json!(ALLOWED_PUBLIC_SPACE_PATCH_FIELDS)
        );
    }

    #[test]
    fn non_object_and_bad_shapes_are_typed_invalid_input() {
        for patch in [
            serde_json::json!("not-an-object"),
            serde_json::json!({"name": "   "}),
            serde_json::json!({"storage_config": "not-an-object"}),
            serde_json::json!({"settings": []}),
        ] {
            let error = typed_error(patch);
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
            assert_eq!(error.code(), ErrorCode::InvalidInput);
        }
    }

    #[test]
    fn valid_patch_is_accepted() {
        assert!(validate_public_space_patch(&serde_json::json!({"name": "New name"})).is_ok());
    }
}

#[cfg(test)]
mod storage_connection_tests {
    use super::*;

    #[tokio::test]
    async fn probe_accepts_memory_and_local_backends() -> Result<()> {
        let memory = probe_storage_connection(&space::StorageConnectionTestConfig {
            uri: "memory://service-probe".to_string(),
            endpoint: None,
        })
        .await?;
        assert_eq!(memory, json!({"status": "ok", "mode": "memory"}));

        let directory = tempfile::tempdir()?;
        let local = probe_storage_connection(&space::StorageConnectionTestConfig {
            uri: format!("file://{}", directory.path().display()),
            endpoint: None,
        })
        .await?;
        assert_eq!(local, json!({"status": "ok", "mode": "local"}));
        Ok(())
    }

    #[test]
    fn payload_parser_accepts_direct_and_nested_shapes() -> Result<()> {
        let direct = space::StorageConnectionTestConfig::from_payload(&json!({
            "uri": "memory://direct"
        }))?;
        assert_eq!(direct.uri, "memory://direct");

        let nested = space::StorageConnectionTestConfig::from_payload(&json!({
            "storage_config": {
                "uri": "memory://nested",
                "endpoint": null
            }
        }))?;
        assert_eq!(nested.uri, "memory://nested");
        Ok(())
    }

    #[tokio::test]
    async fn invalid_and_unsupported_configuration_is_typed() {
        let missing = space::StorageConnectionTestConfig::from_payload(&json!({}))
            .expect_err("missing URI must be rejected");
        let missing = missing
            .downcast_ref::<AppError>()
            .expect("missing URI must be an AppError");
        assert_eq!(missing.kind(), ugoite_core::error::ErrorKind::InvalidInput);
        assert_eq!(missing.code(), ErrorCode::InvalidInput);

        let unsupported = probe_storage_connection(&space::StorageConnectionTestConfig {
            uri: "ftp://example.test/data".to_string(),
            endpoint: None,
        })
        .await
        .expect_err("unsupported backend must be rejected");
        let unsupported = unsupported
            .downcast_ref::<AppError>()
            .expect("unsupported backend must be an AppError");
        assert_eq!(
            unsupported.kind(),
            ugoite_core::error::ErrorKind::Unimplemented
        );
        assert_eq!(unsupported.code(), ErrorCode::StorageMutationUnavailable);
    }

    #[tokio::test]
    async fn probe_unreachable_endpoint_maps_to_storage_connection_failure() -> Result<()> {
        let operator = Operator::new(
            opendal::services::S3::default()
                .bucket("bucket")
                .root("/")
                .region("us-east-1")
                .endpoint("http://192.0.2.1:9")
                .skip_signature(),
        )?;
        let error = probe_storage_backend(&operator, "s3", Duration::from_millis(50))
            .await
            .expect_err("the unreachable endpoint should fail");

        let app_error = error
            .downcast_ref::<AppError>()
            .expect("probe failure should preserve the storage connection app error");
        assert_eq!(app_error.code(), ErrorCode::StorageConnectionFailed);
        assert!(app_error.message().contains("storage connection failed"));
        Ok(())
    }

    #[test]
    fn endpoint_validation_rejects_blocked_hosts() {
        for endpoint in [
            "http://127.0.0.1:9000",
            "http://172.16.0.1:9000",
            "http://10.0.0.1:9000",
            "http://192.168.1.1:9000",
            "http://169.254.1.1:9000",
            "http://[::]:9000",
            "http://[::1]:9000",
            "http://[fc00::1]:9000",
            "http://[fd12:3456:789a::1]:9000",
            "http://[fe80::1]:9000",
            "http://[::ffff:127.0.0.1]:9000",
            "http://[::ffff:10.0.0.1]:9000",
            "http://[::ffff:169.254.1.1]:9000",
        ] {
            assert!(
                validate_storage_endpoint(Some(endpoint)).is_err(),
                "storage endpoint must be blocked: {endpoint}"
            );
        }
    }

    #[test]
    fn endpoint_validation_allows_public_ip_forms() -> Result<()> {
        for endpoint in [
            "http://192.0.2.1:9000",
            "http://[2001:db8::1]:9000",
            "https://s3.example.test",
        ] {
            assert_eq!(validate_storage_endpoint(Some(endpoint))?, Some(endpoint));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{IcebergWorkspace, PublicationContext, WriteConfig};
    use ugoite_domain::identity::{
        Membership, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
    };
    use ugoite_storage::SpaceCatalogStore;

    #[test]
    fn saved_sql_scope_accepts_more_than_normal_read_rows_of_denials() -> Result<()> {
        let principal_id = Uuid::now_v7();
        let mut state = AuthorizationState {
            schema_version: 1,
            space_uid: Uuid::now_v7(),
            principals: [(
                principal_id,
                SpacePrincipal {
                    principal_id,
                    kind: PrincipalKind::Human,
                    display_name: "Owner".to_string(),
                    state: PrincipalState::Active,
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                },
            )]
            .into_iter()
            .collect(),
            memberships: [(
                principal_id,
                Membership {
                    principal_id,
                    role: SpaceRole::Viewer,
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                },
            )]
            .into_iter()
            .collect(),
            policies: BTreeMap::new(),
            policy_history: BTreeMap::new(),
            agents: BTreeMap::new(),
            agent_grants: BTreeMap::new(),
            principal_lifecycle_epochs: [(principal_id, 1)].into_iter().collect(),
            recovery_fences: BTreeMap::new(),
            human_approvals: BTreeMap::new(),
            human_approval_audit_outbox: BTreeMap::new(),
            revision: 1,
        };
        for index in 0..=crate::MAX_NORMAL_READ_ROWS {
            state.policies.insert(
                format!("saved_sql:{index}"),
                ugoite_domain::identity::AccessPolicy {
                    policy_id: Uuid::now_v7(),
                    inherit_space_role: false,
                    grants: Vec::new(),
                },
            );
        }
        let scope = UgoiteService::saved_sql_entry_scope_for_state(&state, &[principal_id])?;
        match scope {
            EntryScope::AllExcept(ids) => assert_eq!(ids.len(), crate::MAX_NORMAL_READ_ROWS + 1),
            other => panic!("expected a provider-side exclusion scope, got {other:?}"),
        }
        let error = UgoiteService::saved_sql_entry_scope_for_state(&state, &[])
            .expect_err("an empty authorized principal set must fail closed");
        assert!(error.to_string().contains("at least one principal"));
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_principal_space_creation_preserves_slug_uniqueness() -> Result<()> {
        let first_service = UgoiteService::new("memory://service-space-create-race")?;
        let second_service = UgoiteService::new("memory://service-space-create-race")?;
        let first =
            first_service.create_space_for_principal("race-space", Uuid::now_v7(), "First owner");
        let second =
            second_service.create_space_for_principal("race-space", Uuid::now_v7(), "Second owner");
        let (first_result, second_result) = tokio::join!(first, second);
        assert!(first_result.is_ok() ^ second_result.is_ok());
        let mut matching_spaces = 0;
        for space_id in first_service.list_space_ids().await? {
            if first_service.get_space(&space_id).await?["slug"] == "race-space" {
                matching_spaces += 1;
            }
        }
        assert_eq!(matching_spaces, 1);
        Ok(())
    }

    #[tokio::test]
    async fn non_local_space_mutations_fail_closed_before_storage_writes() -> Result<()> {
        let operator = Operator::new(
            opendal::services::S3::default()
                .bucket("ugoite-test-bucket")
                .region("us-east-1")
                .endpoint("http://127.0.0.1:1"),
        )?;
        let service = UgoiteService::from_operator(operator, "s3://ugoite-test-bucket/space");
        let assert_unavailable = |error: anyhow::Error| {
            let message = error.to_string();
            let app_error = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<ugoite_core::error::AppError>())
                .unwrap_or_else(|| {
                    panic!("non-local mutation returned an untyped error: {message}")
                });
            assert_eq!(
                app_error.code(),
                ugoite_core::error::ErrorCode::StorageMutationUnavailable
            );
            assert!(app_error.message().contains("authoritative"));
        };

        assert_unavailable(
            service
                .create_space("remote-space")
                .await
                .expect_err("create_space must fail before any remote write"),
        );
        assert_unavailable(
            service
                .create_operator_space("remote-space")
                .await
                .expect_err("create_operator_space must fail before any remote write"),
        );
        assert_unavailable(
            service
                .create_space_for_principal("remote-space", Uuid::now_v7(), "Owner")
                .await
                .expect_err("principal Space creation must fail before any remote write"),
        );
        assert_unavailable(
            service
                .patch_space("remote-space", &serde_json::json!({"name": "patched"}))
                .await
                .expect_err("patch_space must fail before any remote write"),
        );
        assert_unavailable(
            service
                .upsert_form("remote-space", &serde_json::json!({"name": "Entry"}))
                .await
                .expect_err("Form mutation must fail before any remote write"),
        );
        assert_unavailable(
            service
                .save_asset("remote-space", "asset.txt", b"asset")
                .await
                .expect_err("Asset mutation must fail before any remote write"),
        );
        assert_unavailable(
            service
                .create_structured_entry_with_receipt(
                    "remote-space",
                    "entry-1",
                    "Entry".into(),
                    Vec::new(),
                    std::collections::BTreeMap::new(),
                    std::collections::BTreeMap::new(),
                    "author",
                )
                .await
                .expect_err("Entry mutation must fail before any remote write"),
        );
        assert_unavailable(
            crate::entry::append_revision_batch_for_form(
                service.operator(),
                "spaces/remote-space",
                "Entry",
                &[],
            )
            .await
            .expect_err("low-level Entry writer must fail before any remote write"),
        );
        assert_unavailable(
            crate::form::upsert_form(
                service.operator(),
                "spaces/remote-space",
                &serde_json::json!({"name": "Entry"}),
            )
            .await
            .expect_err("low-level Form writer must fail before any remote write"),
        );
        assert_unavailable(
            crate::asset::save_asset(
                service.operator(),
                "spaces/remote-space",
                "asset.txt",
                b"asset",
            )
            .await
            .expect_err("low-level Asset writer must fail before any remote write"),
        );
        assert_unavailable(
            crate::audit::append_audit_event(
                service.operator(),
                "remote-space",
                &serde_json::json!({
                    "action": "test",
                    "subject_principal_id": Uuid::now_v7().to_string(),
                }),
                None,
            )
            .await
            .expect_err("low-level audit writer must fail before any remote write"),
        );
        let principal_ids = [Uuid::now_v7()];
        let readable_entries_by_form = BTreeMap::new();
        let sql_authorization = crate::sql_session::SqlSessionCreateAuthorization {
            authorization: crate::sql_session::SqlSessionAuthorization {
                principal_ids: &principal_ids,
                policy_hash: "policy",
            },
            readable_entries_by_form: &readable_entries_by_form,
        };
        assert_unavailable(
            crate::sql_session::create_sql_session_authorized_for_principals_by_form_with_parameters(
                service.operator(),
                "spaces/remote-space",
                "SELECT * FROM Entry",
                serde_json::Map::new(),
                BTreeMap::new(),
                sql_authorization,
                EntryScope::AllCurrent,
            )
            .await
            .expect_err("low-level SQL-session writer must fail before any remote write"),
        );
        assert_unavailable(
            crate::preferences::patch_user_preferences(
                service.operator(),
                "remote-user",
                &serde_json::json!({"locale": "ja"}),
            )
            .await
            .expect_err("low-level preferences writer must fail before any remote write"),
        );
        assert_unavailable(
            crate::integrity::load_hmac_material(service.operator(), "remote-space")
                .await
                .expect_err("HMAC initialization must fail before any remote write"),
        );
        let integrity_result =
            crate::integrity::RealIntegrityProvider::from_space(service.operator(), "remote-space")
                .await;
        assert_unavailable(
            integrity_result
                .err()
                .expect("integrity initialization must fail before any remote write"),
        );
        let sample_options = crate::sample_data::SampleDataOptions {
            space_id: "remote-space".to_string(),
            scenario: crate::sample_data::DEFAULT_SCENARIO.to_string(),
            entry_count: 1,
            seed: Some(1),
            owner_display_name: None,
        };
        assert_unavailable(
            crate::sample_data::create_sample_space_job(
                service.operator(),
                service.root_uri(),
                &sample_options,
            )
            .await
            .expect_err("sample-job creation must fail before any remote write"),
        );
        let workspace = IcebergWorkspace::open_space(
            SpaceCatalogStore::new(service.operator().clone(), "spaces/remote-space")?,
            ugoite_domain::id::SpaceId::from(Uuid::now_v7()),
            WriteConfig::default(),
        )
        .await?;
        assert_unavailable(
            workspace
                .commit(PublicationContext::with_command_digest(
                    "remote-command",
                    "test.remote",
                    "remote-digest",
                ))
                .expect_err("coordinator creation must fail before any remote write"),
        );
        Ok(())
    }

    #[tokio::test]
    async fn slug_claim_recovery_reuses_uuid_after_bootstrap_interruption() -> Result<()> {
        let service = UgoiteService::new("memory://slug-claim-recovery")?;
        let space_uid = Uuid::now_v7();
        service
            .claim_space_slug("recoverable", &space_uid.to_string())
            .await?;
        service
            .expire_space_slug_claim_for_test("recoverable")
            .await?;

        let recovered = service
            .recover_space_id_by_slug("recoverable")
            .await?
            .context("claim-backed Space should be recoverable")?;
        assert_eq!(recovered, space_uid.to_string());
        assert_eq!(service.get_space(&recovered).await?["slug"], "recoverable");
        assert!(
            service
                .operator
                .exists("spaces/.ugoite-space-slug-claims/recoverable.committed")
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn principal_space_claim_recovery_restores_owner_after_scaffold_crash() -> Result<()> {
        let service = UgoiteService::new("memory://principal-slug-claim-recovery")?;
        let space_uid = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let space_name = "復旧ノート 📝";
        service
            .claim_space_slug_with_owner_and_name(
                "principal-recovery",
                &space_uid.to_string(),
                Some((principal_id, "Recovered owner".to_string())),
                space_name,
            )
            .await?;
        space::create_space_with_identity_and_name(
            service.operator(),
            space_uid,
            "principal-recovery",
            space_name,
            service.root_uri(),
        )
        .await?;
        service
            .operator
            .delete(&format!("spaces/{space_uid}/meta.json"))
            .await?;
        service
            .expire_space_slug_claim_for_test("principal-recovery")
            .await?;

        assert_eq!(
            service
                .recover_space_id_by_slug("principal-recovery")
                .await?,
            Some(space_uid.to_string())
        );
        assert_eq!(
            service.get_space(&space_uid.to_string()).await?["name"],
            space_name
        );
        let state = Authorizer::new(service.operator().clone())
            .state(&space_uid.to_string())
            .await?;
        assert!(state.principals.contains_key(&principal_id));
        assert_eq!(
            state
                .memberships
                .get(&principal_id)
                .map(|membership| membership.role.clone()),
            Some(SpaceRole::Owner)
        );
        Ok(())
    }

    #[tokio::test]
    async fn live_space_slug_claim_is_not_recovered_or_released() -> Result<()> {
        let service = UgoiteService::new("memory://live-space-slug-claim")?;
        service.claim_space_slug("live-claim", "live-claim").await?;

        let incomplete_uid = Uuid::now_v7();
        service
            .claim_space_slug("live-incomplete", &incomplete_uid.to_string())
            .await?;
        space::create_space_with_identity(
            service.operator(),
            incomplete_uid,
            "live-incomplete",
            service.root_uri(),
        )
        .await?;
        service
            .operator
            .delete(&format!("spaces/{incomplete_uid}/settings.json"))
            .await?;

        service.recover_pending_space_claims().await?;
        assert_eq!(service.recover_space_id_by_slug("live-claim").await?, None);
        assert!(service.list_space_ids().await?.is_empty());
        assert!(
            service
                .operator
                .exists("spaces/.ugoite-space-slug-claims/live-claim.json")
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn startup_claim_recovery_precedes_strict_space_listing() -> Result<()> {
        let service = UgoiteService::new("memory://startup-space-claim-recovery")?;
        let space_uid = Uuid::now_v7();
        service
            .claim_space_slug("startup-recover", &space_uid.to_string())
            .await?;
        space::create_space_with_identity(
            service.operator(),
            space_uid,
            "startup-recover",
            service.root_uri(),
        )
        .await?;
        service
            .operator
            .delete(&format!("spaces/{space_uid}/settings.json"))
            .await?;
        service
            .expire_space_slug_claim_for_test("startup-recover")
            .await?;

        service.recover_pending_space_claims().await?;
        let space_ids = service.list_space_ids().await?;
        assert_eq!(space_ids, vec![space_uid.to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn live_mismatched_claim_does_not_hide_complete_space() -> Result<()> {
        let service = UgoiteService::new("memory://live-mismatched-space-claim")?;
        let space_uid = service.create_operator_space("actual-slug").await?;
        service
            .claim_space_slug("different-slug", &space_uid.to_string())
            .await?;

        assert_eq!(service.list_space_ids().await?, vec![space_uid.to_string()]);
        Ok(())
    }

    #[tokio::test]
    async fn unclaimed_incomplete_space_is_not_hidden_by_claim_recovery() -> Result<()> {
        let service = UgoiteService::new("memory://unclaimed-incomplete-space")?;
        let space_uid = Uuid::now_v7();
        space::create_space_with_identity(
            service.operator(),
            space_uid,
            "unclaimed-corrupt",
            service.root_uri(),
        )
        .await?;
        service
            .operator
            .delete(&format!("spaces/{space_uid}/settings.json"))
            .await?;

        service.recover_pending_space_claims().await?;
        let error = service
            .list_space_ids()
            .await
            .expect_err("unclaimed incomplete Space must remain visible as corruption");
        assert!(error.to_string().contains("incomplete Space bootstrap"));
        Ok(())
    }

    #[tokio::test]
    async fn non_v7_identity_claim_is_rejected_before_recovery() -> Result<()> {
        let service = UgoiteService::new("memory://slug-claim-v4")?;
        let space_id = Uuid::new_v4().to_string();
        service
            .claim_space_slug("invalid-identity", &space_id)
            .await?;

        let error = service
            .recover_space_id_by_slug("invalid-identity")
            .await
            .expect_err("UUIDv4 identity claims must fail closed");
        assert!(error.to_string().contains("must use a UUIDv7"));
        Ok(())
    }

    #[tokio::test]
    async fn space_slug_rename_claims_new_slug_and_releases_old_slug() -> Result<()> {
        let service = UgoiteService::new("memory://slug-rename")?;
        let space_uid = service.create_operator_space("before-rename").await?;
        let space_id = space_uid.to_string();

        service
            .patch_space(&space_id, &json!({"slug": "after-rename"}))
            .await?;

        assert_eq!(
            service.space_id_by_slug("after-rename").await?,
            Some(space_id.clone())
        );
        assert_eq!(service.space_id_by_slug("before-rename").await?, None);
        assert!(
            !service
                .operator
                .exists("spaces/.ugoite-space-slug-claims/before-rename.json")
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn pending_rename_claim_before_metadata_swap_is_released() -> Result<()> {
        let service = UgoiteService::new("memory://slug-rename-recovery-window")?;
        let space_uid = service.create_operator_space("rename-before").await?;
        service
            .claim_space_slug("rename-after", &space_uid.to_string())
            .await?;
        service
            .expire_space_slug_claim_for_test("rename-after")
            .await?;

        assert_eq!(
            service.recover_space_id_by_slug("rename-after").await?,
            None
        );
        assert_eq!(
            service.space_id_by_slug("rename-before").await?,
            Some(space_uid.to_string())
        );
        assert!(
            !service
                .operator
                .exists("spaces/.ugoite-space-slug-claims/rename-after.json")
                .await?
        );
        Ok(())
    }

    #[tokio::test]
    async fn legacy_slug_space_rename_keeps_legacy_directory_identity() -> Result<()> {
        let service = UgoiteService::new("memory://legacy-slug-rename")?;
        service.create_space("legacy-before").await?;

        service
            .patch_space("legacy-before", &json!({"slug": "legacy-after"}))
            .await?;

        assert_eq!(
            service.space_id_by_slug("legacy-after").await?,
            Some("legacy-before".to_string())
        );
        assert_eq!(service.space_id_by_slug("legacy-before").await?, None);
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_space_renames_have_one_metadata_winner() -> Result<()> {
        let first_service = UgoiteService::new("memory://service-space-rename-race")?;
        let second_service = UgoiteService::new("memory://service-space-rename-race")?;
        let space_id = first_service.create_operator_space("rename-source").await?;
        let space_id = space_id.to_string();

        let left_patch = json!({"slug": "rename-left"});
        let right_patch = json!({"slug": "rename-right"});
        let first = space::patch_space_if_slug(
            first_service.operator(),
            &space_id,
            &left_patch,
            "rename-source",
        );
        let second = space::patch_space_if_slug(
            second_service.operator(),
            &space_id,
            &right_patch,
            "rename-source",
        );
        let (first_result, second_result) = tokio::join!(first, second);
        assert!(first_result.is_ok() ^ second_result.is_ok());

        let final_slug = first_service.get_space(&space_id).await?["slug"]
            .as_str()
            .context("winning Space slug")?
            .to_string();
        assert!(matches!(
            final_slug.as_str(),
            "rename-left" | "rename-right"
        ));
        Ok(())
    }

    #[tokio::test]
    async fn duplicate_space_uids_are_rejected_during_slug_scan() -> Result<()> {
        let operator = operator_from_uri("memory://service-duplicate-space-uid")?;
        space::create_space(&operator, "first-space", "/tmp").await?;
        space::create_space(&operator, "second-space", "/tmp").await?;
        let first_meta: Value = serde_json::from_slice(
            &operator
                .read("spaces/first-space/meta.json")
                .await?
                .to_vec(),
        )?;
        let mut second_meta: Value = serde_json::from_slice(
            &operator
                .read("spaces/second-space/meta.json")
                .await?
                .to_vec(),
        )?;
        second_meta["space_uid"] = first_meta["space_uid"].clone();
        operator
            .write(
                "spaces/second-space/meta.json",
                serde_json::to_vec(&second_meta)?,
            )
            .await?;
        let service =
            UgoiteService::from_operator(operator, "memory://service-duplicate-space-uid");
        let error = service
            .space_id_by_slug("does-not-exist")
            .await
            .expect_err("duplicate immutable Space UIDs must fail closed");
        assert!(error.to_string().contains("duplicate immutable space_uid"));
        Ok(())
    }

    #[tokio::test]
    async fn ensure_operator_space_is_idempotent_for_same_slug() -> Result<()> {
        let service = UgoiteService::new("memory://ensure-operator-space")?;
        let first = service.ensure_operator_space("retry-space").await?;
        assert!(first.created());
        let first_id = first.space_id();
        let second = service.ensure_operator_space("retry-space").await?;
        assert!(!second.created());
        assert_eq!(second.space_id(), first_id);
        // Strict primitive stays fail-closed on retry.
        let error = service
            .create_operator_space("retry-space")
            .await
            .expect_err("strict create must still conflict");
        assert_eq!(
            error
                .downcast_ref::<ugoite_core::error::AppError>()
                .expect("typed conflict")
                .code_str(),
            "SPACE_ALREADY_EXISTS"
        );
        Ok(())
    }

    #[tokio::test]
    async fn ensure_operator_space_stays_fail_closed() -> Result<()> {
        // Live pending claim: another writer may still be bootstrapping.
        let service = UgoiteService::new("memory://ensure-fail-closed-pending")?;
        service
            .claim_space_slug("pending-space", &Uuid::now_v7().to_string())
            .await?;
        let error = service
            .ensure_operator_space("pending-space")
            .await
            .expect_err("live pending claim must conflict");
        assert_eq!(
            error
                .downcast_ref::<ugoite_core::error::AppError>()
                .expect("typed conflict")
                .code_str(),
            "SPACE_ALREADY_EXISTS"
        );

        // Account-owned claim is never operator-local.
        let service = UgoiteService::new("memory://ensure-fail-closed-owner")?;
        service
            .claim_space_slug_with_owner_and_name(
                "owned-space",
                &Uuid::now_v7().to_string(),
                Some((Uuid::now_v7(), "Owner".to_string())),
                "owned-space",
            )
            .await?;
        let error = service
            .ensure_operator_space("owned-space")
            .await
            .expect_err("account-owned claim must conflict without mutating state");
        assert_eq!(
            error
                .downcast_ref::<ugoite_core::error::AppError>()
                .expect("typed conflict")
                .code_str(),
            "SPACE_ALREADY_EXISTS"
        );

        // Slug/immutable-ID mismatch stays a conflict.
        let service = UgoiteService::new("memory://ensure-fail-closed-mismatch")?;
        let space_uid = service.create_operator_space("actual-slug").await?;
        service
            .claim_space_slug("different-slug", &space_uid.to_string())
            .await?;
        let error = service
            .ensure_operator_space("different-slug")
            .await
            .expect_err("mismatched claim must conflict");
        assert_eq!(
            error
                .downcast_ref::<ugoite_core::error::AppError>()
                .expect("typed conflict")
                .code_str(),
            "SPACE_ALREADY_EXISTS"
        );

        // Broken bootstrap surfaces the validation error, never Existing.
        let service = UgoiteService::new("memory://ensure-fail-closed-broken")?;
        let space_uid = service
            .ensure_operator_space("broken-space")
            .await?
            .space_id();
        service
            .operator
            .delete(&format!("spaces/{space_uid}/settings.json"))
            .await?;
        service
            .ensure_operator_space("broken-space")
            .await
            .expect_err("broken bootstrap must not converge to Existing");

        // Legacy non-UUID directories are not operator idempotency targets.
        let service = UgoiteService::new("memory://ensure-fail-closed-legacy")?;
        space::create_space(service.operator(), "legacy-space", service.root_uri()).await?;
        let error = service
            .ensure_operator_space("legacy-space")
            .await
            .expect_err("legacy ID must conflict");
        assert_eq!(
            error
                .downcast_ref::<ugoite_core::error::AppError>()
                .expect("typed conflict")
                .code_str(),
            "SPACE_ALREADY_EXISTS"
        );
        Ok(())
    }

    fn batch_create(entry_id: &str) -> ApplyOperation {
        ApplyOperation::Create {
            id: Some(entry_id.to_string()),
            form: "Entry".to_string(),
            tags: Vec::new(),
            fields: [("Body".to_string(), Value::String("content".to_string()))]
                .into_iter()
                .collect(),
            extra_attributes: Default::default(),
        }
    }

    async fn batch_test_space(slug_suffix: &str) -> anyhow::Result<(UgoiteService, String, Uuid)> {
        let service = UgoiteService::new(format!("memory://batch-safety-{slug_suffix}"))?;
        let principal = Uuid::now_v7();
        let space_id = service
            .create_space_for_principal(&format!("batch-{slug_suffix}"), principal, "Owner")
            .await?
            .to_string();
        Ok((service, space_id, principal))
    }

    #[tokio::test]
    async fn batch_rejects_mixed_remove_before_first_write() -> anyhow::Result<()> {
        let (service, space_id, principal) = batch_test_space("mixed-remove").await?;
        // A Remove mixed with any other operation is rejected pre-persistence
        // (mirrors the server 422 INVALID_INPUT), so no partial mutation.
        let error = service
            .apply_operations(
                &space_id,
                vec![
                    batch_create("batch-entry-1"),
                    ApplyOperation::Remove {
                        id: "batch-entry-1".to_string(),
                    },
                ],
                &principal.to_string(),
                &[principal],
                Some("run-mixed-remove"),
                None,
            )
            .await
            .expect_err("mixed remove must be rejected pre-persistence");
        assert_eq!(
            error
                .downcast_ref::<AppError>()
                .expect("typed reject")
                .code(),
            ErrorCode::InvalidInput
        );
        let missing = service
            .entry_history(&space_id, "batch-entry-1")
            .await
            .expect_err("rejected batch must leave no partial mutation");
        assert_eq!(
            missing
                .downcast_ref::<AppError>()
                .expect("typed missing")
                .code(),
            ErrorCode::EntryNotFound
        );
        Ok(())
    }

    #[tokio::test]
    async fn batch_rejects_empty_and_bad_version_token_pre_write() -> anyhow::Result<()> {
        let (service, space_id, principal) = batch_test_space("pre-validate").await?;
        let error = service
            .apply_operations(
                &space_id,
                vec![],
                &principal.to_string(),
                &[principal],
                Some("run-empty"),
                None,
            )
            .await
            .expect_err("empty batch must be rejected");
        assert_eq!(
            error
                .downcast_ref::<AppError>()
                .expect("typed reject")
                .code(),
            ErrorCode::InvalidInput
        );
        // A blank version token fails identifier pre-validation before the
        // leading Create writes, so the batch leaves nothing behind.
        let error = service
            .apply_operations(
                &space_id,
                vec![
                    batch_create("batch-entry-2"),
                    ApplyOperation::Update {
                        id: "batch-entry-2".to_string(),
                        version_token: String::new(),
                        form: Some("Entry".to_string()),
                        tags: Some(Vec::new()),
                        fields: [("Body".to_string(), Value::String("content".to_string()))]
                            .into_iter()
                            .collect(),
                        extra_attributes: Default::default(),
                    },
                ],
                &principal.to_string(),
                &[principal],
                Some("run-bad-token"),
                None,
            )
            .await
            .expect_err("bad version token must be rejected pre-persistence");
        assert!(
            error.downcast_ref::<AppError>().is_some(),
            "reject stays typed: {error:#}"
        );
        let missing = service
            .entry_history(&space_id, "batch-entry-2")
            .await
            .expect_err("rejected batch must leave no partial mutation");
        assert_eq!(
            missing
                .downcast_ref::<AppError>()
                .expect("typed missing")
                .code(),
            ErrorCode::EntryNotFound
        );
        Ok(())
    }

    #[tokio::test]
    async fn batch_run_id_groups_and_undo_run_resumes_idempotently() -> anyhow::Result<()> {
        // run_id is grouping metadata, not an idempotency key: repeating a
        // batch appends new Changes under the same Run. The idempotent path
        // for a repeated Run is undo_run, which resumes (second call reverts
        // zero Changes).
        let (service, space_id, principal) = batch_test_space("run-group").await?;
        for entry_id in ["batch-a", "batch-b"] {
            service
                .apply_operations(
                    &space_id,
                    vec![batch_create(entry_id)],
                    &principal.to_string(),
                    &[principal],
                    Some("run-batch-undo"),
                    Some("batch message"),
                )
                .await?;
        }
        let undone = service
            .undo_run(&space_id, "run-batch-undo", &principal.to_string())
            .await?;
        assert_eq!(
            undone.get("reverted_change_count").and_then(Value::as_u64),
            Some(2)
        );
        let resumed = service
            .undo_run(&space_id, "run-batch-undo", &principal.to_string())
            .await?;
        assert_eq!(
            resumed.get("reverted_change_count").and_then(Value::as_u64),
            Some(0)
        );
        Ok(())
    }
}
