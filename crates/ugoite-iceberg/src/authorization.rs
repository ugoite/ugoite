//! Space-portable authorization state and the shared authorizer used by adapters.

use crate::audit;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use fs2::FileExt;
use futures::TryStreamExt;
use opendal::options::{ReadOptions, WriteOptions};
use opendal::Operator;
use rand::TryRng;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    future::Future,
    path::Path,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
};
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio::task::JoinHandle;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::identity::{
    evaluate_policy, role_actions, AccessPolicy, Action, AgentMode, AgentPrincipal, Membership,
    PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use uuid::Uuid;

const AUTHORIZATION_FILE: &str = "security/principals.json";
const LEGACY_AUTHORIZATION_FILE: &str = "authorization.json";
const LEGACY_MIGRATION_STATE_FILE: &str = "security/migration-state.json";
const CURRENT_AUTHORIZATION_SCHEMA_VERSION: u32 = 1;
const MAX_AUTHORIZATION_STATE_BYTES: usize = 64 * 1024 * 1024;
const AUTHORIZATION_STATE_READER_CHUNK_BYTES: usize = 256 * 1024;
const MAX_AUTHORIZATION_MAP_ENTRIES: usize = 100_000;
const MAX_AUTHORIZATION_POLICY_HISTORY_REVISIONS: usize = 1_000_000;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Entry,
    Asset,
    Form,
    SavedSql,
    MaterializedView,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ResourceRef {
    pub kind: ResourceKind,
    pub id: String,
    /// A caller may provide an explicit parent for policy evaluation. Asset
    /// references do not cause the service to infer one by scanning Entries.
    pub parent: Option<Box<ResourceRef>>,
}

impl ResourceRef {
    pub fn key(&self) -> String {
        format!(
            "{}:{}",
            serde_json::to_value(&self.kind)
                .unwrap_or_default()
                .as_str()
                .unwrap_or("resource"),
            self.id
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationState {
    pub schema_version: u32,
    pub space_uid: Uuid,
    #[serde(default)]
    pub principals: BTreeMap<Uuid, SpacePrincipal>,
    #[serde(default)]
    pub memberships: BTreeMap<Uuid, Membership>,
    #[serde(default)]
    pub policies: BTreeMap<String, AccessPolicy>,
    #[serde(default)]
    pub policy_history: BTreeMap<String, Vec<PolicyRevision>>,
    #[serde(default)]
    pub agents: BTreeMap<Uuid, AgentPrincipal>,
    #[serde(default)]
    pub agent_grants: BTreeMap<Uuid, BTreeSet<Action>>,
    /// Monotonic lifecycle epochs for human principals. Recovery approvals
    /// bind to these epochs so revoke/demotion/reactivation cannot resurrect
    /// an old approval.
    #[serde(default)]
    pub principal_lifecycle_epochs: BTreeMap<Uuid, u64>,
    /// Durable recovery reservations. While a fence is active, membership
    /// mutations for this Space must fail rather than race recovery.
    #[serde(default)]
    pub recovery_fences: BTreeMap<Uuid, RecoveryFence>,
    /// Single-use approvals issued by a recently reauthenticated human.
    /// Only the SHA-256 hash of the bearer token is stored here.
    #[serde(default)]
    pub human_approvals: BTreeMap<Uuid, HumanApproval>,
    /// Space-portable, restart-safe audit delivery for approval lifecycle
    /// events. The event payload never contains the bearer token.
    #[serde(default)]
    pub human_approval_audit_outbox: BTreeMap<Uuid, HumanApprovalAuditOutbox>,
    /// Reserved monotonic revision for future synchronization protocols.
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HumanApproval {
    pub approval_id: Uuid,
    pub token_hash: String,
    pub operation: String,
    pub action: Action,
    pub resource: ResourceRef,
    pub intent_hash: String,
    pub actor_principal_id: Uuid,
    pub actor_credential_id: Uuid,
    pub issuer_principal_id: Uuid,
    pub issuer_account_id: Uuid,
    pub issuer_credential_id: Uuid,
    /// Credential generation captured when the issuer's Passkey authorized
    /// this approval. Node-side generation rotation invalidates the approval
    /// even if the Passkey record itself still exists.
    #[serde(default)]
    pub issuer_credential_generation: u64,
    /// Node account lifecycle epoch captured at issuance. Suspension and
    /// reactivation invalidate approvals even when the account generation and
    /// Passkey record are unchanged.
    #[serde(default)]
    pub issuer_node_account_lifecycle_epoch: u64,
    pub issuer_lifecycle_epoch: u64,
    pub actor_lifecycle_epoch: u64,
    pub issued_at: String,
    pub expires_at: String,
    pub consumed_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HumanApprovalAuditOutbox {
    pub event_id: Uuid,
    pub event: Value,
    pub delivered: bool,
    /// Causal insertion order. UUID ordering is not a durable audit order.
    #[serde(default)]
    pub sequence: u64,
}

#[derive(Clone, Debug)]
pub struct HumanApprovalIssue {
    pub operation: String,
    pub action: Action,
    pub resource: ResourceRef,
    pub intent_hash: String,
    pub actor_principal_id: Uuid,
    pub actor_credential_id: Uuid,
    pub issuer_principal_id: Uuid,
    pub issuer_account_id: Uuid,
    pub issuer_credential_id: Uuid,
    pub issuer_credential_generation: u64,
    pub issuer_node_account_lifecycle_epoch: u64,
    pub ttl: chrono::Duration,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecoveryFence {
    pub fence_id: Uuid,
    pub request_id: Uuid,
    pub space_uid: Uuid,
    pub issuer_principal_id: Uuid,
    pub issuer_account_id: Uuid,
    pub target_principal_id: Uuid,
    pub target_account_id: Uuid,
    pub authorization_revision: u64,
    pub issuer_space_lifecycle_epoch: u64,
    pub target_space_lifecycle_epoch: u64,
    #[serde(default)]
    pub issuer_generation: u64,
    #[serde(default)]
    pub target_generation: u64,
    pub expires_at: String,
    pub status: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PolicyRevision {
    pub policy: AccessPolicy,
    pub changed_at: String,
    pub actor_principal_id: Uuid,
}

#[derive(Clone)]
pub struct Authorizer {
    operator: Operator,
    lock: Arc<Mutex<()>>,
    #[cfg(test)]
    ambiguous_write_once: Arc<AtomicBool>,
    #[cfg(test)]
    ambiguous_write_with_post_commit_writer_once: Arc<AtomicBool>,
}

/// Authorization lease held across one protected mutation. The process lock
/// covers local callers; shared operators additionally carry the durable
/// object-store lease in this value.
pub struct AuthorizationLease {
    _guard: OwnedMutexGuard<()>,
    durable: Option<DurableAuthorizationLease>,
}

/// Cross-process lease for a shared Space authorization/content mutation.
/// The lease is deliberately an object-store CAS record rather than a local
/// mutex: a remote ACL writer must not commit between a protected mutation's
/// authorization check and its authoritative write.
struct DurableAuthorizationLease {
    operator: Operator,
    path: String,
    owner: String,
    etag: Arc<Mutex<String>>,
    lost: Arc<AtomicBool>,
    released: Arc<AtomicBool>,
    heartbeat: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct AuthorizationWriteFence {
    durable: Option<Arc<DurableAuthorizationWriteFence>>,
}

struct DurableAuthorizationWriteFence {
    operator: Operator,
    path: String,
    owner: String,
    etag: Arc<Mutex<String>>,
    lost: Arc<AtomicBool>,
}

tokio::task_local! {
    static AUTHORIZATION_WRITE_FENCE: AuthorizationWriteFence;
}

fn authorization_write_lock() -> Arc<Mutex<()>> {
    static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(Mutex::new(()))).clone()
}

fn authorization_mutation_lock_bytes(owner: &str, released: bool) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "owner": owner,
        "released": released,
        "heartbeat_at": Utc::now().timestamp(),
    }))
    .expect("authorization mutation lock is serializable")
}

impl DurableAuthorizationLease {
    async fn release(mut self) -> Result<()> {
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.abort();
            let _ = heartbeat.await;
        }
        if self.lost.load(Ordering::Acquire) {
            return Err(anyhow!(
                "Space authorization mutation lease was lost before release"
            ));
        }
        let etag = self.etag.lock().await.clone();
        let result = match self
            .operator
            .write_options(
                &self.path,
                authorization_mutation_lock_bytes(&self.owner, true),
                WriteOptions {
                    if_match: Some(etag),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => {
                self.released.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) if error.kind() == opendal::ErrorKind::ConditionNotMatch => Err(anyhow!(
                "Space authorization mutation lease changed before release"
            )),
            Err(error) => Err(error.into()),
        };
        result
    }

    fn ensure_held(&self) -> Result<()> {
        if self.lost.load(Ordering::Acquire) {
            bail!("Space authorization mutation lease was lost")
        }
        Ok(())
    }
}

impl Drop for DurableAuthorizationLease {
    fn drop(&mut self) {
        if let Some(heartbeat) = self.heartbeat.take() {
            heartbeat.abort();
        }
        // Never release a shared mutation lease from Drop. Drop is also the
        // cancellation path, and an asynchronous best-effort release could
        // race a committed-or-unknown mutation and let a new writer enter
        // before the old write has settled. Explicit release is the only
        // successful hand-off; cancellation and lease loss retain the owner
        // fence until the backend TTL expires.
    }
}

impl AuthorizationLease {
    pub fn write_fence(&self) -> AuthorizationWriteFence {
        AuthorizationWriteFence {
            durable: self.durable.as_ref().map(|lease| {
                Arc::new(DurableAuthorizationWriteFence {
                    operator: lease.operator.clone(),
                    path: lease.path.clone(),
                    owner: lease.owner.clone(),
                    etag: lease.etag.clone(),
                    lost: lease.lost.clone(),
                })
            }),
        }
    }

    pub async fn release(mut self) -> Result<()> {
        if let Some(durable) = self.durable.take() {
            durable.release().await
        } else {
            Ok(())
        }
    }

    /// Run a protected callback while a shared lease remains alive. The
    /// callback is deliberately allowed to finish if the heartbeat is lost:
    /// dropping an in-flight storage future would make a committed-or-unknown
    /// mutation impossible to reconcile. The latched pre/post checks instead
    /// fail closed and force callers to treat the outcome as retryable.
    pub async fn run_while_held<T, F, Fut>(&self, operation: F) -> std::result::Result<T, ()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = T>,
    {
        let Some(durable) = self.durable.as_ref() else {
            return Ok(operation().await);
        };
        durable.ensure_held().map_err(|_| ())?;
        let value = operation().await;
        durable.ensure_held().map_err(|_| ())?;
        Ok(value)
    }
}

pub async fn with_authorization_write_fence<T, F>(fence: AuthorizationWriteFence, operation: F) -> T
where
    F: Future<Output = T>,
{
    AUTHORIZATION_WRITE_FENCE.scope(fence, operation).await
}

/// Check the lease at the last write boundary of authoritative storage
/// operations. Local callers without a server authorization scope are
/// intentionally unaffected; shared server mutations must still own the
/// conditional lock immediately before committing.
pub async fn ensure_authorization_write_fence() -> Result<()> {
    let Some(fence) = AUTHORIZATION_WRITE_FENCE.try_with(Clone::clone).ok() else {
        return Ok(());
    };
    if let Some(durable) = fence.durable {
        if durable.lost.load(Ordering::Acquire) {
            bail!("Space authorization mutation lease was lost before write");
        }
        for _ in 0..2 {
            let etag = durable.etag.lock().await.clone();
            let bytes = match durable
                .operator
                .read_options(
                    &durable.path,
                    ReadOptions {
                        if_match: Some(etag),
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == opendal::ErrorKind::ConditionNotMatch => continue,
                Err(error) => return Err(error.into()),
            };
            let value: Value = serde_json::from_slice(&bytes.to_vec())
                .context("decode Space authorization mutation fence")?;
            if value.get("owner").and_then(Value::as_str) != Some(durable.owner.as_str())
                || value.get("released").and_then(Value::as_bool) == Some(true)
            {
                durable.lost.store(true, Ordering::Release);
                bail!("Space authorization mutation lease changed before write");
            }
            return Ok(());
        }
        bail!("Space authorization mutation lease changed before write");
    }
    Ok(())
}

async fn finish_authorization_lease<T>(lease: AuthorizationLease, result: Result<T>) -> Result<T> {
    let release = lease.release().await;
    match (result, release) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(release_error)) => Err(error.context(format!(
            "release Space authorization mutation lease: {release_error:#}"
        ))),
    }
}

#[derive(Clone)]
pub struct CreateAgentRequest {
    pub display_name: String,
    pub description: String,
    pub mode: AgentMode,
    pub owner_principal_ids: BTreeSet<Uuid>,
    pub granted_actions: BTreeSet<Action>,
    pub expires_at: Option<String>,
}

impl Authorizer {
    pub fn new(operator: Operator) -> Self {
        Self {
            operator,
            lock: authorization_write_lock(),
            #[cfg(test)]
            ambiguous_write_once: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            ambiguous_write_with_post_commit_writer_once: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    fn inject_ambiguous_write_once(&self) {
        self.ambiguous_write_once.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn inject_ambiguous_write_with_post_commit_writer_once(&self) {
        self.ambiguous_write_with_post_commit_writer_once
            .store(true, Ordering::SeqCst);
    }

    pub async fn initialize_owner(
        &self,
        space_id: &str,
        space_uid: Uuid,
        principal_id: Uuid,
        display_name: &str,
    ) -> Result<()> {
        let _guard = self.lock.lock().await;
        let path = state_path(space_id);
        if self.operator.exists(&path).await? {
            bail!("authorization state already exists");
        }
        let now = now_iso();
        let principal = SpacePrincipal {
            principal_id,
            kind: PrincipalKind::Human,
            display_name: display_name.trim().to_string(),
            state: PrincipalState::Active,
            created_at: now.clone(),
        };
        let membership = Membership {
            principal_id,
            role: SpaceRole::Owner,
            created_at: now,
        };
        let state = AuthorizationState {
            schema_version: 1,
            space_uid,
            principals: [(principal_id, principal)].into_iter().collect(),
            memberships: [(principal_id, membership)].into_iter().collect(),
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
        self.write_state(space_id, &state).await
    }

    /// Ensures an operator-created Space has the first current-release owner.
    /// Existing authorization state is validated rather than upgraded.
    pub async fn validate_current_layout(&self, space_id: &str, space_uid: Uuid) -> Result<()> {
        self.validated_current_owner(space_id, space_uid)
            .await
            .map(|_| ())
    }

    async fn validated_current_owner(
        &self,
        space_id: &str,
        space_uid: Uuid,
    ) -> Result<Option<Uuid>> {
        for marker in [LEGACY_AUTHORIZATION_FILE, LEGACY_MIGRATION_STATE_FILE] {
            let path = format!("spaces/{space_id}/{marker}");
            if self.operator.exists(&path).await? {
                bail!("unsupported Space layout: legacy marker {marker} is present");
            }
        }

        let settings_path = format!("spaces/{space_id}/settings.json");
        if self.operator.exists(&settings_path).await? {
            let settings: serde_json::Value =
                serde_json::from_slice(&self.operator.read(&settings_path).await?.to_vec())?;
            if let Some(legacy_key) = crate::service::MEMBERSHIP_MANAGED_SPACE_SETTING_KEYS
                .iter()
                .find(|key| settings.get(*key).is_some())
            {
                bail!(
                    "unsupported Space layout: legacy membership setting {legacy_key} is present"
                );
            }
        }

        let path = state_path(space_id);
        if !self.operator.exists(&path).await? {
            return Ok(None);
        }

        let state = self.state(space_id).await?;
        if state.space_uid != space_uid {
            bail!("Space metadata and authorization state use different space_uid values");
        }
        let owner_memberships = state
            .memberships
            .values()
            .filter(|membership| matches!(membership.role, SpaceRole::Owner))
            .collect::<Vec<_>>();
        if owner_memberships.is_empty() {
            bail!("Space has no owner principal");
        }
        for membership in &owner_memberships {
            let principal = state
                .principals
                .get(&membership.principal_id)
                .ok_or_else(|| anyhow!("Space owner membership references an unknown principal"))?;
            if !matches!(principal.kind, PrincipalKind::Human) {
                bail!("Space owner membership must reference a human principal");
            }
        }
        owner_memberships
            .into_iter()
            .find(|membership| {
                state
                    .principals
                    .get(&membership.principal_id)
                    .is_some_and(|principal| matches!(principal.state, PrincipalState::Active))
            })
            .map(|membership| Some(membership.principal_id))
            .ok_or_else(|| anyhow!("Space has no active owner principal"))
    }

    pub async fn ensure_owner(
        &self,
        space_id: &str,
        space_uid: Uuid,
        display_name: &str,
    ) -> Result<Uuid> {
        if let Some(owner_principal_id) = self.validated_current_owner(space_id, space_uid).await? {
            return Ok(owner_principal_id);
        }
        let principal_id = Uuid::now_v7();
        self.initialize_owner(space_id, space_uid, principal_id, display_name)
            .await?;
        Ok(principal_id)
    }

    pub async fn state(&self, space_id: &str) -> Result<AuthorizationState> {
        let bytes = read_authorization_state_bytes(&self.operator, &state_path(space_id), None)
            .await
            .context("read Space authorization state")?;
        let state: AuthorizationState =
            serde_json::from_slice(&bytes).context("decode Space authorization state")?;
        validate_authorization_state(&state)?;
        let metadata_path = format!("spaces/{space_id}/meta.json");
        if self.operator.exists(&metadata_path).await? {
            let metadata = crate::space::get_space_raw(&self.operator, space_id)
                .await
                .context("read Space metadata for authorization binding")?;
            let metadata_space_uid = metadata
                .get("space_uid")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Space metadata has no immutable space_uid"))
                .and_then(|value| Uuid::parse_str(value).map_err(anyhow::Error::from))?;
            if state.space_uid != metadata_space_uid {
                bail!("Space metadata and authorization state use different space_uid values");
            }
        }
        Ok(state)
    }

    async fn acquire_durable_mutation_lease(
        &self,
        _space_id: &str,
    ) -> Result<Option<DurableAuthorizationLease>> {
        if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
            return Ok(None);
        }
        self.ensure_authoritative_mutation_contract()?;
        unreachable!("shared authorization mutation leases are fail-closed in v1")
    }

    /// Shared authorization/content mutations are intentionally unavailable
    /// until the storage backend can fence all participating objects as one
    /// atomic operation.  This check is also used by multi-object bootstrap
    /// paths before they create their first authoritative object.
    pub fn ensure_authoritative_mutation_contract(&self) -> Result<()> {
        if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
            return Ok(());
        }
        Err(AppError::dependency_unavailable(
            ErrorCode::StorageMutationUnavailable,
            "non-local Space mutations are unavailable in v0.1 until the storage backend provides an atomic multi-object fencing contract",
        )
        .into())
    }

    /// Runs an authorization-dependent read while holding the same process
    /// lock used by every authorization mutation. Shared backends still get a
    /// revision check at the caller boundary, while local mutations cannot
    /// commit between the snapshot and the protected read.
    pub async fn with_state_lock<T, F, Fut>(&self, space_id: &str, operation: F) -> Result<T>
    where
        F: FnOnce(AuthorizationState) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let _guard: OwnedMutexGuard<()> = self.lock.clone().lock_owned().await;
        let state = self.state(space_id).await?;
        operation(state).await
    }

    pub async fn acquire_state_lease(
        &self,
        space_id: &str,
    ) -> Result<(AuthorizationState, AuthorizationLease)> {
        let guard = self.lock.clone().lock_owned().await;
        let durable = self.acquire_durable_mutation_lease(space_id).await?;
        let state = match self.state(space_id).await {
            Ok(state) => state,
            Err(error) => {
                if let Some(durable) = durable {
                    let _ = durable.release().await;
                }
                return Err(error);
            }
        };
        Ok((
            state,
            AuthorizationLease {
                _guard: guard,
                durable,
            },
        ))
    }

    pub async fn effective_actions(
        &self,
        space_id: &str,
        principal_id: Uuid,
        resource: Option<&ResourceRef>,
    ) -> Result<BTreeSet<Action>> {
        let state = self.state(space_id).await?;
        effective_actions_for_state(&state, principal_id, resource)
    }

    /// Issue a single-use approval after the server has verified a recent
    /// phishing-resistant human authentication. Authorization and the actor
    /// tuple are checked again against the portable Space state here.
    pub async fn issue_human_approval(
        &self,
        space_id: &str,
        request: HumanApprovalIssue,
    ) -> Result<(HumanApproval, String)> {
        self.issue_human_approval_with_audit(space_id, request, |_| Vec::new())
            .await
    }

    pub async fn issue_human_approval_with_audit<F>(
        &self,
        space_id: &str,
        request: HumanApprovalIssue,
        audit_events: F,
    ) -> Result<(HumanApproval, String)>
    where
        F: FnOnce(&HumanApproval) -> Vec<(Uuid, Value)>,
    {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .issue_human_approval_with_audit_with_lease(space_id, request, audit_events, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    /// Issue an approval while reusing an authorization lease held by a
    /// compound request. The permission snapshot and approval commit must be
    /// in the same lease window as the request's other Node-side effects.
    pub async fn issue_human_approval_with_audit_with_lease<F>(
        &self,
        space_id: &str,
        request: HumanApprovalIssue,
        audit_events: F,
        lease: &AuthorizationLease,
    ) -> Result<(HumanApproval, String)>
    where
        F: FnOnce(&HumanApproval) -> Vec<(Uuid, Value)>,
    {
        if request.ttl < chrono::Duration::seconds(1)
            || request.ttl > chrono::Duration::seconds(300)
        {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "human approval TTL must be between 1 and 300 seconds",
            )
            .into());
        }
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        let issuer = state
            .principals
            .get(&request.issuer_principal_id)
            .filter(|principal| {
                matches!(principal.kind, PrincipalKind::Human)
                    && matches!(principal.state, PrincipalState::Active)
            })
            .ok_or_else(|| AppError::forbidden("approval issuer is not an active human"))?;
        let _ = issuer;
        if !effective_actions_for_state(
            &state,
            request.issuer_principal_id,
            Some(&request.resource),
        )?
        .contains(&request.action)
        {
            return Err(AppError::forbidden("approval issuer lacks the required action").into());
        }
        if !effective_actions_for_state(
            &state,
            request.actor_principal_id,
            Some(&request.resource),
        )?
        .contains(&request.action)
        {
            return Err(AppError::forbidden("approval actor lacks the required action").into());
        }
        let actor = state
            .principals
            .get(&request.actor_principal_id)
            .filter(|principal| matches!(principal.state, PrincipalState::Active))
            .ok_or_else(|| AppError::forbidden("approval actor is not active"))?;
        let _ = actor;
        let mut raw_token = [0_u8; 32];
        rand::rng()
            .try_fill_bytes(&mut raw_token)
            .map_err(|error| anyhow!("generate human approval token: {error}"))?;
        let token = URL_SAFE_NO_PAD.encode(raw_token);
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
        let now = Utc::now();
        let approval = HumanApproval {
            approval_id: Uuid::now_v7(),
            token_hash,
            operation: request.operation,
            action: request.action,
            resource: request.resource,
            intent_hash: request.intent_hash,
            actor_principal_id: request.actor_principal_id,
            actor_credential_id: request.actor_credential_id,
            issuer_principal_id: request.issuer_principal_id,
            issuer_account_id: request.issuer_account_id,
            issuer_credential_id: request.issuer_credential_id,
            issuer_credential_generation: request.issuer_credential_generation,
            issuer_node_account_lifecycle_epoch: request.issuer_node_account_lifecycle_epoch,
            issuer_lifecycle_epoch: state
                .principal_lifecycle_epochs
                .get(&request.issuer_principal_id)
                .copied()
                .unwrap_or_default(),
            actor_lifecycle_epoch: state
                .principal_lifecycle_epochs
                .get(&request.actor_principal_id)
                .copied()
                .unwrap_or_default(),
            issued_at: now.to_rfc3339_opts(SecondsFormat::Millis, true),
            expires_at: (now + request.ttl).to_rfc3339_opts(SecondsFormat::Millis, true),
            consumed_at: None,
        };
        queue_human_approval_audit_events(&mut state, audit_events(&approval));
        state
            .human_approvals
            .insert(approval.approval_id, approval.clone());
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        self.write_state_with_lease(space_id, &state, lease).await?;
        Ok((approval, token))
    }

    /// Atomically consume an approval. A consumed token is never restored,
    /// including when the subsequent business mutation has an unknown result.
    #[allow(clippy::too_many_arguments)]
    pub async fn consume_human_approval(
        &self,
        space_id: &str,
        token: &str,
        operation: &str,
        action: Action,
        resource: &ResourceRef,
        intent_hash: &str,
        actor_principal_id: Uuid,
        actor_credential_id: Uuid,
    ) -> Result<HumanApproval> {
        self.consume_human_approval_with_audit(
            space_id,
            token,
            operation,
            action,
            resource,
            intent_hash,
            actor_principal_id,
            actor_credential_id,
            |_, _, _, _| Vec::new(),
        )
        .await
    }

    pub async fn human_approval_for_token(
        &self,
        space_id: &str,
        token: &str,
    ) -> Result<Option<HumanApproval>> {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
        Ok(self
            .state(space_id)
            .await?
            .human_approvals
            .values()
            .find(|approval| approval.token_hash == token_hash)
            .cloned())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn consume_human_approval_with_audit<F>(
        &self,
        space_id: &str,
        token: &str,
        operation: &str,
        action: Action,
        resource: &ResourceRef,
        intent_hash: &str,
        actor_principal_id: Uuid,
        actor_credential_id: Uuid,
        audit_events: F,
    ) -> Result<HumanApproval>
    where
        F: Fn(Option<&HumanApproval>, &str, &str, &str) -> Vec<(Uuid, Value)>,
    {
        let (approval, mutation) = self
            .consume_human_approval_with_audit_and(
                space_id,
                token,
                operation,
                action,
                resource,
                intent_hash,
                actor_principal_id,
                actor_credential_id,
                audit_events,
                || async { Ok::<(), anyhow::Error>(()) },
            )
            .await?;
        mutation?;
        Ok(approval)
    }

    /// Consume an approval and run the dangerous mutation while the shared
    /// authorization write lock is still held. This is the linearization
    /// point for approval-bound mutations: an ACL/lifecycle write cannot land
    /// between the current authorization check and the actual mutation.
    #[allow(clippy::too_many_arguments)]
    pub async fn consume_human_approval_with_audit_and<T, F, Fut>(
        &self,
        space_id: &str,
        token: &str,
        operation: &str,
        action: Action,
        resource: &ResourceRef,
        intent_hash: &str,
        actor_principal_id: Uuid,
        actor_credential_id: Uuid,
        audit_events: F,
        mutation: impl FnOnce() -> Fut,
    ) -> Result<(HumanApproval, T)>
    where
        F: Fn(Option<&HumanApproval>, &str, &str, &str) -> Vec<(Uuid, Value)>,
        Fut: Future<Output = T> + Send + 'static,
    {
        self.consume_human_approval_with_audit_and_with_lease(
            space_id,
            token,
            operation,
            action,
            resource,
            intent_hash,
            actor_principal_id,
            actor_credential_id,
            audit_events,
            |_| Box::pin(mutation()),
        )
        .await
    }

    /// Variant of approval consumption that exposes the already-held
    /// authorization lease to a nested Space mutation. Nested authorization
    /// writers must reuse this lease instead of trying to acquire a second
    /// shared lock.
    #[allow(clippy::too_many_arguments)]
    pub async fn consume_human_approval_with_audit_and_with_lease<T, F>(
        &self,
        space_id: &str,
        token: &str,
        operation: &str,
        action: Action,
        resource: &ResourceRef,
        intent_hash: &str,
        actor_principal_id: Uuid,
        actor_credential_id: Uuid,
        audit_events: F,
        mutation: impl for<'a> FnOnce(
            &'a AuthorizationLease,
        ) -> Pin<Box<dyn Future<Output = T> + Send + 'a>>,
    ) -> Result<(HumanApproval, T)>
    where
        F: Fn(Option<&HumanApproval>, &str, &str, &str) -> Vec<(Uuid, Value)>,
    {
        let guard = self.lock.clone().lock_owned().await;
        let durable = self.acquire_durable_mutation_lease(space_id).await?;
        let lease = AuthorizationLease {
            _guard: guard,
            durable,
        };
        let result = self
            .consume_human_approval_with_audit_and_locked(
                space_id,
                token,
                operation,
                action,
                resource,
                intent_hash,
                actor_principal_id,
                actor_credential_id,
                audit_events,
                &lease.durable,
                &lease,
                mutation,
            )
            .await;
        let release = lease.release().await;
        match (result, release) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), Ok(())) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Err(error), Err(release_error)) => Err(error.context(format!(
                "release Space authorization mutation lease: {release_error:#}"
            ))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn consume_human_approval_with_audit_and_locked<T, F>(
        &self,
        space_id: &str,
        token: &str,
        operation: &str,
        action: Action,
        resource: &ResourceRef,
        intent_hash: &str,
        actor_principal_id: Uuid,
        actor_credential_id: Uuid,
        audit_events: F,
        durable: &Option<DurableAuthorizationLease>,
        lease: &AuthorizationLease,
        mutation: impl for<'a> FnOnce(
            &'a AuthorizationLease,
        ) -> Pin<Box<dyn Future<Output = T> + Send + 'a>>,
    ) -> Result<(HumanApproval, T)>
    where
        F: Fn(Option<&HumanApproval>, &str, &str, &str) -> Vec<(Uuid, Value)>,
    {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));
        let mut state = self.state(space_id).await?;
        let Some(approval) = state
            .human_approvals
            .values()
            .find(|approval| approval.token_hash == token_hash)
            .cloned()
        else {
            queue_human_approval_audit_events(
                &mut state,
                audit_events(None, "rejected", "error", "error"),
            );
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow!("authorization revision overflow"))?;
            self.write_human_approval_state(space_id, &state, None, durable)
                .await?;
            return Err(AppError::forbidden("HUMAN_APPROVAL_INVALID").into());
        };
        if approval.consumed_at.is_some() {
            queue_human_approval_audit_events(
                &mut state,
                audit_events(Some(&approval), "replayed", "error", "error"),
            );
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow!("authorization revision overflow"))?;
            self.write_human_approval_state(space_id, &state, Some(approval.approval_id), durable)
                .await?;
            return Err(
                AppError::conflict(ErrorCode::InvalidInput, "HUMAN_APPROVAL_REPLAYED").into(),
            );
        }
        let expires_at = DateTime::parse_from_rfc3339(&approval.expires_at)
            .context("invalid stored approval timestamp")?
            .with_timezone(&Utc);
        if expires_at <= Utc::now() {
            queue_human_approval_audit_events(
                &mut state,
                audit_events(Some(&approval), "expired", "error", "error"),
            );
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow!("authorization revision overflow"))?;
            self.write_human_approval_state(space_id, &state, Some(approval.approval_id), durable)
                .await?;
            return Err(
                AppError::expired(ErrorCode::InvalidInput, "HUMAN_APPROVAL_EXPIRED").into(),
            );
        }
        let issuer_authorized = match effective_actions_for_state(
            &state,
            approval.issuer_principal_id,
            Some(&approval.resource),
        ) {
            Ok(actions) => actions.contains(&approval.action),
            Err(error)
                if error.downcast_ref::<AppError>().is_some_and(|error| {
                    error.kind() == ugoite_core::error::ErrorKind::Forbidden
                }) =>
            {
                false
            }
            Err(error) => return Err(error),
        };
        let actor_authorized = match effective_actions_for_state(
            &state,
            approval.actor_principal_id,
            Some(&approval.resource),
        ) {
            Ok(actions) => actions.contains(&approval.action),
            Err(error)
                if error.downcast_ref::<AppError>().is_some_and(|error| {
                    error.kind() == ugoite_core::error::ErrorKind::Forbidden
                }) =>
            {
                false
            }
            Err(error) => return Err(error),
        };
        if approval.operation != operation
            || approval.action != action
            || approval.resource != *resource
            || approval.intent_hash != intent_hash
            || approval.actor_principal_id != actor_principal_id
            || approval.actor_credential_id != actor_credential_id
            || state
                .principal_lifecycle_epochs
                .get(&approval.issuer_principal_id)
                .copied()
                != Some(approval.issuer_lifecycle_epoch)
            || state
                .principal_lifecycle_epochs
                .get(&approval.actor_principal_id)
                .copied()
                != Some(approval.actor_lifecycle_epoch)
            || !issuer_authorized
            || !actor_authorized
        {
            queue_human_approval_audit_events(
                &mut state,
                audit_events(Some(&approval), "rejected", "error", "error"),
            );
            state.revision = state
                .revision
                .checked_add(1)
                .ok_or_else(|| anyhow!("authorization revision overflow"))?;
            self.write_human_approval_state(space_id, &state, Some(approval.approval_id), durable)
                .await?;
            return Err(AppError::forbidden("HUMAN_APPROVAL_INVALID").into());
        }
        queue_human_approval_audit_events(
            &mut state,
            audit_events(Some(&approval), "consumed", "error", "unknown"),
        );
        let consumed_at = now_iso();
        state
            .human_approvals
            .get_mut(&approval.approval_id)
            .expect("approval was found")
            .consumed_at = Some(consumed_at);
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        self.write_human_approval_state(space_id, &state, Some(approval.approval_id), durable)
            .await?;
        let mutation = lease
            .run_while_held(|| mutation(lease))
            .await
            .map_err(|_| anyhow!("Space authorization mutation lease was lost"))?;
        Ok((approval, mutation))
    }

    async fn write_human_approval_state(
        &self,
        space_id: &str,
        state: &AuthorizationState,
        approval_id: Option<Uuid>,
        durable: &Option<DurableAuthorizationLease>,
    ) -> Result<()> {
        if let Some(durable) = durable {
            durable.ensure_held()?
        }
        let write_result = match durable {
            Some(_) => self.write_state_inner(space_id, state).await,
            None => self.write_state(space_id, state).await,
        };
        match write_result {
            Ok(()) => Ok(()),
            Err(error) => {
                // A remote CAS response can be lost after this exact state is
                // durably committed. In that case the approval is consumed,
                // but the mutation callback has not run; reporting replay
                // would falsely imply that another caller won the mutation.
                // Fail closed with an explicit unknown outcome so operators
                // reconcile the durable state before retrying the operation.
                let observed = self.state(space_id).await.ok();
                if observed.as_ref().is_some_and(|current| {
                    serde_json::to_vec_pretty(current)
                        .ok()
                        .zip(serde_json::to_vec_pretty(state).ok())
                        .is_some_and(|(observed, expected)| observed == expected)
                }) {
                    return Err(anyhow!("HUMAN_APPROVAL_OUTCOME_UNKNOWN"));
                }
                // Attribute the consumed transition to this caller before
                // classifying a non-identical state as a replay. A later
                // writer may have changed an unrelated field after this
                // caller's CAS committed, so `consumed_at.is_some()` alone
                // is not evidence that another consumer won.
                let desired_consumed_at = approval_id.and_then(|approval_id| {
                    state
                        .human_approvals
                        .get(&approval_id)
                        .and_then(|approval| approval.consumed_at.as_deref())
                });
                if desired_consumed_at.is_some_and(|desired_consumed_at| {
                    observed
                        .as_ref()
                        .and_then(|current| current.human_approvals.get(&approval_id?))
                        .and_then(|approval| approval.consumed_at.as_deref())
                        == Some(desired_consumed_at)
                }) {
                    return Err(anyhow!("HUMAN_APPROVAL_OUTCOME_UNKNOWN"));
                }
                if approval_id.is_some_and(|approval_id| {
                    observed
                        .as_ref()
                        .and_then(|current| current.human_approvals.get(&approval_id))
                        .and_then(|approval| approval.consumed_at.as_deref())
                        .is_some()
                }) {
                    return Err(AppError::conflict(
                        ErrorCode::InvalidInput,
                        "HUMAN_APPROVAL_REPLAYED",
                    )
                    .into());
                }
                Err(error)
            }
        }
    }

    pub async fn queue_human_approval_audit(
        &self,
        space_id: &str,
        event_id: Uuid,
        event: Value,
    ) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        if state
            .human_approval_audit_outbox
            .get(&event_id)
            .is_some_and(|record| record.delivered)
        {
            return Ok(());
        }
        let sequence = state
            .human_approval_audit_outbox
            .get(&event_id)
            .map(|record| record.sequence)
            .unwrap_or_else(|| next_human_approval_audit_sequence(&state));
        state.human_approval_audit_outbox.insert(
            event_id,
            HumanApprovalAuditOutbox {
                event_id,
                event,
                delivered: false,
                sequence,
            },
        );
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        self.write_state(space_id, &state).await
    }

    pub async fn pending_human_approval_audits(
        &self,
        space_id: &str,
    ) -> Result<Vec<HumanApprovalAuditOutbox>> {
        let mut records = self
            .state(space_id)
            .await?
            .human_approval_audit_outbox
            .values()
            .filter(|record| !record.delivered)
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by_key(|record| (record.sequence, record.event_id));
        Ok(records)
    }

    pub async fn mark_human_approval_audit_delivered(
        &self,
        space_id: &str,
        event_id: Uuid,
    ) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        if let Some(record) = state.human_approval_audit_outbox.get_mut(&event_id) {
            if !record.delivered {
                record.delivered = true;
                state.revision = state
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("authorization revision overflow"))?;
                self.write_state(space_id, &state).await?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reserve_recovery_fence(
        &self,
        space_id: &str,
        request_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        target_principal_id: Uuid,
        target_account_id: Uuid,
        issuer_generation: u64,
        target_generation: u64,
        ttl: chrono::Duration,
    ) -> Result<RecoveryFence> {
        self.reserve_recovery_fence_with_id(
            space_id,
            request_id,
            Uuid::now_v7(),
            issuer_principal_id,
            issuer_account_id,
            target_principal_id,
            target_account_id,
            issuer_generation,
            target_generation,
            ttl,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reserve_recovery_fence_with_id(
        &self,
        space_id: &str,
        request_id: Uuid,
        fence_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        target_principal_id: Uuid,
        target_account_id: Uuid,
        issuer_generation: u64,
        target_generation: u64,
        ttl: chrono::Duration,
    ) -> Result<RecoveryFence> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .reserve_recovery_fence_with_id_with_lease(
                space_id,
                request_id,
                fence_id,
                issuer_principal_id,
                issuer_account_id,
                target_principal_id,
                target_account_id,
                issuer_generation,
                target_generation,
                ttl,
                &lease,
            )
            .await;
        finish_authorization_lease(lease, result).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reserve_recovery_fence_with_id_with_lease(
        &self,
        space_id: &str,
        request_id: Uuid,
        fence_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        target_principal_id: Uuid,
        target_account_id: Uuid,
        issuer_generation: u64,
        target_generation: u64,
        ttl: chrono::Duration,
        lease: &AuthorizationLease,
    ) -> Result<RecoveryFence> {
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        if state.space_uid == Uuid::nil() {
            bail!("recovery fence is unavailable")
        }
        if let Some(existing) = state.recovery_fences.get(&fence_id).cloned() {
            if existing.status == "active"
                && existing.request_id == request_id
                && existing.space_uid == state.space_uid
                && existing.issuer_principal_id == issuer_principal_id
                && existing.issuer_account_id == issuer_account_id
                && existing.target_principal_id == target_principal_id
                && existing.target_account_id == target_account_id
                && existing.issuer_generation == issuer_generation
                && existing.target_generation == target_generation
            {
                // A retry may be recovering the result of a Space CAS whose
                // response was lost. Reusing the exact fence identity is
                // idempotent; a different tuple must never borrow it.
                return Ok(existing);
            }
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        if state
            .recovery_fences
            .values()
            .any(|fence| fence.status == "active")
        {
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        let _issuer = state
            .principals
            .get(&issuer_principal_id)
            .filter(|principal| {
                matches!(principal.kind, PrincipalKind::Human)
                    && matches!(principal.state, PrincipalState::Active)
            })
            .ok_or_else(|| anyhow!("recovery issuer is not active"))?;
        if !state
            .memberships
            .get(&issuer_principal_id)
            .is_some_and(|membership| matches!(membership.role, SpaceRole::Owner))
            || issuer_principal_id == target_principal_id
            || issuer_account_id == target_account_id
        {
            bail!("recovery fence tuple is invalid")
        }
        state
            .principals
            .get(&target_principal_id)
            .filter(|principal| {
                matches!(principal.kind, PrincipalKind::Human)
                    && matches!(principal.state, PrincipalState::Active)
            })
            .ok_or_else(|| anyhow!("recovery target is not active"))?;
        if !state.memberships.contains_key(&target_principal_id) {
            bail!("recovery target is not a Space member")
        }
        let fence = RecoveryFence {
            fence_id,
            request_id,
            space_uid: state.space_uid,
            issuer_principal_id,
            issuer_account_id,
            target_principal_id,
            target_account_id,
            authorization_revision: state.revision,
            issuer_space_lifecycle_epoch: state
                .principal_lifecycle_epochs
                .get(&issuer_principal_id)
                .copied()
                .unwrap_or_default(),
            target_space_lifecycle_epoch: state
                .principal_lifecycle_epochs
                .get(&target_principal_id)
                .copied()
                .unwrap_or_default(),
            issuer_generation,
            target_generation,
            expires_at: (Utc::now() + ttl).to_rfc3339_opts(SecondsFormat::Millis, true),
            status: "active".to_string(),
        };
        state.recovery_fences.insert(fence.fence_id, fence.clone());
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        self.write_state_with_lease(space_id, &state, lease).await?;
        Ok(fence)
    }

    pub async fn recovery_fence(&self, space_id: &str, fence_id: Uuid) -> Result<RecoveryFence> {
        self.state(space_id)
            .await?
            .recovery_fences
            .get(&fence_id)
            .cloned()
            .ok_or_else(|| anyhow!("recovery fence is unavailable"))
    }

    pub async fn complete_recovery_fence(&self, space_id: &str, fence_id: Uuid) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .complete_recovery_fence_with_lease(space_id, fence_id, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn complete_recovery_fence_with_lease(
        &self,
        space_id: &str,
        fence_id: Uuid,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        let fence = state
            .recovery_fences
            .get(&fence_id)
            .ok_or_else(|| anyhow!("recovery fence is unavailable"))?;
        if fence.status == "completed" {
            return Ok(());
        }
        if fence.status != "active" {
            bail!("recovery fence is not active")
        }
        if DateTime::parse_from_rfc3339(&fence.expires_at)
            .map(|expires_at| expires_at.with_timezone(&Utc) <= Utc::now())
            .map_err(|_| anyhow!("invalid stored recovery fence timestamp"))?
        {
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        let expected_revision = fence
            .authorization_revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        if state.revision != expected_revision {
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        if state
            .principal_lifecycle_epochs
            .get(&fence.issuer_principal_id)
            .copied()
            != Some(fence.issuer_space_lifecycle_epoch)
            || state
                .principal_lifecycle_epochs
                .get(&fence.target_principal_id)
                .copied()
                != Some(fence.target_space_lifecycle_epoch)
        {
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        state
            .recovery_fences
            .get_mut(&fence_id)
            .expect("fence was checked above")
            .status = "completed".to_string();
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        self.write_state_with_lease(space_id, &state, lease).await
    }

    pub async fn release_recovery_fence(&self, space_id: &str, fence_id: Uuid) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .release_recovery_fence_with_lease(space_id, fence_id, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn release_recovery_fence_with_lease(
        &self,
        space_id: &str,
        fence_id: Uuid,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        if let Some(fence) = state.recovery_fences.get_mut(&fence_id) {
            if fence.status == "active" {
                fence.status = "released".to_string();
                state.revision = state
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("authorization revision overflow"))?;
                self.write_state_with_lease(space_id, &state, lease).await?;
            }
        }
        Ok(())
    }

    fn ensure_recovery_mutation_allowed(&self, state: &mut AuthorizationState) -> Result<()> {
        for fence in state
            .recovery_fences
            .values()
            .filter(|fence| fence.status == "active")
        {
            DateTime::parse_from_rfc3339(&fence.expires_at)
                .context("invalid stored recovery fence timestamp")?;
        }
        if state
            .recovery_fences
            .values()
            .any(|fence| fence.status == "active")
        {
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        Ok(())
    }

    pub async fn require(
        &self,
        space_id: &str,
        principal_id: Uuid,
        action: Action,
        resource: Option<&ResourceRef>,
    ) -> Result<()> {
        if self
            .effective_actions(space_id, principal_id, resource)
            .await?
            .contains(&action)
        {
            let state = self.state(space_id).await?;
            if state
                .principals
                .get(&principal_id)
                .is_some_and(|principal| matches!(principal.kind, PrincipalKind::Agent))
            {
                let target = resource
                    .map(ResourceRef::key)
                    .unwrap_or_else(|| "space".to_string());
                let action_name = format!("{action:?}").to_lowercase();
                audit::append_audit_event(
                    &self.operator,
                    space_id,
                    &serde_json::json!({
                        "action": format!("agent.authorization.{action_name}"),
                        "subject_principal_id": principal_id,
                        "actor_principal_id": principal_id,
                        "target_type": "authorization",
                        "target_id": target,
                        "outcome": "success"
                    }),
                    None,
                )
                .await?;
            }
            return Ok(());
        }
        let target = resource
            .map(ResourceRef::key)
            .unwrap_or_else(|| "space".to_string());
        let _ = audit::append_audit_event(
            &self.operator,
            space_id,
            &serde_json::json!({
                "action": "authorization.denied",
                "subject_principal_id": principal_id,
                "actor_principal_id": principal_id,
                "outcome": "deny",
                "target_type": "authorization",
                "target_id": target,
                "metadata": {"required_action": action}
            }),
            None,
        )
        .await;
        Err(
            AppError::forbidden(format!("principal lacks {action:?} permission on {target}"))
                .into(),
        )
    }

    pub async fn set_policy(
        &self,
        space_id: &str,
        actor: Uuid,
        resource: &ResourceRef,
        policy: AccessPolicy,
    ) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .set_policy_with_lease(space_id, actor, resource, policy, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    /// Apply a policy without emitting the generic policy audit event. The
    /// dangerous-operation route uses this after a human approval has already
    /// established the causal audit sequence and appends its mutation result
    /// afterward.
    pub async fn set_policy_without_audit(
        &self,
        space_id: &str,
        actor: Uuid,
        resource: &ResourceRef,
        policy: AccessPolicy,
    ) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .set_policy_without_audit_with_lease(space_id, actor, resource, policy, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn set_policy_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        resource: &ResourceRef,
        policy: AccessPolicy,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, Some(resource))
            .await?;
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        self.set_policy_locked(
            space_id,
            actor,
            resource,
            policy,
            true,
            lease.durable.as_ref(),
        )
        .await
    }

    pub async fn set_policy_without_audit_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        resource: &ResourceRef,
        policy: AccessPolicy,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, Some(resource))
            .await?;
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        self.set_policy_locked(
            space_id,
            actor,
            resource,
            policy,
            false,
            lease.durable.as_ref(),
        )
        .await
    }

    /// Apply a policy from inside an approval-bound mutation. The caller must
    /// already hold the authorizer write lock, which is what keeps ACL
    /// revocation from racing the approved mutation.
    pub async fn set_policy_after_approval(
        &self,
        space_id: &str,
        actor: Uuid,
        resource: &ResourceRef,
        policy: AccessPolicy,
    ) -> Result<()> {
        self.set_policy_locked(space_id, actor, resource, policy, false, None)
            .await
    }

    /// Apply a policy from an approval callback while reusing the callback's
    /// already-held authorization lease.
    pub async fn set_policy_after_approval_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        resource: &ResourceRef,
        policy: AccessPolicy,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        self.set_policy_locked(
            space_id,
            actor,
            resource,
            policy,
            false,
            lease.durable.as_ref(),
        )
        .await
    }

    async fn set_policy_locked(
        &self,
        space_id: &str,
        actor: Uuid,
        resource: &ResourceRef,
        policy: AccessPolicy,
        append_audit: bool,
        durable: Option<&DurableAuthorizationLease>,
    ) -> Result<()> {
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&mut state)?;
        for grant in &policy.grants {
            let Some(principal) = state.principals.get(&grant.principal_id) else {
                bail!("policy references a principal outside the space");
            };
            if matches!(principal.kind, PrincipalKind::Agent)
                && (grant.actions.contains(&Action::Delete)
                    || grant.actions.contains(&Action::Share))
                && !state
                    .memberships
                    .get(&actor)
                    .is_some_and(|membership| matches!(membership.role, SpaceRole::Owner))
            {
                bail!("only a Space owner may grant agent delete or share actions")
            }
        }
        let resource_key = resource.key();
        state.policies.insert(resource_key.clone(), policy.clone());
        state
            .policy_history
            .entry(resource_key)
            .or_default()
            .push(PolicyRevision {
                policy,
                changed_at: now_iso(),
                actor_principal_id: actor,
            });
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        match durable {
            Some(durable) => {
                self.write_state_with_durable(space_id, &state, durable)
                    .await?
            }
            None => self.write_state(space_id, &state).await?,
        }
        if append_audit {
            audit::append_audit_event(
                &self.operator,
                space_id,
                &serde_json::json!({
                    "action": "authorization.policy.updated",
                    "subject_principal_id": actor,
                    "actor_principal_id": actor,
                    "target_type": "authorization_policy",
                    "target_id": resource.key(),
                }),
                None,
            )
            .await?;
        }
        Ok(())
    }

    pub async fn add_human_member(
        &self,
        space_id: &str,
        actor: Uuid,
        principal: SpacePrincipal,
        role: SpaceRole,
    ) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .add_human_member_with_lease(space_id, actor, principal, role, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    /// Add a member while the caller already owns the Space authorization
    /// lease. This is used by invitation acceptance and other compound Node +
    /// Space mutations; acquiring another lease inside the callback would
    /// deadlock on shared backends.
    pub async fn add_human_member_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        principal: SpacePrincipal,
        role: SpaceRole,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        if !matches!(principal.kind, PrincipalKind::Human) {
            bail!("human member must use a human principal");
        }
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&mut state)?;
        if let Some(existing) = state.principals.get(&principal.principal_id) {
            if existing.kind == principal.kind
                && state.memberships.contains_key(&principal.principal_id)
            {
                return Ok(());
            }
            bail!("principal already exists with conflicting state");
        }
        let principal_id = principal.principal_id;
        let created_at = principal.created_at.clone();
        state.principals.insert(principal_id, principal);
        state.memberships.insert(
            principal_id,
            Membership {
                principal_id,
                role,
                created_at,
            },
        );
        state
            .principal_lifecycle_epochs
            .entry(principal_id)
            .or_insert(1);
        state.revision += 1;
        self.write_state_with_lease(space_id, &state, lease).await?;
        audit::append_audit_event(
            &self.operator,
            space_id,
            &serde_json::json!({
                "action": "principal.activated",
                "subject_principal_id": principal_id,
                "actor_principal_id": actor,
                "target_type": "space_principal",
                "target_id": principal_id,
            }),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn resource_policy_history(
        &self,
        space_id: &str,
        resource: &ResourceRef,
    ) -> Result<Vec<PolicyRevision>> {
        Ok(self
            .state(space_id)
            .await?
            .policy_history
            .get(&resource.key())
            .cloned()
            .unwrap_or_default())
    }

    pub async fn create_agent(
        &self,
        space_id: &str,
        actor: Uuid,
        request: CreateAgentRequest,
    ) -> Result<AgentPrincipal> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .create_agent_with_lease(space_id, actor, request, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn create_agent_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        request: CreateAgentRequest,
        lease: &AuthorizationLease,
    ) -> Result<AgentPrincipal> {
        let CreateAgentRequest {
            display_name,
            description,
            mode,
            owner_principal_ids,
            granted_actions,
            expires_at,
        } = request;
        self.require(space_id, actor, Action::Share, None).await?;
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        if granted_actions.contains(&Action::Delete) || granted_actions.contains(&Action::Share) {
            bail!("agents cannot receive delete or share actions");
        }
        if expires_at.is_none() {
            bail!("agent expiry or review deadline is required");
        }
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&mut state)?;
        if owner_principal_ids.is_empty() || !owner_principal_ids.contains(&actor) {
            bail!("agent sponsor must be one of at least one human owner");
        }
        if owner_principal_ids
            .iter()
            .any(|id| !is_active_space_owner(&state, *id))
        {
            bail!("all agent owners must be active Space owners");
        }
        let agent_id = Uuid::now_v7();
        let now = now_iso();
        let agent = AgentPrincipal {
            agent_id,
            display_name: display_name.trim().to_string(),
            description: description.trim().to_string(),
            sponsor_principal_id: actor,
            owner_principal_ids,
            mode,
            status: PrincipalState::Active,
            created_at: now.clone(),
            expires_at,
            last_used_at: None,
        };
        agent.validate()?;
        state.principals.insert(
            agent_id,
            SpacePrincipal {
                principal_id: agent_id,
                kind: PrincipalKind::Agent,
                display_name: agent.display_name.clone(),
                state: PrincipalState::Active,
                created_at: now,
            },
        );
        state.principal_lifecycle_epochs.insert(agent_id, 1);
        state.agent_grants.insert(agent_id, granted_actions);
        state.agents.insert(agent_id, agent.clone());
        state.revision += 1;
        self.write_state_with_lease(space_id, &state, lease).await?;
        audit::append_audit_event(
            &self.operator,
            space_id,
            &serde_json::json!({
                "action": "agent.created",
                "subject_principal_id": agent_id,
                "actor_principal_id": actor,
                "target_type": "agent",
                "target_id": agent_id,
            }),
            None,
        )
        .await?;
        Ok(agent)
    }

    /// Returns the active agent produced by the same request, if one exists.
    /// The server uses this as the retry coordinate when the Space write has
    /// committed but the subsequent Node credential/audit write did not.
    pub async fn find_agent_for_request(
        &self,
        space_id: &str,
        actor: Uuid,
        request: &CreateAgentRequest,
    ) -> Result<Option<AgentPrincipal>> {
        let state = self.state(space_id).await?;
        Ok(state
            .agents
            .values()
            .find(|agent| {
                matches!(agent.status, PrincipalState::Active)
                    && agent.sponsor_principal_id == actor
                    && agent.display_name == request.display_name.trim()
                    && agent.description == request.description.trim()
                    && agent.mode == request.mode
                    && agent.owner_principal_ids == request.owner_principal_ids
                    && agent.expires_at == request.expires_at
                    && state
                        .agent_grants
                        .get(&agent.agent_id)
                        .is_some_and(|actions| actions == &request.granted_actions)
            })
            .cloned())
    }

    /// Create an agent, or recover the Space half of a request whose later
    /// Node operation failed. Re-reading after an error also covers an
    /// ambiguous response where the authorization CAS committed before the
    /// caller observed the error.
    pub async fn create_or_recover_agent_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        request: CreateAgentRequest,
        lease: &AuthorizationLease,
    ) -> Result<AgentPrincipal> {
        if let Some(agent) = self
            .find_agent_for_request(space_id, actor, &request)
            .await?
        {
            return Ok(agent);
        }
        match self
            .create_agent_with_lease(space_id, actor, request.clone(), lease)
            .await
        {
            Ok(agent) => Ok(agent),
            Err(error) => self
                .find_agent_for_request(space_id, actor, &request)
                .await?
                .ok_or(error),
        }
    }

    pub async fn revoke_agent(&self, space_id: &str, actor: Uuid, agent_id: Uuid) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .revoke_agent_with_lease(space_id, actor, agent_id, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn revoke_agent_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        agent_id: Uuid,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&mut state)?;
        let agent = state
            .agents
            .get_mut(&agent_id)
            .ok_or_else(|| anyhow!("agent not found"))?;
        if actor != agent.sponsor_principal_id && !agent.owner_principal_ids.contains(&actor) {
            bail!("agent sponsor or owner is required");
        }
        agent.status = PrincipalState::Revoked;
        if let Some(principal) = state.principals.get_mut(&agent_id) {
            principal.state = PrincipalState::Revoked;
        }
        *state
            .principal_lifecycle_epochs
            .entry(agent_id)
            .or_insert(0) += 1;
        state.agent_grants.remove(&agent_id);
        state.revision += 1;
        self.write_state_with_lease(space_id, &state, lease).await?;
        audit::append_audit_event(
            &self.operator,
            space_id,
            &serde_json::json!({
                "action": "agent.revoked",
                "subject_principal_id": agent_id,
                "actor_principal_id": actor,
                "target_type": "agent",
                "target_id": agent_id,
            }),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn mark_agent_used(&self, space_id: &str, agent_id: Uuid) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .mark_agent_used_with_lease(space_id, agent_id, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn mark_agent_used_with_lease(
        &self,
        space_id: &str,
        agent_id: Uuid,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&mut state)?;
        let agent = state
            .agents
            .get_mut(&agent_id)
            .filter(|agent| matches!(agent.status, PrincipalState::Active))
            .ok_or_else(|| anyhow!("agent is not active"))?;
        agent.last_used_at = Some(now_iso());
        state.revision += 1;
        self.write_state_with_lease(space_id, &state, lease).await
    }

    pub async fn change_role(
        &self,
        space_id: &str,
        actor: Uuid,
        principal_id: Uuid,
        role: SpaceRole,
    ) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .change_role_with_lease(space_id, actor, principal_id, role, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn change_role_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        principal_id: Uuid,
        role: SpaceRole,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&mut state)?;
        let current = state
            .memberships
            .get(&principal_id)
            .ok_or_else(|| anyhow!("member not found"))?;
        if matches!(current.role, SpaceRole::Owner)
            && !matches!(role, SpaceRole::Owner)
            && owner_count(&state) == 1
        {
            return Err(AppError::conflict(
                ErrorCode::LastAdminRequired,
                "cannot demote the last Space owner",
            )
            .into());
        }
        let demoting = !matches!(&role, SpaceRole::Owner);
        state
            .memberships
            .get_mut(&principal_id)
            .expect("checked membership")
            .role = role;
        let revoked_agents = if demoting {
            state
                .agents
                .iter()
                .filter(|(_, agent)| agent.owner_principal_ids.contains(&principal_id))
                .map(|(agent_id, _)| *agent_id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for agent_id in &revoked_agents {
            if let Some(agent) = state.agents.get_mut(agent_id) {
                agent.status = PrincipalState::Revoked;
            }
            if let Some(principal) = state.principals.get_mut(agent_id) {
                principal.state = PrincipalState::Revoked;
            }
            state.agent_grants.remove(agent_id);
            *state
                .principal_lifecycle_epochs
                .entry(*agent_id)
                .or_insert(0) += 1;
        }
        *state
            .principal_lifecycle_epochs
            .entry(principal_id)
            .or_insert(0) += 1;
        state.revision += 1;
        self.write_state_with_lease(space_id, &state, lease).await?;
        audit::append_audit_event(
            &self.operator,
            space_id,
            &serde_json::json!({
                "action": "principal.role_changed",
                "subject_principal_id": principal_id,
                "actor_principal_id": actor,
                "target_type": "space_principal",
                "target_id": principal_id,
            }),
            None,
        )
        .await?;
        for agent_id in revoked_agents {
            audit::append_audit_event(
                &self.operator,
                space_id,
                &serde_json::json!({
                    "action": "agent.revoked",
                    "subject_principal_id": agent_id,
                    "actor_principal_id": actor,
                    "target_type": "agent",
                    "target_id": agent_id,
                    "reason": "owner_demoted",
                }),
                None,
            )
            .await?;
        }
        Ok(())
    }

    pub async fn revoke_principal(
        &self,
        space_id: &str,
        actor: Uuid,
        principal_id: Uuid,
    ) -> Result<()> {
        let (_, lease) = self.acquire_state_lease(space_id).await?;
        let result = self
            .revoke_principal_with_lease(space_id, actor, principal_id, &lease)
            .await;
        finish_authorization_lease(lease, result).await
    }

    pub async fn revoke_principal_with_lease(
        &self,
        space_id: &str,
        actor: Uuid,
        principal_id: Uuid,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        lease
            .durable
            .as_ref()
            .map_or(Ok(()), DurableAuthorizationLease::ensure_held)?;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&mut state)?;
        if state
            .principals
            .get(&principal_id)
            .is_some_and(|principal| matches!(principal.kind, PrincipalKind::Agent))
        {
            bail!("agent principals must be revoked through revoke_agent");
        }
        if state
            .memberships
            .get(&principal_id)
            .is_some_and(|membership| matches!(membership.role, SpaceRole::Owner))
            && owner_count(&state) == 1
        {
            return Err(AppError::conflict(
                ErrorCode::LastAdminRequired,
                "cannot revoke the last Space owner",
            )
            .into());
        }
        let principal = state
            .principals
            .get_mut(&principal_id)
            .ok_or_else(|| anyhow!("principal not found"))?;
        principal.state = PrincipalState::Revoked;
        *state
            .principal_lifecycle_epochs
            .entry(principal_id)
            .or_insert(0) += 1;
        state.revision += 1;
        self.write_state_with_lease(space_id, &state, lease).await?;
        audit::append_audit_event(
            &self.operator,
            space_id,
            &serde_json::json!({
                "action": "principal.revoked",
                "subject_principal_id": principal_id,
                "actor_principal_id": actor,
                "target_type": "space_principal",
                "target_id": principal_id,
            }),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn filter_authorized_resources(
        &self,
        space_id: &str,
        principal_id: Uuid,
        resources: impl IntoIterator<Item = ResourceRef>,
        action: Action,
    ) -> Result<BTreeSet<String>> {
        let mut allowed = BTreeSet::new();
        for resource in resources {
            if self
                .effective_actions(space_id, principal_id, Some(&resource))
                .await?
                .contains(&action)
            {
                allowed.insert(resource.id);
            }
        }
        Ok(allowed)
    }

    async fn write_state(&self, space_id: &str, state: &AuthorizationState) -> Result<()> {
        let durable = self.acquire_durable_mutation_lease(space_id).await?;
        let result = self.write_state_inner(space_id, state).await;
        let release = if let Some(durable) = durable {
            durable.release().await
        } else {
            Ok(())
        };
        match (result, release) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(release_error)) => Err(error.context(format!(
                "release Space authorization mutation lease: {release_error:#}"
            ))),
        }
    }

    async fn write_state_with_durable(
        &self,
        space_id: &str,
        state: &AuthorizationState,
        durable: &DurableAuthorizationLease,
    ) -> Result<()> {
        durable.ensure_held()?;
        self.write_state_inner(space_id, state).await
    }

    async fn write_state_with_lease(
        &self,
        space_id: &str,
        state: &AuthorizationState,
        lease: &AuthorizationLease,
    ) -> Result<()> {
        match lease.durable.as_ref() {
            Some(durable) => {
                self.write_state_with_durable(space_id, state, durable)
                    .await
            }
            None => self.write_state_inner(space_id, state).await,
        }
    }

    async fn write_state_inner(&self, space_id: &str, state: &AuthorizationState) -> Result<()> {
        self.ensure_authoritative_mutation_contract()?;
        let path = state_path(space_id);
        validate_authorization_state(state)?;
        let serialized = serde_json::to_vec_pretty(state)?;
        if serialized.len() > MAX_AUTHORIZATION_STATE_BYTES {
            bail!(
                "Space authorization state exceeds the {} byte limit",
                MAX_AUTHORIZATION_STATE_BYTES
            );
        }
        let capabilities = self.operator.info().capability();
        let _local_lock = self.local_authorization_lock(space_id)?;

        if state.revision == 1 {
            if capabilities.write_with_if_not_exists {
                self.operator
                    .write_with(&path, serialized)
                    .if_not_exists(true)
                    .await
                    .context("atomically create Space authorization state")?;
            } else if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
                if self.operator.exists(&path).await? {
                    bail!("authorization state already exists");
                }
                self.operator.write(&path, serialized).await?;
            } else {
                bail!("Space authorization state requires conditional storage capabilities");
            }
            return Ok(());
        }

        let expected_revision = state
            .revision
            .checked_sub(1)
            .ok_or_else(|| anyhow!("invalid authorization revision"))?;
        if capabilities.write_with_if_match {
            let metadata = self
                .operator
                .stat(&path)
                .await
                .context("stat Space authorization state for compare-and-swap")?;
            let version = metadata
                .etag()
                .or_else(|| metadata.version())
                .ok_or_else(|| {
                    anyhow!("Space authorization object has no conditional-write version")
                })?
                .to_string();
            let current = read_authorization_state_bytes(&self.operator, &path, Some(&version))
                .await
                .context("read versioned Space authorization state")?;
            let current: AuthorizationState = serde_json::from_slice(&current)
                .context("decode versioned Space authorization state")?;
            validate_authorization_state(&current)?;
            if current.revision != expected_revision {
                bail!("Space authorization revision conflict");
            }
            if let Err(error) = self
                .operator
                .write_with(&path, serialized.clone())
                .if_match(&version)
                .await
            {
                let error: anyhow::Error = error.into();
                let error = error.context("compare-and-swap Space authorization state");
                // A remote conditional write may have committed before its
                // response was lost. Do not release the paired Node fence
                // until the Space outcome is classified. The exact desired
                // bytes prove this write committed; a failed verification is
                // deliberately treated as unknown and remains fenced.
                match self.state(space_id).await {
                    Ok(observed) => {
                        if serde_json::to_vec_pretty(&observed)
                            .ok()
                            .is_some_and(|value| value == serialized)
                        {
                            return Err(anyhow!(
                                "Space authorization write committed with an ambiguous response: {error}"
                            ));
                        }
                        return Err(error);
                    }
                    Err(read_error) => {
                        return Err(anyhow!(
                            "Space authorization write outcome unknown: {error}; verification failed: {read_error}"
                        ));
                    }
                }
            }
        } else if matches!(self.operator.info().scheme(), "memory" | "fs" | "file") {
            // Filesystem and in-memory adapters are serialized by the shared process lock.
            let current = self.state(space_id).await?;
            if current.revision != expected_revision {
                bail!("Space authorization revision conflict");
            }
            self.operator.write(&path, serialized).await?;
        } else {
            bail!("Space authorization state requires conditional storage capabilities");
        }
        #[cfg(test)]
        if self
            .ambiguous_write_with_post_commit_writer_once
            .swap(false, Ordering::SeqCst)
        {
            let mut later_state = state.clone();
            later_state.revision = later_state
                .revision
                .checked_add(1)
                .expect("test revision does not overflow");
            self.operator
                .write(&path, serde_json::to_vec_pretty(&later_state)?)
                .await?;
            return Err(anyhow!(
                "injected ambiguous authorization CAS response after a later writer"
            ));
        }
        #[cfg(test)]
        if self.ambiguous_write_once.swap(false, Ordering::SeqCst) {
            return Err(anyhow!("injected ambiguous authorization CAS response"));
        }
        Ok(())
    }

    fn local_authorization_lock(&self, space_id: &str) -> Result<Option<std::fs::File>> {
        if !matches!(self.operator.info().scheme(), "fs" | "file") {
            return Ok(None);
        }
        let root_value = self.operator.info().root();
        let root = Path::new(root_value.as_str());
        let lock_path = root
            .join("spaces")
            .join(space_id)
            .join("security/principals.json.lock");
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        file.lock_exclusive()?;
        Ok(Some(file))
    }
}

fn queue_human_approval_audit_events(state: &mut AuthorizationState, events: Vec<(Uuid, Value)>) {
    for (event_id, event) in events {
        if state
            .human_approval_audit_outbox
            .get(&event_id)
            .is_some_and(|record| record.delivered)
        {
            continue;
        }
        let sequence = state
            .human_approval_audit_outbox
            .get(&event_id)
            .map(|record| record.sequence)
            .unwrap_or_else(|| next_human_approval_audit_sequence(state));
        state.human_approval_audit_outbox.insert(
            event_id,
            HumanApprovalAuditOutbox {
                event_id,
                event,
                delivered: false,
                sequence,
            },
        );
    }
}

fn next_human_approval_audit_sequence(state: &AuthorizationState) -> u64 {
    state
        .human_approval_audit_outbox
        .values()
        .map(|record| record.sequence)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn validate_authorization_state(state: &AuthorizationState) -> Result<()> {
    if state.schema_version != CURRENT_AUTHORIZATION_SCHEMA_VERSION {
        bail!(
            "unsupported Space authorization schema_version {}; expected {}",
            state.schema_version,
            CURRENT_AUTHORIZATION_SCHEMA_VERSION
        );
    }
    if state.revision == 0 {
        bail!("Space authorization revision must be positive");
    }
    for (principal_id, principal) in &state.principals {
        if *principal_id != principal.principal_id {
            bail!("Space principal map key does not match principal_id");
        }
        if !state.principal_lifecycle_epochs.contains_key(principal_id) {
            bail!("Space principal is missing a lifecycle epoch");
        }
        match principal.kind {
            PrincipalKind::Human if !state.memberships.contains_key(principal_id) => {
                bail!("Space human principal is missing a membership");
            }
            PrincipalKind::Agent if state.memberships.contains_key(principal_id) => {
                bail!("Space agent principal must not have a membership");
            }
            _ => {}
        }
        if matches!(principal.kind, PrincipalKind::Agent) != state.agents.contains_key(principal_id)
        {
            bail!("Space agent principal and agent record are inconsistent");
        }
    }
    for (principal_id, membership) in &state.memberships {
        if *principal_id != membership.principal_id {
            bail!("Space membership map key does not match principal_id");
        }
        if !state.principals.contains_key(principal_id) {
            bail!("Space membership references an unknown principal");
        }
        if !state
            .principals
            .get(principal_id)
            .is_some_and(|principal| matches!(principal.kind, PrincipalKind::Human))
        {
            bail!("Space membership must reference a human principal");
        }
    }
    for (agent_id, agent) in &state.agents {
        let Some(principal) = state.principals.get(agent_id) else {
            bail!("Space agent records are inconsistent");
        };
        if *agent_id != agent.agent_id
            || !matches!(principal.kind, PrincipalKind::Agent)
            || principal.state != agent.status
            || matches!(agent.status, PrincipalState::Active)
                != state.agent_grants.contains_key(agent_id)
        {
            bail!("Space agent records are inconsistent");
        }
        agent.validate()?;
        if matches!(agent.status, PrincipalState::Active)
            && (agent
                .owner_principal_ids
                .iter()
                .any(|owner_id| !is_active_space_owner(state, *owner_id))
                || !is_active_space_owner(state, agent.sponsor_principal_id))
        {
            bail!("active agent owners and sponsor must remain active Space owners");
        }
    }
    for principal_id in state.agent_grants.keys() {
        if !state.agents.contains_key(principal_id) {
            bail!("Space agent grants reference an unknown agent");
        }
    }
    for (principal_id, epoch) in &state.principal_lifecycle_epochs {
        if *epoch == 0 || !state.principals.contains_key(principal_id) {
            bail!("Space principal lifecycle epochs are inconsistent");
        }
    }
    if owner_count(state) == 0 {
        bail!("Space has no active human owner principal");
    }
    validate_authorization_state_limits(state)
}

fn validate_authorization_state_limits(state: &AuthorizationState) -> Result<()> {
    let collections = [
        ("principals", state.principals.len()),
        ("memberships", state.memberships.len()),
        ("policies", state.policies.len()),
        ("policy history", state.policy_history.len()),
        ("agents", state.agents.len()),
        ("agent grants", state.agent_grants.len()),
        (
            "principal lifecycle epochs",
            state.principal_lifecycle_epochs.len(),
        ),
        ("recovery fences", state.recovery_fences.len()),
        ("human approvals", state.human_approvals.len()),
        (
            "human approval audit outbox",
            state.human_approval_audit_outbox.len(),
        ),
    ];
    for (name, count) in collections {
        if count > MAX_AUTHORIZATION_MAP_ENTRIES {
            bail!(
                "Space authorization {name} exceeds the {MAX_AUTHORIZATION_MAP_ENTRIES} entry limit"
            );
        }
    }
    let history_revisions = state
        .policy_history
        .values()
        .try_fold(0usize, |total, revisions| {
            total.checked_add(revisions.len())
        })
        .context("Space authorization policy history size overflow")?;
    if history_revisions > MAX_AUTHORIZATION_POLICY_HISTORY_REVISIONS {
        bail!(
            "Space authorization policy history exceeds the {MAX_AUTHORIZATION_POLICY_HISTORY_REVISIONS} revision limit"
        );
    }
    Ok(())
}

/// Evaluates authorization against one already-read state snapshot. Query
/// session creation uses this to derive its scope and fingerprint from the
/// same state without an Entry-by-Entry OpenDAL reread.
pub fn effective_actions_for_state(
    state: &AuthorizationState,
    principal_id: Uuid,
    resource: Option<&ResourceRef>,
) -> Result<BTreeSet<Action>> {
    let principal = state
        .principals
        .get(&principal_id)
        .filter(|principal| matches!(principal.state, PrincipalState::Active))
        .ok_or_else(|| AppError::forbidden("principal is not active in this space"))?;
    if !matches!(principal.kind, PrincipalKind::Human | PrincipalKind::Agent) {
        return Ok(BTreeSet::new());
    }
    let mut effective = if matches!(principal.kind, PrincipalKind::Agent) {
        let agent = state
            .agents
            .get(&principal_id)
            .filter(|agent| matches!(agent.status, PrincipalState::Active))
            .ok_or_else(|| AppError::forbidden("agent is not active"))?;
        if agent
            .owner_principal_ids
            .iter()
            .any(|owner_id| !is_active_space_owner(state, *owner_id))
            || !is_active_space_owner(state, agent.sponsor_principal_id)
        {
            return Err(
                AppError::forbidden("agent sponsor or owner is not an active Space owner").into(),
            );
        }
        let expires_at = agent
            .expires_at
            .as_deref()
            .ok_or_else(|| AppError::forbidden("agent has no expiry or review deadline"))?;
        if chrono::DateTime::parse_from_rfc3339(expires_at)
            .context("invalid agent expiry")?
            .with_timezone(&Utc)
            <= Utc::now()
        {
            return Err(AppError::forbidden("agent expiry or review deadline has passed").into());
        }
        state
            .agent_grants
            .get(&principal_id)
            .cloned()
            .unwrap_or_default()
    } else {
        let membership = state
            .memberships
            .get(&principal_id)
            .ok_or_else(|| AppError::forbidden("principal is not a member of this space"))?;
        role_actions(&membership.role)
    };
    if let Some(resource) = resource {
        if let Some(parent) = resource.parent.as_deref() {
            effective =
                evaluate_policy(&effective, principal_id, state.policies.get(&parent.key()));
        }
        effective = evaluate_policy(
            &effective,
            principal_id,
            state.policies.get(&resource.key()),
        );
    }
    if matches!(principal.kind, PrincipalKind::Human)
        && state
            .memberships
            .get(&principal_id)
            .is_some_and(|membership| matches!(membership.role, SpaceRole::Owner))
    {
        effective.extend(role_actions(&SpaceRole::Owner));
    }
    Ok(effective)
}

fn owner_count(state: &AuthorizationState) -> usize {
    state
        .memberships
        .values()
        .filter(|membership| {
            matches!(membership.role, SpaceRole::Owner)
                && state
                    .principals
                    .get(&membership.principal_id)
                    .is_some_and(|p| matches!(p.state, PrincipalState::Active))
        })
        .count()
}

fn is_active_space_owner(state: &AuthorizationState, principal_id: Uuid) -> bool {
    state
        .principals
        .get(&principal_id)
        .is_some_and(|principal| {
            matches!(principal.kind, PrincipalKind::Human)
                && matches!(principal.state, PrincipalState::Active)
        })
        && state
            .memberships
            .get(&principal_id)
            .is_some_and(|membership| matches!(membership.role, SpaceRole::Owner))
}

fn state_path(space_id: &str) -> String {
    format!("spaces/{space_id}/{AUTHORIZATION_FILE}")
}

async fn read_authorization_state_bytes(
    operator: &Operator,
    path: &str,
    exact_version: Option<&str>,
) -> Result<Vec<u8>> {
    let metadata = operator.stat(path).await?;
    if metadata.content_length() > MAX_AUTHORIZATION_STATE_BYTES as u64 {
        bail!(
            "Space authorization state exceeds the {} byte limit",
            MAX_AUTHORIZATION_STATE_BYTES
        );
    }
    let mut reader = operator.reader_with(path);
    if let Some(version) = exact_version.or_else(|| metadata.etag().filter(|etag| !etag.is_empty()))
    {
        reader = reader.if_match(version);
    }
    let reader = reader.chunk(AUTHORIZATION_STATE_READER_CHUNK_BYTES).await?;
    let mut stream = reader.into_stream(0..).await?;
    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.content_length())
            .unwrap_or(MAX_AUTHORIZATION_STATE_BYTES)
            .min(MAX_AUTHORIZATION_STATE_BYTES),
    );
    while let Some(buffer) = stream.try_next().await? {
        bytes.extend(buffer.into_iter().flatten());
        if bytes.len() > MAX_AUTHORIZATION_STATE_BYTES {
            bail!(
                "Space authorization state exceeds the {} byte limit",
                MAX_AUTHORIZATION_STATE_BYTES
            );
        }
    }
    Ok(bytes)
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_domain::identity::Grant;
    use ugoite_storage::operator_from_uri;

    #[tokio::test]
    async fn authorizer_enforces_roles_and_last_owner() -> Result<()> {
        let op = operator_from_uri("memory://authorizer")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        authorizer
            .require("demo", owner, Action::Delete, None)
            .await?;
        assert!(authorizer
            .change_role("demo", owner, owner, SpaceRole::Viewer)
            .await
            .is_err());

        let viewer = Uuid::now_v7();
        authorizer
            .add_human_member(
                "demo",
                owner,
                SpacePrincipal {
                    principal_id: viewer,
                    kind: PrincipalKind::Human,
                    display_name: "Viewer".to_string(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        let entry = ResourceRef {
            kind: ResourceKind::Entry,
            id: "private-entry".to_string(),
            parent: None,
        };
        authorizer
            .set_policy(
                "demo",
                owner,
                &entry,
                AccessPolicy {
                    policy_id: Uuid::now_v7(),
                    inherit_space_role: false,
                    grants: vec![],
                },
            )
            .await?;
        assert!(authorizer
            .require("demo", viewer, Action::Read, Some(&entry))
            .await
            .is_err());
        let asset = ResourceRef {
            kind: ResourceKind::Asset,
            id: "private-asset".to_string(),
            parent: Some(Box::new(entry)),
        };
        assert!(authorizer
            .require("demo", viewer, Action::Read, Some(&asset))
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn agent_never_inherits_sponsor_rights_and_stops_with_sponsor() -> Result<()> {
        let op = operator_from_uri("memory://agent-authorizer")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let sponsor = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), sponsor, "Sponsor")
            .await?;
        let other_owner = Uuid::now_v7();
        authorizer
            .add_human_member(
                "demo",
                sponsor,
                SpacePrincipal {
                    principal_id: other_owner,
                    kind: PrincipalKind::Human,
                    display_name: "Other owner".to_string(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Owner,
            )
            .await?;
        let request = CreateAgentRequest {
            display_name: "Reader".to_string(),
            description: "Read-only automation".to_string(),
            mode: AgentMode::Both,
            owner_principal_ids: [sponsor].into_iter().collect(),
            granted_actions: [Action::Read].into_iter().collect(),
            expires_at: Some((Utc::now() + chrono::Duration::hours(1)).to_rfc3339()),
        };
        let agent = authorizer
            .create_agent("demo", sponsor, request.clone())
            .await?;
        assert_eq!(
            authorizer
                .find_agent_for_request("demo", sponsor, &request)
                .await?
                .map(|candidate| candidate.agent_id),
            Some(agent.agent_id)
        );
        let actions = authorizer
            .effective_actions("demo", agent.agent_id, None)
            .await?;
        assert_eq!(actions, [Action::Read].into_iter().collect());
        authorizer
            .revoke_agent("demo", sponsor, agent.agent_id)
            .await?;
        let revoked_state = authorizer.state("demo").await?;
        assert!(matches!(
            revoked_state
                .agents
                .get(&agent.agent_id)
                .map(|agent| &agent.status),
            Some(PrincipalState::Revoked)
        ));
        assert!(!revoked_state.agent_grants.contains_key(&agent.agent_id));
        assert!(authorizer
            .effective_actions("demo", agent.agent_id, None)
            .await
            .is_err());
        authorizer
            .revoke_principal("demo", other_owner, sponsor)
            .await?;
        assert!(authorizer
            .effective_actions("demo", agent.agent_id, None)
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn agent_is_revoked_when_an_owner_is_demoted() -> Result<()> {
        let op = operator_from_uri("memory://agent-owner-demotion").map_err(anyhow::Error::from)?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let sponsor = Uuid::now_v7();
        let second_owner = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), sponsor, "Sponsor")
            .await?;
        authorizer
            .add_human_member(
                "demo",
                sponsor,
                SpacePrincipal {
                    principal_id: second_owner,
                    kind: PrincipalKind::Human,
                    display_name: "Second owner".to_string(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Owner,
            )
            .await?;
        let agent = authorizer
            .create_agent(
                "demo",
                sponsor,
                CreateAgentRequest {
                    display_name: "Reader".to_string(),
                    description: String::new(),
                    mode: AgentMode::Autonomous,
                    owner_principal_ids: [sponsor].into_iter().collect(),
                    granted_actions: [Action::Read].into_iter().collect(),
                    expires_at: Some((Utc::now() + chrono::Duration::hours(1)).to_rfc3339()),
                },
            )
            .await?;

        authorizer
            .change_role("demo", second_owner, sponsor, SpaceRole::Viewer)
            .await?;
        let state = authorizer.state("demo").await?;
        assert!(matches!(
            state.agents.get(&agent.agent_id).map(|agent| &agent.status),
            Some(PrincipalState::Revoked)
        ));
        assert!(!state.agent_grants.contains_key(&agent.agent_id));
        assert!(authorizer
            .revoke_principal("demo", second_owner, agent.agent_id)
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn concurrent_revocation_and_policy_update_cannot_restore_principal() -> Result<()> {
        let op = operator_from_uri("memory://authorization-cas")?;
        op.create_dir("spaces/demo/").await?;
        let first = Authorizer::new(op.clone());
        let second = Authorizer::new(op);
        let owner = Uuid::now_v7();
        first
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        let member = Uuid::now_v7();
        first
            .add_human_member(
                "demo",
                owner,
                SpacePrincipal {
                    principal_id: member,
                    kind: PrincipalKind::Human,
                    display_name: "Member".to_string(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry".to_string(),
            parent: None,
        };
        let revoke = first.revoke_principal("demo", owner, member);
        let update = second.set_policy(
            "demo",
            owner,
            &resource,
            AccessPolicy {
                policy_id: Uuid::now_v7(),
                inherit_space_role: true,
                grants: vec![],
            },
        );
        let (revoke_result, update_result) = tokio::join!(revoke, update);
        revoke_result?;
        update_result?;
        let state = first.state("demo").await?;
        assert!(matches!(
            state
                .principals
                .get(&member)
                .map(|principal| &principal.state),
            Some(PrincipalState::Revoked)
        ));
        assert!(state.policies.contains_key(&resource.key()));
        Ok(())
    }

    #[tokio::test]
    async fn recovery_fence_serializes_membership_lifecycle_changes() -> Result<()> {
        let op = operator_from_uri("memory://authorization-recovery-fence")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op.clone());
        let owner = Uuid::now_v7();
        let member = Uuid::now_v7();
        let issuer_account = Uuid::now_v7();
        let target_account = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", space_uid, owner, "Owner")
            .await?;
        authorizer
            .add_human_member(
                "demo",
                owner,
                SpacePrincipal {
                    principal_id: member,
                    kind: PrincipalKind::Human,
                    display_name: "Member".to_string(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        let fence = authorizer
            .reserve_recovery_fence(
                "demo",
                Uuid::now_v7(),
                owner,
                issuer_account,
                member,
                target_account,
                0,
                0,
                chrono::Duration::minutes(5),
            )
            .await?;
        assert!(authorizer
            .change_role("demo", owner, member, SpaceRole::Editor)
            .await
            .is_err());
        let mut tampered = authorizer.state("demo").await?;
        tampered.revision += 1;
        op.write(&state_path("demo"), serde_json::to_vec_pretty(&tampered)?)
            .await?;
        assert!(authorizer
            .complete_recovery_fence("demo", fence.fence_id)
            .await
            .is_err());
        let error = authorizer
            .reserve_recovery_fence(
                "demo",
                Uuid::now_v7(),
                owner,
                issuer_account,
                member,
                target_account,
                0,
                0,
                chrono::Duration::minutes(5),
            )
            .await
            .expect_err("an active recovery fence must not be superseded");
        assert!(error.to_string().contains("RECOVERY_FENCE_UNAVAILABLE"));
        authorizer
            .release_recovery_fence("demo", fence.fence_id)
            .await?;
        authorizer
            .change_role("demo", owner, member, SpaceRole::Editor)
            .await?;
        let state = authorizer.state("demo").await?;
        assert_eq!(state.principal_lifecycle_epochs[&member], 2);
        assert_eq!(state.recovery_fences[&fence.fence_id].status, "released");
        Ok(())
    }

    #[tokio::test]
    async fn expired_recovery_fence_blocks_until_explicit_abort() -> Result<()> {
        let op = operator_from_uri("memory://authorization-expired-recovery-fence")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let member = Uuid::now_v7();
        let issuer_account = Uuid::now_v7();
        let target_account = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", space_uid, owner, "Owner")
            .await?;
        authorizer
            .add_human_member(
                "demo",
                owner,
                SpacePrincipal {
                    principal_id: member,
                    kind: PrincipalKind::Human,
                    display_name: "Member".to_string(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Viewer,
            )
            .await?;
        let fence = authorizer
            .reserve_recovery_fence(
                "demo",
                Uuid::now_v7(),
                owner,
                issuer_account,
                member,
                target_account,
                0,
                0,
                chrono::Duration::seconds(-1),
            )
            .await?;
        assert!(authorizer
            .change_role("demo", owner, member, SpaceRole::Editor)
            .await
            .is_err());
        authorizer
            .release_recovery_fence("demo", fence.fence_id)
            .await?;
        authorizer
            .change_role("demo", owner, member, SpaceRole::Editor)
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn recovery_fence_retry_reuses_exact_space_identity() -> Result<()> {
        let op = operator_from_uri("memory://authorization-recovery-fence-retry")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let member = Uuid::now_v7();
        let issuer_account = Uuid::now_v7();
        let target_account = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let request_id = Uuid::now_v7();
        let fence_id = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", space_uid, owner, "Owner")
            .await?;
        authorizer
            .add_human_member(
                "demo",
                owner,
                SpacePrincipal {
                    principal_id: member,
                    kind: PrincipalKind::Human,
                    display_name: "Member".to_string(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Viewer,
            )
            .await?;

        let first = authorizer
            .reserve_recovery_fence_with_id(
                "demo",
                request_id,
                fence_id,
                owner,
                issuer_account,
                member,
                target_account,
                0,
                0,
                chrono::Duration::minutes(5),
            )
            .await?;
        let retry = authorizer
            .reserve_recovery_fence_with_id(
                "demo",
                request_id,
                fence_id,
                owner,
                issuer_account,
                member,
                target_account,
                0,
                0,
                chrono::Duration::minutes(5),
            )
            .await?;
        assert_eq!(retry.fence_id, first.fence_id);
        assert_eq!(retry.request_id, first.request_id);
        assert_eq!(retry.expires_at, first.expires_at);

        let mismatch = authorizer
            .reserve_recovery_fence_with_id(
                "demo",
                request_id,
                fence_id,
                owner,
                issuer_account,
                member,
                Uuid::now_v7(),
                0,
                0,
                chrono::Duration::minutes(5),
            )
            .await
            .expect_err("a fence identity cannot be borrowed by another tuple");
        assert!(mismatch.to_string().contains("RECOVERY_FENCE_UNAVAILABLE"));
        Ok(())
    }

    #[tokio::test]
    async fn legacy_authorization_layout_is_rejected_before_owner_creation() -> Result<()> {
        let op = operator_from_uri("memory://legacy-authorization-layout")?;
        op.create_dir("spaces/demo/").await?;
        op.write("spaces/demo/authorization.json", "{}").await?;
        let authorizer = Authorizer::new(op.clone());

        let error = authorizer
            .ensure_owner("demo", Uuid::now_v7(), "Owner")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unsupported Space layout"));
        assert!(!op.exists("spaces/demo/security/principals.json").await?);
        Ok(())
    }

    #[tokio::test]
    async fn current_authorization_recovery_rejects_invalid_schema_and_owner_identity() -> Result<()>
    {
        let op = operator_from_uri("memory://invalid-current-authorization")?;
        op.create_dir("spaces/demo/").await?;
        let owner = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let authorizer = Authorizer::new(op.clone());
        authorizer
            .initialize_owner("demo", space_uid, owner, "Owner")
            .await?;

        let path = "spaces/demo/security/principals.json";
        let mut invalid_schema = authorizer.state("demo").await?;
        invalid_schema.schema_version = 99;
        op.write(path, serde_json::to_vec(&invalid_schema)?).await?;
        let error = authorizer
            .ensure_owner("demo", space_uid, "Owner")
            .await
            .expect_err("unsupported authorization schema must fail closed");
        assert!(error
            .to_string()
            .contains("unsupported Space authorization schema"));

        let mut invalid_owner = invalid_schema;
        invalid_owner.schema_version = CURRENT_AUTHORIZATION_SCHEMA_VERSION;
        invalid_owner
            .memberships
            .get_mut(&owner)
            .expect("owner membership")
            .principal_id = Uuid::now_v7();
        op.write(path, serde_json::to_vec(&invalid_owner)?).await?;
        let error = authorizer
            .ensure_owner("demo", space_uid, "Owner")
            .await
            .expect_err("owner membership identity mismatch must fail closed");
        assert!(error
            .to_string()
            .contains("membership map key does not match principal_id"));
        Ok(())
    }

    #[tokio::test]
    async fn legacy_membership_settings_are_rejected_before_owner_creation() -> Result<()> {
        let op = operator_from_uri("memory://legacy-membership-settings")?;
        op.create_dir("spaces/demo/").await?;
        op.write(
            "spaces/demo/settings.json",
            serde_json::to_vec(&serde_json::json!({"members": {"old-user": "owner"}}))?,
        )
        .await?;
        let authorizer = Authorizer::new(op.clone());

        let error = authorizer
            .ensure_owner("demo", Uuid::now_v7(), "Owner")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unsupported Space layout"));
        assert!(!op.exists("spaces/demo/security/principals.json").await?);
        Ok(())
    }

    #[tokio::test]
    async fn interrupted_migration_layout_is_rejected_before_owner_creation() -> Result<()> {
        let op = operator_from_uri("memory://interrupted-migration-layout")?;
        op.create_dir("spaces/demo/").await?;
        op.write("spaces/demo/security/migration-state.json", "{}")
            .await?;
        let authorizer = Authorizer::new(op.clone());

        let error = authorizer
            .ensure_owner("demo", Uuid::now_v7(), "Owner")
            .await
            .unwrap_err();

        assert!(error.to_string().contains("unsupported Space layout"));
        assert!(!op.exists("spaces/demo/security/principals.json").await?);
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_is_bound_and_single_use() -> Result<()> {
        let op = operator_from_uri("memory://human-approval")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let issuer_account = Uuid::now_v7();
        let credential = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry-1".into(),
            parent: None,
        };
        let (approval, token) = authorizer
            .issue_human_approval(
                "demo",
                HumanApprovalIssue {
                    operation: "entry.delete".into(),
                    action: Action::Delete,
                    resource: resource.clone(),
                    intent_hash: "a".repeat(64),
                    actor_principal_id: owner,
                    actor_credential_id: credential,
                    issuer_principal_id: owner,
                    issuer_account_id: issuer_account,
                    issuer_credential_id: credential,
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        assert_eq!(token.len(), 43);
        assert_eq!(authorizer.state("demo").await?.human_approvals.len(), 1);
        let mismatch = authorizer
            .consume_human_approval(
                "demo",
                &token,
                "entry.delete",
                Action::Delete,
                &resource,
                &"b".repeat(64),
                owner,
                credential,
            )
            .await
            .expect_err("an execution-time intent mismatch must not consume the approval");
        assert_eq!(mismatch.to_string(), "HUMAN_APPROVAL_INVALID");
        assert!(authorizer
            .state("demo")
            .await?
            .human_approvals
            .values()
            .all(|approval| approval.consumed_at.is_none()));
        let consumed = authorizer
            .consume_human_approval(
                "demo",
                &token,
                "entry.delete",
                Action::Delete,
                &resource,
                &"a".repeat(64),
                owner,
                credential,
            )
            .await?;
        assert_eq!(consumed.approval_id, approval.approval_id);
        let replay = authorizer
            .consume_human_approval(
                "demo",
                &token,
                "entry.delete",
                Action::Delete,
                &resource,
                &"a".repeat(64),
                owner,
                credential,
            )
            .await
            .expect_err("the same token cannot be consumed twice");
        assert_eq!(replay.to_string(), "HUMAN_APPROVAL_REPLAYED");
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_is_invalidated_by_principal_lifecycle_change() -> Result<()> {
        let op = operator_from_uri("memory://human-approval-lifecycle")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let member = Uuid::now_v7();
        let credential = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        authorizer
            .add_human_member(
                "demo",
                owner,
                SpacePrincipal {
                    principal_id: member,
                    kind: PrincipalKind::Human,
                    display_name: "Member".into(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Editor,
            )
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry-1".into(),
            parent: None,
        };
        authorizer
            .set_policy(
                "demo",
                owner,
                &resource,
                AccessPolicy {
                    policy_id: Uuid::now_v7(),
                    inherit_space_role: false,
                    grants: vec![Grant {
                        principal_id: member,
                        actions: [Action::Delete].into_iter().collect(),
                    }],
                },
            )
            .await?;
        let (_, token) = authorizer
            .issue_human_approval(
                "demo",
                HumanApprovalIssue {
                    operation: "entry.delete".into(),
                    action: Action::Delete,
                    resource: resource.clone(),
                    intent_hash: "a".repeat(64),
                    actor_principal_id: member,
                    actor_credential_id: credential,
                    issuer_principal_id: owner,
                    issuer_account_id: Uuid::now_v7(),
                    issuer_credential_id: Uuid::now_v7(),
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        authorizer.revoke_principal("demo", owner, member).await?;
        let error = authorizer
            .consume_human_approval(
                "demo",
                &token,
                "entry.delete",
                Action::Delete,
                &resource,
                &"a".repeat(64),
                member,
                credential,
            )
            .await
            .expect_err("principal revocation must invalidate the approval");
        assert_eq!(error.to_string(), "HUMAN_APPROVAL_INVALID");
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_rechecks_current_acl_under_the_consume_lock() -> Result<()> {
        let op = operator_from_uri("memory://human-approval-acl-race")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let member = Uuid::now_v7();
        let credential = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        authorizer
            .add_human_member(
                "demo",
                owner,
                SpacePrincipal {
                    principal_id: member,
                    kind: PrincipalKind::Human,
                    display_name: "Member".into(),
                    state: PrincipalState::Active,
                    created_at: now_iso(),
                },
                SpaceRole::Editor,
            )
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry-1".into(),
            parent: None,
        };
        authorizer
            .set_policy(
                "demo",
                owner,
                &resource,
                AccessPolicy {
                    policy_id: Uuid::now_v7(),
                    inherit_space_role: false,
                    grants: vec![Grant {
                        principal_id: member,
                        actions: [Action::Delete].into_iter().collect(),
                    }],
                },
            )
            .await?;
        let (_, token) = authorizer
            .issue_human_approval(
                "demo",
                HumanApprovalIssue {
                    operation: "entry.delete".into(),
                    action: Action::Delete,
                    resource: resource.clone(),
                    intent_hash: "a".repeat(64),
                    actor_principal_id: member,
                    actor_credential_id: credential,
                    issuer_principal_id: member,
                    issuer_account_id: Uuid::now_v7(),
                    issuer_credential_id: Uuid::now_v7(),
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        authorizer
            .set_policy(
                "demo",
                owner,
                &resource,
                AccessPolicy {
                    policy_id: Uuid::now_v7(),
                    inherit_space_role: false,
                    grants: vec![],
                },
            )
            .await?;
        let error = authorizer
            .consume_human_approval(
                "demo",
                &token,
                "entry.delete",
                Action::Delete,
                &resource,
                &"a".repeat(64),
                member,
                credential,
            )
            .await
            .expect_err("revoked ACL permission must invalidate the approval");
        assert_eq!(error.to_string(), "HUMAN_APPROVAL_INVALID");
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_concurrent_replay_has_one_winner() -> Result<()> {
        let op = operator_from_uri("memory://human-approval-concurrent")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let credential = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry-1".into(),
            parent: None,
        };
        let (_, token) = authorizer
            .issue_human_approval(
                "demo",
                HumanApprovalIssue {
                    operation: "entry.delete".into(),
                    action: Action::Delete,
                    resource: resource.clone(),
                    intent_hash: "b".repeat(64),
                    actor_principal_id: owner,
                    actor_credential_id: credential,
                    issuer_principal_id: owner,
                    issuer_account_id: Uuid::now_v7(),
                    issuer_credential_id: credential,
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        let left = authorizer.clone();
        let right = authorizer.clone();
        let left_token = token.clone();
        let right_token = token;
        let left_resource = resource.clone();
        let right_resource = resource;
        let (left, right) = tokio::join!(
            async move {
                left.consume_human_approval(
                    "demo",
                    &left_token,
                    "entry.delete",
                    Action::Delete,
                    &left_resource,
                    &"b".repeat(64),
                    owner,
                    credential,
                )
                .await
            },
            async move {
                right
                    .consume_human_approval(
                        "demo",
                        &right_token,
                        "entry.delete",
                        Action::Delete,
                        &right_resource,
                        &"b".repeat(64),
                        owner,
                        credential,
                    )
                    .await
            }
        );
        assert!(left.is_ok() ^ right.is_ok());
        let replay = if let Err(error) = left {
            error
        } else {
            right.unwrap_err()
        };
        assert_eq!(replay.to_string(), "HUMAN_APPROVAL_REPLAYED");
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_mutation_callback_holds_the_authorization_lock() -> Result<()> {
        let op = operator_from_uri("memory://human-approval-mutation-lock")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let credential = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry-1".to_string(),
            parent: None,
        };
        let (_, token) = authorizer
            .issue_human_approval(
                "demo",
                HumanApprovalIssue {
                    operation: "entry.delete".to_string(),
                    action: Action::Delete,
                    resource: resource.clone(),
                    intent_hash: "c".repeat(64),
                    actor_principal_id: owner,
                    actor_credential_id: credential,
                    issuer_principal_id: owner,
                    issuer_account_id: Uuid::now_v7(),
                    issuer_credential_id: credential,
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let consuming_authorizer = authorizer.clone();
        let consuming_token = token.clone();
        let consuming_resource = resource.clone();
        let consuming = tokio::spawn(async move {
            consuming_authorizer
                .consume_human_approval_with_audit_and(
                    "demo",
                    &consuming_token,
                    "entry.delete",
                    Action::Delete,
                    &consuming_resource,
                    &"c".repeat(64),
                    owner,
                    credential,
                    |_, _, _, _| Vec::new(),
                    || async move {
                        let _ = started_tx.send(());
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        Ok::<(), anyhow::Error>(())
                    },
                )
                .await
        });
        started_rx.await?;
        let updating_authorizer = authorizer.clone();
        let updating = tokio::spawn(async move {
            updating_authorizer
                .set_policy(
                    "demo",
                    owner,
                    &resource,
                    AccessPolicy {
                        policy_id: Uuid::now_v7(),
                        inherit_space_role: true,
                        grants: vec![],
                    },
                )
                .await
        });
        tokio::task::yield_now().await;
        assert!(!updating.is_finished());
        let (_, mutation) = consuming.await??;
        mutation?;
        updating.await??;
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_ambiguous_commit_fails_closed_before_mutation() -> Result<()> {
        let op = operator_from_uri("memory://human-approval-ambiguous-commit")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let credential = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry-ambiguous".to_string(),
            parent: None,
        };
        let (_, token) = authorizer
            .issue_human_approval(
                "demo",
                HumanApprovalIssue {
                    operation: "entry.delete".to_string(),
                    action: Action::Delete,
                    resource: resource.clone(),
                    intent_hash: "d".repeat(64),
                    actor_principal_id: owner,
                    actor_credential_id: credential,
                    issuer_principal_id: owner,
                    issuer_account_id: Uuid::now_v7(),
                    issuer_credential_id: credential,
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        authorizer.inject_ambiguous_write_once();
        let called = Arc::new(AtomicBool::new(false));
        let callback_called = called.clone();
        let error = authorizer
            .consume_human_approval_with_audit_and(
                "demo",
                &token,
                "entry.delete",
                Action::Delete,
                &resource,
                &"d".repeat(64),
                owner,
                credential,
                |_, _, _, _| Vec::new(),
                || async move {
                    callback_called.store(true, Ordering::SeqCst);
                    Ok::<(), anyhow::Error>(())
                },
            )
            .await
            .expect_err("an ambiguous approval-state CAS must fail closed");
        assert_eq!(error.to_string(), "HUMAN_APPROVAL_OUTCOME_UNKNOWN");
        assert!(!called.load(Ordering::SeqCst));
        assert!(authorizer
            .state("demo")
            .await?
            .human_approvals
            .values()
            .all(|approval| approval.consumed_at.is_some()));
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_post_commit_writer_stays_unknown_before_mutation() -> Result<()> {
        let op = operator_from_uri("memory://human-approval-post-commit-writer")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        let owner = Uuid::now_v7();
        let credential = Uuid::now_v7();
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), owner, "Owner")
            .await?;
        let resource = ResourceRef {
            kind: ResourceKind::Entry,
            id: "entry-post-commit-writer".to_string(),
            parent: None,
        };
        let (_, token) = authorizer
            .issue_human_approval(
                "demo",
                HumanApprovalIssue {
                    operation: "entry.delete".to_string(),
                    action: Action::Delete,
                    resource: resource.clone(),
                    intent_hash: "e".repeat(64),
                    actor_principal_id: owner,
                    actor_credential_id: credential,
                    issuer_principal_id: owner,
                    issuer_account_id: Uuid::now_v7(),
                    issuer_credential_id: credential,
                    issuer_credential_generation: 0,
                    issuer_node_account_lifecycle_epoch: 0,
                    ttl: chrono::Duration::seconds(30),
                },
            )
            .await?;
        authorizer.inject_ambiguous_write_with_post_commit_writer_once();
        let called = Arc::new(AtomicBool::new(false));
        let callback_called = called.clone();
        let error = authorizer
            .consume_human_approval_with_audit_and(
                "demo",
                &token,
                "entry.delete",
                Action::Delete,
                &resource,
                &"e".repeat(64),
                owner,
                credential,
                |_, _, _, _| Vec::new(),
                || async move {
                    callback_called.store(true, Ordering::SeqCst);
                    Ok::<(), anyhow::Error>(())
                },
            )
            .await
            .expect_err("a post-commit writer must keep the mutation outcome unknown");
        assert_eq!(error.to_string(), "HUMAN_APPROVAL_OUTCOME_UNKNOWN");
        assert!(!called.load(Ordering::SeqCst));
        assert!(authorizer
            .state("demo")
            .await?
            .human_approvals
            .values()
            .all(|approval| approval.consumed_at.is_some()));
        Ok(())
    }

    #[tokio::test]
    async fn human_approval_audit_retry_preserves_causal_sequence() -> Result<()> {
        let op = operator_from_uri("memory://human-approval-audit-order")?;
        op.create_dir("spaces/demo/").await?;
        let authorizer = Authorizer::new(op);
        authorizer
            .initialize_owner("demo", Uuid::now_v7(), Uuid::now_v7(), "Owner")
            .await?;
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        authorizer
            .queue_human_approval_audit("demo", first, serde_json::json!({"phase": "issued"}))
            .await?;
        authorizer
            .queue_human_approval_audit("demo", second, serde_json::json!({"phase": "consumed"}))
            .await?;
        authorizer
            .queue_human_approval_audit("demo", first, serde_json::json!({"phase": "issued"}))
            .await?;
        let pending = authorizer.pending_human_approval_audits("demo").await?;
        assert_eq!(
            pending
                .iter()
                .map(|record| record.event_id)
                .collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_eq!(pending[0].sequence, 1);
        assert_eq!(pending[1].sequence, 2);
        Ok(())
    }
}
