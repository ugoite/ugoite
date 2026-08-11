//! Space-portable authorization state and the shared authorizer used by adapters.

use crate::audit;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{SecondsFormat, Utc};
use fs2::FileExt;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    path::Path,
    sync::{Arc, OnceLock},
};
use tokio::sync::Mutex;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::identity::{
    evaluate_policy, role_actions, AccessPolicy, Action, AgentMode, AgentPrincipal, Membership,
    PrincipalKind, PrincipalState, SpacePrincipal, SpaceRole,
};
use uuid::Uuid;

const AUTHORIZATION_FILE: &str = "security/principals.json";
const LEGACY_AUTHORIZATION_FILE: &str = "authorization.json";
const LEGACY_MIGRATION_STATE_FILE: &str = "security/migration-state.json";

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
    /// Reserved monotonic revision for future synchronization protocols.
    pub revision: u64,
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
}

fn authorization_write_lock() -> Arc<Mutex<()>> {
    static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(Mutex::new(()))).clone()
}

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
        }
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
        state
            .memberships
            .values()
            .find(|membership| matches!(membership.role, SpaceRole::Owner))
            .map(|membership| Some(membership.principal_id))
            .ok_or_else(|| anyhow!("Space has no owner principal"))
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
        let bytes = self
            .operator
            .read(&state_path(space_id))
            .await
            .context("read Space authorization state")?;
        serde_json::from_slice(&bytes.to_vec()).context("decode Space authorization state")
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
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        if state.space_uid == Uuid::nil() {
            bail!("recovery fence is unavailable")
        }
        for fence in state.recovery_fences.values_mut().filter(|fence| {
            fence.status == "active"
                && chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
                    .is_ok_and(|expires| expires.with_timezone(&Utc) > Utc::now())
        }) {
            if fence.target_principal_id == target_principal_id
                && fence.target_account_id == target_account_id
            {
                fence.status = "superseded".to_string();
            } else {
                bail!("RECOVERY_FENCE_UNAVAILABLE")
            }
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
            fence_id: Uuid::now_v7(),
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
        self.write_state(space_id, &state).await?;
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
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        let fence = state
            .recovery_fences
            .get_mut(&fence_id)
            .ok_or_else(|| anyhow!("recovery fence is unavailable"))?;
        if fence.status != "active" {
            bail!("recovery fence is not active")
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
        fence.status = "completed".to_string();
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow!("authorization revision overflow"))?;
        self.write_state(space_id, &state).await
    }

    pub async fn release_recovery_fence(&self, space_id: &str, fence_id: Uuid) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        if let Some(fence) = state.recovery_fences.get_mut(&fence_id) {
            if fence.status == "active" {
                fence.status = "released".to_string();
                state.revision = state
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("authorization revision overflow"))?;
                self.write_state(space_id, &state).await?;
            }
        }
        Ok(())
    }

    fn ensure_recovery_mutation_allowed(&self, state: &AuthorizationState) -> Result<()> {
        if state.recovery_fences.values().any(|fence| {
            fence.status == "active"
                && chrono::DateTime::parse_from_rfc3339(&fence.expires_at)
                    .is_ok_and(|expires| expires.with_timezone(&Utc) > Utc::now())
        }) {
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
        self.require(space_id, actor, Action::Share, Some(resource))
            .await?;
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&state)?;
        for grant in &policy.grants {
            let Some(principal) = state.principals.get(&grant.principal_id) else {
                bail!("policy references a principal outside the space");
            };
            if matches!(principal.kind, PrincipalKind::Agent)
                && (grant.actions.contains(&Action::Delete)
                    || grant.actions.contains(&Action::Share))
            {
                bail!("delete and share require a human approval object and cannot be granted to agents");
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
        self.write_state(space_id, &state).await?;
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
        Ok(())
    }

    pub async fn add_human_member(
        &self,
        space_id: &str,
        actor: Uuid,
        principal: SpacePrincipal,
        role: SpaceRole,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        if !matches!(principal.kind, PrincipalKind::Human) {
            bail!("human member must use a human principal");
        }
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&state)?;
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
        self.write_state(space_id, &state).await?;
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
        let CreateAgentRequest {
            display_name,
            description,
            mode,
            owner_principal_ids,
            granted_actions,
            expires_at,
        } = request;
        self.require(space_id, actor, Action::Share, None).await?;
        if granted_actions.contains(&Action::Delete) || granted_actions.contains(&Action::Share) {
            bail!("agents cannot receive delete or share actions");
        }
        if expires_at.is_none() {
            bail!("agent expiry or review deadline is required");
        }
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&state)?;
        if owner_principal_ids.is_empty() || !owner_principal_ids.contains(&actor) {
            bail!("agent sponsor must be one of at least one human owner");
        }
        if owner_principal_ids.iter().any(|id| {
            !state.principals.get(id).is_some_and(|p| {
                matches!(p.kind, PrincipalKind::Human) && matches!(p.state, PrincipalState::Active)
            })
        }) {
            bail!("all agent owners must be active human principals in the Space");
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
        state.agent_grants.insert(agent_id, granted_actions);
        state.agents.insert(agent_id, agent.clone());
        state.revision += 1;
        self.write_state(space_id, &state).await?;
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

    pub async fn revoke_agent(&self, space_id: &str, actor: Uuid, agent_id: Uuid) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&state)?;
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
        state.agent_grants.remove(&agent_id);
        state.revision += 1;
        self.write_state(space_id, &state).await?;
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
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&state)?;
        let agent = state
            .agents
            .get_mut(&agent_id)
            .filter(|agent| matches!(agent.status, PrincipalState::Active))
            .ok_or_else(|| anyhow!("agent is not active"))?;
        agent.last_used_at = Some(now_iso());
        state.revision += 1;
        self.write_state(space_id, &state).await
    }

    pub async fn change_role(
        &self,
        space_id: &str,
        actor: Uuid,
        principal_id: Uuid,
        role: SpaceRole,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&state)?;
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
        state
            .memberships
            .get_mut(&principal_id)
            .expect("checked membership")
            .role = role;
        *state
            .principal_lifecycle_epochs
            .entry(principal_id)
            .or_insert(0) += 1;
        state.revision += 1;
        self.write_state(space_id, &state).await?;
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
        Ok(())
    }

    pub async fn revoke_principal(
        &self,
        space_id: &str,
        actor: Uuid,
        principal_id: Uuid,
    ) -> Result<()> {
        self.require(space_id, actor, Action::Share, None).await?;
        let _guard = self.lock.lock().await;
        let mut state = self.state(space_id).await?;
        self.ensure_recovery_mutation_allowed(&state)?;
        if state
            .memberships
            .get(&principal_id)
            .is_some_and(|m| matches!(m.role, SpaceRole::Owner))
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
        self.write_state(space_id, &state).await?;
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
        let path = state_path(space_id);
        let serialized = serde_json::to_vec_pretty(state)?;
        let capabilities = self.operator.info().full_capability();
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
            let current = self
                .operator
                .read_with(&path)
                .if_match(&version)
                .await
                .context("read versioned Space authorization state")?;
            let current: AuthorizationState = serde_json::from_slice(&current.to_vec())
                .context("decode versioned Space authorization state")?;
            if current.revision != expected_revision {
                bail!("Space authorization revision conflict");
            }
            self.operator
                .write_with(&path, serialized)
                .if_match(&version)
                .await
                .context("compare-and-swap Space authorization state")?;
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

/// Evaluates authorization against one already-read state snapshot. Query
/// session creation uses this to derive its scope and fingerprint from the
/// same state without an Entry-by-Entry OpenDAL reread.
pub(crate) fn effective_actions_for_state(
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
        if !state
            .principals
            .get(&agent.sponsor_principal_id)
            .is_some_and(|sponsor| {
                matches!(sponsor.kind, PrincipalKind::Human)
                    && matches!(sponsor.state, PrincipalState::Active)
            })
        {
            return Err(AppError::forbidden("agent sponsor is not active").into());
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
    if state
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

fn state_path(space_id: &str) -> String {
    format!("spaces/{space_id}/{AUTHORIZATION_FILE}")
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let agent = authorizer
            .create_agent(
                "demo",
                sponsor,
                CreateAgentRequest {
                    display_name: "Reader".to_string(),
                    description: "Read-only automation".to_string(),
                    mode: AgentMode::Both,
                    owner_principal_ids: [sponsor].into_iter().collect(),
                    granted_actions: [Action::Read].into_iter().collect(),
                    expires_at: Some((Utc::now() + chrono::Duration::hours(1)).to_rfc3339()),
                },
            )
            .await?;
        let actions = authorizer
            .effective_actions("demo", agent.agent_id, None)
            .await?;
        assert_eq!(actions, [Action::Read].into_iter().collect());
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
                chrono::Duration::minutes(5),
            )
            .await?;
        assert!(authorizer
            .change_role("demo", owner, member, SpaceRole::Editor)
            .await
            .is_err());
        let replacement = authorizer
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
        let superseded_state = authorizer.state("demo").await?;
        assert_eq!(
            superseded_state.recovery_fences[&fence.fence_id].status,
            "superseded"
        );
        authorizer
            .complete_recovery_fence("demo", replacement.fence_id)
            .await?;
        authorizer
            .change_role("demo", owner, member, SpaceRole::Editor)
            .await?;
        let state = authorizer.state("demo").await?;
        assert_eq!(state.principal_lifecycle_epochs[&member], 2);
        assert_eq!(
            state.recovery_fences[&replacement.fence_id].status,
            "completed"
        );
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
}
