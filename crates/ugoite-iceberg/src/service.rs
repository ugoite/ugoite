use anyhow::{anyhow, bail, Result};
use opendal::Operator;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::Duration;
use tokio::sync::{Mutex as AsyncMutex, Notify};
use uuid::Uuid;

use crate::integrity::RealIntegrityProvider;
use crate::{
    asset,
    authorization::{
        effective_actions_for_state, AuthorizationState, Authorizer, ResourceKind, ResourceRef,
    },
    entry, form, iceberg_store, index, preferences, saved_sql, search, space, sql_session,
};
use crate::{CheckpointIntegrityError, CheckpointUnavailable, SpaceCheckpoint};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use ugoite_domain::id::{
    validate_asset_id, validate_checkpoint_name, validate_entry_id, validate_form_name,
    validate_revision_id, validate_space_id, validate_sql_id, validate_sql_session_id, FormId,
};
use ugoite_domain::identity::Action;
use ugoite_storage::operator_from_uri;

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

const ASSET_TEXT_REFRESH_DEBOUNCE: Duration = Duration::from_millis(250);
const ASSET_TEXT_REFRESH_OPERATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_BACKGROUND_REFRESH_WORKERS: usize = 1024;
const MAX_BACKGROUND_REFRESH_RETRIES: usize = 8;
const MAX_AUTHORIZED_SCOPE_FORMS: usize = 100_000;
const MAX_AUTHORIZED_SCOPE_FORM_DEFINITION_BYTES: usize = 256 * 1024 * 1024;

impl UgoiteService {
    pub fn new(root_uri: impl Into<String>) -> Result<Self> {
        let root_uri = root_uri.into();
        let operator = operator_from_uri(&root_uri)?;
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
                        let shared = matches!(op.info().scheme(), "s3" | "gcs" | "oss" | "azdls");
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

    pub async fn create_space(&self, space_id: &str) -> Result<()> {
        let creation_lock = SPACE_CREATION_SERIALIZER
            .get_or_init(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _creation_guard = creation_lock.lock().await;
        validate_storage_id(validate_space_id(space_id))?;
        space::create_space(&self.operator, space_id, &self.root_uri).await
    }

    /// Creates an operator-local Space with an immutable UUIDv7 directory and
    /// no application principal. A node must explicitly claim it before remote use.
    pub async fn create_operator_space(&self, slug: &str) -> Result<Uuid> {
        let creation_lock = SPACE_CREATION_SERIALIZER
            .get_or_init(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let _creation_guard = creation_lock.lock().await;
        validate_storage_id(validate_space_id(slug))?;
        if self.space_id_by_slug(slug).await?.is_some() {
            return Err(AppError::conflict(
                ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                format!("Space slug already exists: {slug}"),
            )
            .into());
        }
        let space_id = Uuid::now_v7();
        space::create_space_with_identity(&self.operator, space_id, slug, &self.root_uri).await?;
        Ok(space_id)
    }

    pub async fn create_space_for_principal(
        &self,
        slug: &str,
        principal_id: Uuid,
        display_name: &str,
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
        if self.space_id_by_slug(slug).await?.is_some() {
            return Err(AppError::conflict(
                ugoite_core::error::ErrorCode::SpaceAlreadyExists,
                format!("Space slug already exists: {slug}"),
            )
            .into());
        }
        let space_uid = Uuid::now_v7();
        let space_id = space_uid.to_string();
        space::create_space_with_identity(&self.operator, space_uid, slug, &self.root_uri).await?;
        Authorizer::new(self.operator.clone())
            .initialize_owner(&space_id, space_uid, principal_id, display_name)
            .await?;
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
        space::list_spaces(&self.operator).await
    }

    pub async fn space_id_by_slug(&self, slug: &str) -> Result<Option<String>> {
        let mut seen_uids = BTreeMap::<Uuid, String>::new();
        let mut matched = None;
        for space_id in self.list_space_ids().await? {
            let meta = self.get_space(&space_id).await?;
            let space_uid = meta
                .get("space_uid")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Space is missing immutable space_uid"))
                .and_then(|value| Uuid::parse_str(value).map_err(anyhow::Error::from))?;
            if let Some(previous_id) = seen_uids.insert(space_uid, space_id.clone()) {
                bail!(
                    "duplicate immutable space_uid {space_uid} is used by Spaces {previous_id} and {space_id}"
                );
            }
            if meta.get("slug").and_then(Value::as_str) == Some(slug) {
                if matched.replace(space_id).is_some() {
                    bail!("Space slug is not unique: {slug}");
                }
            }
        }
        Ok(matched)
    }

    pub async fn get_space(&self, space_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        space::get_space_raw(&self.operator, space_id).await
    }

    /// Returns read-only Catalog Head and Iceberg metadata evidence for one
    /// Space. Checkpoint names are caller-supplied because listing storage is
    /// not a source of Catalog or orphan authority.
    pub async fn space_health(&self, space_id: &str, checkpoint_names: &[String]) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        Ok(serde_json::to_value(
            workspace.health_report(checkpoint_names).await?,
        )?)
    }

    pub async fn create_named_checkpoint(
        &self,
        space_id: &str,
        checkpoint_name: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_checkpoint_name(checkpoint_name))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let checkpoint = workspace
            .capture_checkpoint()
            .await
            .map_err(map_checkpoint_error)?;
        workspace
            .save_checkpoint(checkpoint_name, &checkpoint)
            .await
            .map_err(map_checkpoint_error)?;
        Ok(json!({
            "name": checkpoint_name,
            "space_id": checkpoint.space_id,
            "catalog_generation": checkpoint.catalog_generation,
            "coordinate_checksum": checkpoint.coordinate_checksum,
        }))
    }

    pub async fn patch_space(&self, space_id: &str, patch: &Value) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_public_space_patch(patch)?;
        space::patch_space(&self.operator, space_id, patch).await
    }

    pub async fn ensure_space(&self, space_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        space::get_space(&self.operator, space_id).await.map(|_| ())
    }

    pub async fn list_forms(&self, space_id: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        form::list_forms(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_form(&self, space_id: &str, form_name: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_form_name(form_name))?;
        form::get_form(&self.operator, &self.workspace_path(space_id), form_name).await
    }

    pub async fn upsert_form(&self, space_id: &str, form_def: &Value) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        form::upsert_form(&self.operator, &self.workspace_path(space_id), form_def).await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(())
    }

    pub async fn create_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let workspace = self.workspace_path(space_id);
        entry::create_entry(
            &self.operator,
            &workspace,
            entry_id,
            markdown,
            author,
            &integrity,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        let result = entry::get_entry(&self.operator, &workspace, entry_id).await?;
        Ok(result)
    }

    pub async fn create_entry_authorized(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
        principal_id: Uuid,
    ) -> Result<Value> {
        self.create_entry_authorized_for_principals(
            space_id,
            entry_id,
            markdown,
            author,
            &[principal_id],
        )
        .await
    }

    pub async fn create_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let scopes = self
            .authorized_form_entry_scopes_for_principals(space_id, principal_ids)
            .await?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let workspace = self.workspace_path(space_id);
        entry::create_entry_with_scopes(
            &self.operator,
            &workspace,
            entry_id,
            markdown,
            author,
            &integrity,
            Some(&scopes),
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        let result = entry::get_entry(&self.operator, &workspace, entry_id).await?;
        Ok(result)
    }

    pub async fn list_entries(&self, space_id: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        entry::list_entries(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn get_entry(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::get_entry(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn get_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let scopes = self
            .authorized_form_entry_scopes_for_principals(space_id, principal_ids)
            .await?;
        entry::get_entry_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            &scopes,
        )
        .await
    }

    pub async fn update_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
    ) -> Result<Value> {
        self.update_entry_authorized_for_principals(
            space_id,
            entry_id,
            markdown,
            parent_revision_id,
            author,
            &[],
        )
        .await
    }

    pub async fn update_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        markdown: &str,
        parent_revision_id: Option<&str>,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        if let Some(parent_revision_id) = parent_revision_id {
            validate_storage_id(validate_revision_id(parent_revision_id))?;
        }
        self.require_entry_action_for_principals(space_id, entry_id, Action::Update, principal_ids)
            .await?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let scopes = if principal_ids.is_empty() {
            None
        } else {
            Some(
                self.authorized_form_entry_scopes_for_principals(space_id, principal_ids)
                    .await?,
            )
        };
        let result = entry::update_entry_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            markdown,
            parent_revision_id,
            author,
            &integrity,
            scopes.as_ref(),
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(result)
    }

    pub async fn delete_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        hard_delete: bool,
        actor: &str,
    ) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::delete_entry(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            hard_delete,
            actor,
        )
        .await?;
        self.schedule_asset_text_refresh(space_id);
        Ok(())
    }

    pub async fn entry_history(&self, space_id: &str, entry_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        entry::get_entry_history(&self.operator, &self.workspace_path(space_id), entry_id).await
    }

    pub async fn entry_history_at_checkpoint(
        &self,
        space_id: &str,
        entry_id: &str,
        checkpoint_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let checkpoint = self
            .load_named_checkpoint(space_id, checkpoint_name)
            .await?;
        let scopes = self
            .checkpoint_form_scopes_for_principals(space_id, principal_ids)
            .await?;
        entry::get_entry_history_at_checkpoint(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            &checkpoint,
            scopes.as_ref(),
        )
        .await
        .map_err(map_checkpoint_error)
    }

    pub async fn entry_revision(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
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

    pub async fn entry_revision_at_checkpoint(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        checkpoint_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        let checkpoint = self
            .load_named_checkpoint(space_id, checkpoint_name)
            .await?;
        let scopes = self
            .checkpoint_form_scopes_for_principals(space_id, principal_ids)
            .await?;
        entry::get_entry_revision_at_checkpoint(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
            &checkpoint,
            scopes.as_ref(),
        )
        .await
        .map_err(map_checkpoint_error)
    }

    pub async fn restore_entry(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        author: &str,
    ) -> Result<Value> {
        self.restore_entry_authorized_for_principals(space_id, entry_id, revision_id, author, &[])
            .await
    }

    pub async fn restore_entry_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        self.require_entry_action_for_principals(space_id, entry_id, Action::Update, principal_ids)
            .await?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let scopes = if principal_ids.is_empty() {
            None
        } else {
            Some(
                self.authorized_form_entry_scopes_for_principals(space_id, principal_ids)
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

    pub async fn restore_entry_from_checkpoint_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        revision_id: &str,
        checkpoint_name: &str,
        author: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        validate_storage_id(validate_revision_id(revision_id))?;
        self.require_entry_action_for_principals(space_id, entry_id, Action::Update, principal_ids)
            .await?;
        let checkpoint = self
            .load_named_checkpoint(space_id, checkpoint_name)
            .await?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        let scopes = self
            .checkpoint_form_scopes_for_principals(space_id, principal_ids)
            .await?;
        let result = entry::restore_entry_from_checkpoint_authorized(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            revision_id,
            &checkpoint,
            author,
            &integrity,
            scopes.as_ref(),
        )
        .await
        .map_err(map_checkpoint_error)?;
        self.schedule_asset_text_refresh(space_id);
        Ok(result)
    }

    pub async fn entry_at_checkpoint_authorized_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        checkpoint_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_entry_id(entry_id))?;
        let checkpoint = self
            .load_named_checkpoint(space_id, checkpoint_name)
            .await?;
        let scopes = self
            .checkpoint_form_scopes_for_principals(space_id, principal_ids)
            .await?;
        entry::get_entry_at_checkpoint(
            &self.operator,
            &self.workspace_path(space_id),
            entry_id,
            &checkpoint,
            scopes.as_ref(),
        )
        .await
        .map_err(map_checkpoint_error)
    }

    pub async fn diff_checkpoints_authorized_for_principals(
        &self,
        space_id: &str,
        from_name: &str,
        to_name: &str,
        principal_ids: &[Uuid],
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        let from = self.load_named_checkpoint(space_id, from_name).await?;
        let to = self.load_named_checkpoint(space_id, to_name).await?;
        let scopes = self
            .checkpoint_form_scopes_for_principals(space_id, principal_ids)
            .await?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let diff = workspace
            .diff_checkpoints_with_scopes(&from, &to, scopes.as_ref())
            .await
            .map_err(map_checkpoint_error)?;
        let mut diff = serde_json::to_value(diff)?;
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
        Ok(diff)
    }

    async fn load_named_checkpoint(
        &self,
        space_id: &str,
        checkpoint_name: &str,
    ) -> Result<SpaceCheckpoint> {
        validate_storage_id(validate_checkpoint_name(checkpoint_name))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        workspace
            .load_checkpoint(checkpoint_name)
            .await
            .map_err(map_checkpoint_error)
    }

    async fn checkpoint_form_scopes_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<Option<BTreeMap<FormId, EntryScope>>> {
        if principal_ids.is_empty() {
            return Ok(None);
        }
        let scopes = self
            .authorized_form_entry_scopes_for_principals(space_id, principal_ids)
            .await?;
        let saved_sql_scope = self
            .authorized_saved_sql_entry_scope_for_principals(space_id, principal_ids)
            .await?;
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

    async fn require_entry_action_for_principals(
        &self,
        space_id: &str,
        entry_id: &str,
        action: Action,
        principal_ids: &[Uuid],
    ) -> Result<()> {
        if principal_ids.is_empty() {
            return Ok(());
        }
        let state = Authorizer::new(self.operator.clone())
            .state(space_id)
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: entry_id.to_string(),
            parent: None,
        };
        for principal_id in principal_ids {
            if !effective_actions_for_state(&state, *principal_id, Some(&resource))?
                .contains(&action)
            {
                return Err(AppError::forbidden("Entry is not writable").into());
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
        validate_storage_id(validate_space_id(space_id))?;
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
        let scopes = self
            .authorized_form_entry_scopes(space_id, principal_id)
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
    }

    pub async fn list_entry_options_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        form: Option<&str>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<entry::EntrySummary>> {
        let scopes = self
            .authorized_form_entry_scopes_for_principals(space_id, principal_ids)
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
    }

    pub async fn search_entries(
        &self,
        space_id: &str,
        query: &str,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        validate_storage_id(validate_space_id(space_id))?;
        search::search_entries(
            &self.operator,
            &self.workspace_path(space_id),
            query,
            crate::MAX_NORMAL_READ_ROWS,
        )
        .await
    }

    pub async fn query_entries(&self, space_id: &str, filter: &Value) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        index::query_index(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
        )
        .await
    }

    pub async fn execute_sql_query(&self, space_id: &str, sql: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
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
        if principal_ids.is_empty() {
            return Ok(BTreeMap::new());
        }
        let state = Authorizer::new(self.operator.clone())
            .state(space_id)
            .await?;
        self.authorized_form_entry_scopes_for_state(space_id, &state, principal_ids)
            .await
    }

    async fn authorized_form_entry_scopes_for_state(
        &self,
        space_id: &str,
        state: &AuthorizationState,
        principal_ids: &[Uuid],
    ) -> Result<BTreeMap<String, EntryScope>> {
        for principal_id in principal_ids {
            if !effective_actions_for_state(&state, *principal_id, None)?.contains(&Action::Read) {
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
            .filter(|form| {
                let resource = ResourceRef {
                    kind: ResourceKind::Form,
                    id: form.name.clone(),
                    parent: None,
                };
                principal_ids.iter().all(|principal_id| {
                    effective_actions_for_state(&state, *principal_id, Some(&resource))
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
                if !effective_actions_for_state(&state, *principal_id, Some(&resource))?
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
        let state = Authorizer::new(self.operator.clone())
            .state(space_id)
            .await?;
        Self::saved_sql_entry_scope_for_state(&state, principal_ids)
    }

    pub(crate) fn saved_sql_entry_scope_for_state(
        state: &AuthorizationState,
        principal_ids: &[Uuid],
    ) -> Result<EntryScope> {
        if principal_ids.is_empty() {
            return Ok(EntryScope::Only(BTreeSet::new()));
        }
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
        validate_storage_id(validate_space_id(space_id))?;
        let scopes = self
            .authorized_form_entry_scopes_for_principals(space_id, principal_ids)
            .await?;
        entry::list_entries_with_scopes(
            &self.operator,
            &self.workspace_path(space_id),
            &scopes,
            limit,
            offset,
        )
        .await
    }

    pub async fn list_entries_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
    ) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
        let scopes = self
            .authorized_form_entry_scopes(space_id, principal_id)
            .await?;
        entry::list_entries_with_scopes(
            &self.operator,
            &self.workspace_path(space_id),
            &scopes,
            crate::MAX_NORMAL_READ_ROWS,
            0,
        )
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
        let allowed = Authorizer::new(self.operator.clone())
            .filter_authorized_resources(space_id, principal_id, resources, Action::Read)
            .await?;
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
        let mut filtered = values;
        for principal_id in principal_ids {
            filtered = self
                .filter_json_resources_authorized(
                    space_id,
                    *principal_id,
                    kind.clone(),
                    id_field,
                    filtered,
                )
                .await?;
        }
        Ok(filtered)
    }

    pub async fn search_entries_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        query: &str,
        limit: usize,
    ) -> Result<Vec<search::KeywordSearchResult>> {
        self.search_entries_authorized_for_principals_after(
            space_id,
            principal_ids,
            query,
            limit,
            None,
        )
        .await
    }

    pub async fn search_entries_authorized_for_principals_after(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        query: &str,
        limit: usize,
        after: Option<(&str, &str, &str)>,
    ) -> Result<Vec<search::KeywordSearchResult>> {
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

    pub async fn query_entries_authorized_for_principals(
        &self,
        space_id: &str,
        principal_ids: &[Uuid],
        filter: &Value,
    ) -> Result<Vec<Value>> {
        let scopes = self
            .authorized_form_entry_scopes_for_principals(space_id, principal_ids)
            .await?;
        index::query_index_authorized_by_form_scopes(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
            &scopes,
        )
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
        validate_storage_id(validate_space_id(space_id))?;
        require_sql_session_principals(principal_ids)?;
        let relation = index::sql_session_page_relation(sql).map_err(|error| {
            AppError::invalid_input(
                ugoite_core::error::ErrorCode::InvalidInput,
                error.to_string(),
            )
        })?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        let checkpoint = workspace.capture_checkpoint().await?;
        let state = Authorizer::new(self.operator.clone())
            .state(space_id)
            .await?;
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
        let bound_parameters = index::datafusion_parameters(&parameters, &parameter_types)?;
        sql_session::create_sql_session_authorized_for_principals_with_frozen_policy_and_saved_sql_scope(
            &self.operator,
            &self.workspace_path(space_id),
            sql,
            parameters,
            parameter_types,
            authorization,
            bound_parameters,
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
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        let current_authorization = self
            .sql_session_current_execution_authorization(space_id, session_id, principal_ids)
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
    }

    /// Rebuilds the execution policy from immutable checkpoint metadata and
    /// the current authorization state. Durable session policy JSON is only a
    /// cache: every use compares it against this independently derived value.
    async fn sql_session_current_execution_authorization(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<CurrentSqlSessionExecutionAuthorization> {
        require_sql_session_principals(principal_ids)?;
        let workspace_path = self.workspace_path(space_id);
        let inputs =
            sql_session::get_session_execution_inputs(&self.operator, &workspace_path, session_id)
                .await
                .map_err(sql_session_metadata_authorization_error)?;
        let relation = index::sql_session_page_relation(&inputs.sql)
            .map_err(sql_session_metadata_authorization_error)?;
        let state = Authorizer::new(self.operator.clone())
            .state(space_id)
            .await?;
        let entry_scope = sql_session_entry_scope(&state, principal_ids)?;
        let query_policy = index::sql_session_query_policy_at_checkpoint(
            &self.operator,
            &workspace_path,
            &relation,
            entry_scope,
            &inputs.checkpoint,
        )
        .await
        .map_err(sql_session_metadata_authorization_error)?;
        Ok(CurrentSqlSessionExecutionAuthorization {
            policy_hash: sql_session_policy_hash(&state, principal_ids)?,
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
        let scopes = self
            .authorized_form_entry_scopes(space_id, principal_id)
            .await?;
        index::query_index_authorized_by_form_scopes(
            &self.operator,
            &self.workspace_path(space_id),
            &filter.to_string(),
            &scopes,
        )
        .await
    }

    pub async fn execute_sql_query_authorized(
        &self,
        space_id: &str,
        principal_id: Uuid,
        sql: &str,
    ) -> Result<Vec<Value>> {
        let scopes = self
            .authorized_form_entry_scopes(space_id, principal_id)
            .await?;
        index::execute_sql_query_authorized_by_form_scopes(
            &self.operator,
            &self.workspace_path(space_id),
            sql,
            &scopes,
        )
        .await
    }

    pub async fn reindex(&self, space_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        index::reindex_all(&self.operator, &self.workspace_path(space_id)).await
    }

    /// Performs the synchronous cleanup pass used by explicit local index
    /// maintenance. Server workers can keep the delayed grace-period task
    /// alive; `ugoite index run` invokes this pass before it exits.
    pub async fn garbage_collect_asset_text_builds(&self, space_id: &str) -> Result<Vec<String>> {
        validate_storage_id(validate_space_id(space_id))?;
        crate::derived_relation::garbage_collect_asset_text(
            &self.operator,
            &self.workspace_path(space_id),
        )
        .await
    }

    /// Retries physical cleanup for Assets whose authoritative tombstone is
    /// already committed.  A failed delete must remain recoverable without
    /// replaying Catalog history, so server startup and explicit index
    /// maintenance call this bounded sweeper.
    pub async fn garbage_collect_deleted_asset_blobs(&self, space_id: &str) -> Result<usize> {
        validate_storage_id(validate_space_id(space_id))?;
        let workspace =
            iceberg_store::native_workspace(&self.operator, &self.workspace_path(space_id)).await?;
        workspace.garbage_collect_deleted_asset_blobs().await
    }

    /// Rehydrates derived GC after a server restart. Derived cleanup is
    /// best-effort and never blocks authoritative startup recovery.
    pub async fn rearm_asset_text_gc(&self, space_id: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
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
        validate_storage_id(validate_space_id(space_id))?;
        let ws_path = self.workspace_path(space_id);
        if crate::derived_relation::asset_text_refresh_needed(&self.operator, &ws_path).await? {
            let shared = matches!(
                self.operator.info().scheme(),
                "s3" | "gcs" | "oss" | "azdls"
            );
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
        validate_storage_id(validate_space_id(space_id))?;
        index::get_space_stats(&self.operator, &self.workspace_path(space_id)).await
    }

    pub async fn save_asset(
        &self,
        space_id: &str,
        filename: &str,
        content: &[u8],
    ) -> Result<ugoite_domain::entry::AssetReference> {
        validate_storage_id(validate_space_id(space_id))?;
        asset::save_asset(
            &self.operator,
            &self.workspace_path(space_id),
            filename,
            content,
        )
        .await
    }

    pub async fn save_asset_with_media_type(
        &self,
        space_id: &str,
        filename: &str,
        content: &[u8],
        media_type: &str,
    ) -> Result<ugoite_domain::entry::AssetReference> {
        validate_storage_id(validate_space_id(space_id))?;
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
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_asset_id(asset_id))?;
        asset::read_asset(&self.operator, &self.workspace_path(space_id), asset_id).await
    }

    pub async fn ensure_asset_reference_is_readable(
        &self,
        space_id: &str,
        form_name: &str,
        entry_id: &str,
        asset_id: &str,
    ) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
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
        self.delete_asset_with_principal(space_id, asset_id, None)
            .await
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
            None => {
                self.delete_asset_with_principals(space_id, asset_id, &[])
                    .await
            }
        }
    }

    pub async fn delete_asset_with_principals(
        &self,
        space_id: &str,
        asset_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_asset_id(asset_id))?;
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
            self.authorized_form_entry_scopes_for_principals(space_id, principal_ids)
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
        preferences::patch_user_preferences(&self.operator, user_id, patch).await
    }

    pub async fn get_sql_session_count_authorized_for_principals(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
    ) -> Result<u64> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        let current_authorization = self
            .sql_session_current_execution_authorization(space_id, session_id, principal_ids)
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
    }

    pub async fn get_sql_session_rows_authorized_for_principals(
        &self,
        space_id: &str,
        session_id: &str,
        principal_ids: &[Uuid],
        offset: usize,
        limit: usize,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_session_id(session_id))?;
        let current_authorization = self
            .sql_session_current_execution_authorization(space_id, session_id, principal_ids)
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
    }

    /// Lists Saved SQL without resource filtering for operator-local/admin
    /// tooling. Server-backed user requests use the authorized variant below.
    pub async fn list_saved_sql_operator_unscoped(&self, space_id: &str) -> Result<Vec<Value>> {
        validate_storage_id(validate_space_id(space_id))?;
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
        validate_storage_id(validate_space_id(space_id))?;
        let entry_scope = self
            .authorized_saved_sql_entry_scope_for_principals(space_id, principal_ids)
            .await?;
        saved_sql::list_sql(&self.operator, &self.workspace_path(space_id), entry_scope).await
    }

    pub async fn create_saved_sql(
        &self,
        space_id: &str,
        sql_id: &str,
        payload: &saved_sql::SqlPayload,
        author: &str,
    ) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_id(sql_id))?;
        let integrity = RealIntegrityProvider::from_space(&self.operator, space_id).await?;
        saved_sql::create_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            author,
            &integrity,
        )
        .await
    }

    pub async fn get_saved_sql(&self, space_id: &str, sql_id: &str) -> Result<Value> {
        validate_storage_id(validate_space_id(space_id))?;
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
        validate_storage_id(validate_space_id(space_id))?;
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
        saved_sql::update_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            payload,
            parent_revision_id,
            author,
            &integrity,
        )
        .await
    }

    pub async fn delete_saved_sql(&self, space_id: &str, sql_id: &str, actor: &str) -> Result<()> {
        validate_storage_id(validate_space_id(space_id))?;
        validate_storage_id(validate_sql_id(sql_id))?;
        saved_sql::delete_sql(
            &self.operator,
            &self.workspace_path(space_id),
            sql_id,
            actor,
        )
        .await
    }

    pub async fn test_storage_connection(
        &self,
        config: &space::StorageConnectionTestConfig,
    ) -> Result<Value> {
        space::test_storage_connection(config).await
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

pub fn validate_public_space_patch(patch: &Value) -> Result<()> {
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

    Err(anyhow!(
        "space patch does not allow membership-managed settings keys: {}. Use the dedicated member commands instead.",
        reserved_keys.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_domain::identity::{
        Membership, PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
    };

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
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_principal_space_creation_preserves_slug_uniqueness() -> Result<()> {
        let service = UgoiteService::new("memory://service-space-create-race")?;
        let first = service.create_space_for_principal("race-space", Uuid::now_v7(), "First owner");
        let second =
            service.create_space_for_principal("race-space", Uuid::now_v7(), "Second owner");
        let (first_result, second_result) = tokio::join!(first, second);
        assert!(first_result.is_ok() ^ second_result.is_ok());
        let mut matching_spaces = 0;
        for space_id in service.list_space_ids().await? {
            if service.get_space(&space_id).await?["slug"] == "race-space" {
                matching_spaces += 1;
            }
        }
        assert_eq!(matching_spaces, 1);
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
}
