//! Node-local identity state. Nothing in this module is stored inside a Space.

use crate::{
    control_store::{NodeControlStore, OpenDalNodeControlStore},
    oauth::AccessTokenClaims,
    secret_store::{EnvironmentSecretStore, NodeSecretStore},
};
use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, KeyInit as HmacKeyInit, Mac};
use opendal::Operator;
use rand::TryRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::Mutex;
use ugoite_domain::identity::{
    AccountStatus, AssuranceLevel, AuthenticationMethod, AuthenticationMethodKind, BindingMethod,
    HumanAccount, NodeRole, PrincipalBinding,
};
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PasskeyAuthentication, PasskeyRegistration,
    PublicKeyCredential, RegisterPublicKeyCredential, RequestChallengeResponse, Url, Webauthn,
    WebauthnBuilder,
};

const NODE_POINTER_KEY: &str = "node.json";

fn node_state_key(node_id: Uuid) -> String {
    format!("nodes/{node_id}/state.json")
}
const SETUP_LIFETIME_MINUTES: i64 = 30;
const CHALLENGE_LIFETIME_MINUTES: i64 = 5;
const SESSION_IDLE_HOURS: i64 = 24;
const SESSION_ABSOLUTE_DAYS: i64 = 30;
const INVITATION_LIFETIME_HOURS: i64 = 72;
const TOTP_ENROLLMENT_LIFETIME_MINUTES: i64 = 10;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeLifecycle {
    Uninitialized,
    Active,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OneTimeSecret {
    pub token_hash: String,
    pub expires_at: String,
    pub used_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredPasskey {
    pub credential_id: String,
    pub account_id: Uuid,
    #[serde(default = "Uuid::nil")]
    pub method_id: Uuid,
    pub passkey: Passkey,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub rp_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RegistrationChallenge {
    account_id: Uuid,
    #[serde(default)]
    credential_generation: u64,
    display_name: String,
    state: PasskeyRegistration,
    /// Keep the browser-facing challenge beside the server-side WebAuthn
    /// state so a retry can replay the same start response after an
    /// ambiguous Node state write.
    #[serde(default)]
    public_key: Option<CreationChallengeResponse>,
    purpose: RegistrationPurpose,
    expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RegistrationPurpose {
    Setup,
    Invitation {
        invitation_id: Uuid,
    },
    AddCredential,
    Recovery,
    OwnerRecovery {
        approval_id: Uuid,
        reset_id: Uuid,
        space_uid: Uuid,
        principal_id: Uuid,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AuthenticationChallenge {
    state: PasskeyAuthentication,
    expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BrowserSession {
    #[serde(default = "Uuid::nil")]
    pub session_id: Uuid,
    pub session_hash: String,
    #[serde(default = "Uuid::nil")]
    pub credential_id: Uuid,
    #[serde(default = "default_session_assurance")]
    pub assurance: AssuranceLevel,
    pub account_id: Uuid,
    #[serde(default)]
    pub credential_generation: u64,
    pub created_at: String,
    pub last_seen_at: String,
    pub expires_at: String,
    pub authenticated_at: String,
    pub revoked_at: Option<String>,
    /// Sessions created by owner recovery are accepted only after the
    /// matching reset marker is durably committed.
    #[serde(default)]
    pub recovery_reset_id: Option<Uuid>,
    #[serde(default)]
    pub revocation_epoch: u64,
}

fn default_session_assurance() -> AssuranceLevel {
    AssuranceLevel::PhishingResistant
}

#[derive(Clone, Debug)]
pub struct AuthenticatedSession {
    pub account: HumanAccount,
    pub session_id: Uuid,
    pub credential_id: Uuid,
    pub assurance: AssuranceLevel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeAuditEvent {
    pub event_id: Uuid,
    pub timestamp: String,
    pub node_id: Uuid,
    pub subject_account_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub credential_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Option<String>,
    pub outcome: String,
    pub request_id: Option<String>,
    pub safe_metadata: serde_json::Value,
    #[serde(default)]
    pub canonical_event: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
pub struct NodeAuditInput<'a> {
    pub subject_account_id: Option<Uuid>,
    pub actor_account_id: Option<Uuid>,
    pub credential_id: Option<Uuid>,
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: Option<String>,
    pub outcome: &'a str,
    pub request_id: Option<String>,
    pub safe_metadata: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AccountInvitation {
    pub invitation_id: Uuid,
    pub token_hash: String,
    pub display_name: String,
    pub space_uid: Option<Uuid>,
    pub role: Option<String>,
    pub expires_at: String,
    pub acceptance: Option<InvitationAcceptance>,
    pub created_by: Uuid,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvitationAcceptanceKind {
    ExistingAccount,
    PasskeyRegistration,
    Oidc,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum InvitationAcceptance {
    Pending {
        account_id: Uuid,
        principal_id: Uuid,
        kind: InvitationAcceptanceKind,
        claimed_at: String,
        #[serde(default)]
        credential_generation: u64,
    },
    Completed {
        account_id: Uuid,
        principal_id: Uuid,
        kind: InvitationAcceptanceKind,
        claimed_at: String,
        completed_at: String,
        #[serde(default)]
        credential_generation: u64,
    },
}

impl InvitationAcceptance {
    fn account_id(&self) -> Uuid {
        match self {
            Self::Pending { account_id, .. } | Self::Completed { account_id, .. } => *account_id,
        }
    }

    fn principal_id(&self) -> Uuid {
        match self {
            Self::Pending { principal_id, .. } | Self::Completed { principal_id, .. } => {
                *principal_id
            }
        }
    }

    fn kind(&self) -> &InvitationAcceptanceKind {
        match self {
            Self::Pending { kind, .. } | Self::Completed { kind, .. } => kind,
        }
    }

    fn credential_generation(&self) -> u64 {
        match self {
            Self::Pending {
                credential_generation,
                ..
            }
            | Self::Completed {
                credential_generation,
                ..
            } => *credential_generation,
        }
    }
}

impl AccountInvitation {
    pub fn accepted_principal_id(&self) -> Option<Uuid> {
        self.acceptance
            .as_ref()
            .map(InvitationAcceptance::principal_id)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecoveryRecord {
    pub account_id: Uuid,
    pub code_hashes: Vec<String>,
    pub totp_secret_encrypted: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub failed_attempts: u32,
    #[serde(default)]
    pub locked_until: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OwnerRecoveryApproval {
    pub approval_id: Uuid,
    pub token_hash: String,
    pub space_uid: Uuid,
    pub principal_id: Uuid,
    pub account_id: Uuid,
    pub issuer_principal_id: Uuid,
    pub issuer_account_id: Uuid,
    #[serde(default)]
    pub issuer_credential_id: Option<Uuid>,
    #[serde(default)]
    pub target_generation: u64,
    #[serde(default)]
    pub issuer_generation: u64,
    #[serde(default)]
    pub issuer_space_lifecycle_epoch: u64,
    #[serde(default)]
    pub target_space_lifecycle_epoch: u64,
    #[serde(default)]
    pub issuer_node_lifecycle_epoch: u64,
    #[serde(default)]
    pub target_node_lifecycle_epoch: u64,
    #[serde(default)]
    pub space_authorization_revision: u64,
    #[serde(default)]
    pub recovery_fence_id: Option<Uuid>,
    pub issued_at: String,
    pub expires_at: String,
    pub challenge_id: Option<Uuid>,
    pub reset_id: Option<Uuid>,
    pub used_at: Option<String>,
    #[serde(default)]
    pub invalidated_at: Option<String>,
    /// Encrypted bearer retained for replaying the same idempotent issuance
    /// after a response/write outcome was ambiguous. The plaintext is never
    /// included in Node audit state.
    #[serde(default)]
    pub encrypted_token: Option<String>,
}

#[derive(Clone, Debug)]
pub struct OwnerRecoveryContext {
    pub space_uid: Uuid,
    pub principal_id: Uuid,
    pub account_id: Uuid,
    pub issuer_principal_id: Uuid,
    pub issuer_account_id: Uuid,
    pub target_generation: u64,
    pub issuer_generation: u64,
    pub issuer_space_lifecycle_epoch: u64,
    pub target_space_lifecycle_epoch: u64,
    pub issuer_node_lifecycle_epoch: u64,
    pub target_node_lifecycle_epoch: u64,
    pub space_authorization_revision: u64,
    pub recovery_fence_id: Option<Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryBindingSnapshot {
    pub request_id: Uuid,
    pub recovery_fence_id: Uuid,
    pub recovery_fence_expires_at: String,
    pub space_authorization_revision: u64,
    pub issuer_space_lifecycle_epoch: u64,
    pub target_space_lifecycle_epoch: u64,
    pub issuer_node_lifecycle_epoch: u64,
    pub target_node_lifecycle_epoch: u64,
    pub issuer_generation: u64,
    pub target_generation: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct NodeRecoveryFence {
    fence_id: Uuid,
    #[serde(default = "default_node_recovery_fence_request_id")]
    request_id: Uuid,
    space_uid: Uuid,
    principal_id: Uuid,
    account_id: Uuid,
    issuer_account_id: Uuid,
    issuer_node_lifecycle_epoch: u64,
    target_node_lifecycle_epoch: u64,
    issuer_generation: u64,
    target_generation: u64,
    expires_at: String,
    status: String,
    #[serde(default = "default_node_recovery_fence_phase")]
    phase: String,
}

fn default_node_recovery_fence_phase() -> String {
    "paired".to_string()
}

fn default_node_recovery_fence_request_id() -> Uuid {
    Uuid::nil()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecoveryResetMarker {
    pub reset_id: Uuid,
    pub challenge_id: Uuid,
    pub approval_id: Uuid,
    pub account_id: Uuid,
    pub generation_before: u64,
    pub generation_after: u64,
    pub session_id: Uuid,
    #[serde(default)]
    pub space_authorization_revision: u64,
    #[serde(default = "Uuid::nil")]
    pub recovery_fence_id: Uuid,
    #[serde(default = "Uuid::nil")]
    pub space_uid: Uuid,
    #[serde(default = "Uuid::nil")]
    pub principal_id: Uuid,
    #[serde(default = "Uuid::nil")]
    pub issuer_principal_id: Uuid,
    #[serde(default = "default_space_fence_status")]
    pub space_fence_status: String,
    pub committed_at: String,
    /// Encrypted one-time response material retained until the client has
    /// received the replacement session and recovery codes.
    #[serde(default)]
    pub encrypted_response: Option<String>,
    #[serde(default)]
    pub response_delivered_at: Option<String>,
    /// Stable claim identity lets a caller prove that its delivery CAS won
    /// even when another process writes unrelated Node state before the
    /// verification read.
    #[serde(default)]
    pub response_delivery_id: Option<Uuid>,
    #[serde(default)]
    pub response_invalidated_at: Option<String>,
    #[serde(default)]
    pub completion_proof_hash: Option<String>,
}

fn default_space_fence_status() -> String {
    "node_committed_space_fence_pending".to_string()
}

fn invalidate_pending_recovery_responses(state: &mut NodeState, account_id: Uuid, now: &str) {
    for marker in state.recovery_reset_markers.values_mut().filter(|marker| {
        marker.account_id == account_id
            && marker.response_delivered_at.is_none()
            && marker.response_invalidated_at.is_none()
    }) {
        marker.response_invalidated_at = Some(now.to_string());
    }
    for record in state
        .backup_rotation_requests
        .values_mut()
        .filter(|record| {
            record.account_id == account_id
                && record.codes_delivered_at.is_none()
                && record.codes_invalidated_at.is_none()
        })
    {
        record.codes_invalidated_at = Some(now.to_string());
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecoveryChallengeTombstone {
    pub challenge_id: Uuid,
    pub approval_id: Uuid,
    pub reset_id: Uuid,
    pub reason: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BackupRotationRecord {
    pub request_id: Uuid,
    pub space_uid: Uuid,
    pub principal_id: Uuid,
    pub account_id: Uuid,
    pub issuer_principal_id: Uuid,
    pub issuer_account_id: Uuid,
    #[serde(default)]
    pub issuer_credential_id: Option<Uuid>,
    pub target_generation: u64,
    #[serde(default)]
    pub issuer_generation: u64,
    #[serde(default)]
    pub issuer_space_lifecycle_epoch: u64,
    #[serde(default)]
    pub target_space_lifecycle_epoch: u64,
    #[serde(default)]
    pub issuer_node_lifecycle_epoch: u64,
    #[serde(default)]
    pub target_node_lifecycle_epoch: u64,
    #[serde(default)]
    pub space_authorization_revision: u64,
    #[serde(default)]
    pub recovery_fence_id: Option<Uuid>,
    #[serde(default = "default_space_fence_status")]
    pub space_fence_status: String,
    pub issued_at: String,
    pub code_hashes: Vec<String>,
    /// The one-time response is encrypted with the Node key so a crash after
    /// the Node CAS can finish fence reconciliation before delivering it.
    #[serde(default)]
    pub encrypted_codes: Option<String>,
    #[serde(default)]
    pub codes_delivered_at: Option<String>,
    /// Stable claim identity distinguishes this delivery from a competing
    /// process after a subsequent Node-state write.
    #[serde(default)]
    pub codes_delivery_id: Option<Uuid>,
    #[serde(default)]
    pub codes_invalidated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RecoveryAuditOutboxRecord {
    pub event_id: Uuid,
    pub action: String,
    pub request_id: Uuid,
    pub space_uid: Uuid,
    pub principal_id: Uuid,
    pub account_id: Uuid,
    pub issuer_principal_id: Option<Uuid>,
    #[serde(default)]
    pub issuer_account_id: Option<Uuid>,
    #[serde(default)]
    pub credential_id: Option<Uuid>,
    #[serde(default)]
    pub actor_principal_id: Option<Uuid>,
    #[serde(default)]
    pub actor_account_id: Option<Uuid>,
    #[serde(default)]
    pub actor_credential_id: Option<Uuid>,
    pub status: String,
    #[serde(default)]
    pub event: serde_json::Value,
}

fn queue_recovery_audit(
    state: &mut NodeState,
    event_id: Uuid,
    action: &str,
    request_id: Uuid,
    challenge_id: Option<Uuid>,
    space_uid: Uuid,
    principal_id: Uuid,
    account_id: Uuid,
    actor_principal_id: Option<Uuid>,
    actor_account_id: Option<Uuid>,
    actor_credential_id: Option<Uuid>,
    issuer_principal_id: Option<Uuid>,
    issuer_account_id: Option<Uuid>,
    issuer_credential_id: Option<Uuid>,
    safe_metadata: serde_json::Value,
) {
    state.recovery_audit_outbox.insert(
        event_id,
        RecoveryAuditOutboxRecord {
            event_id,
            action: action.to_string(),
            request_id,
            space_uid,
            principal_id,
            account_id,
            issuer_principal_id,
            issuer_account_id,
            credential_id: actor_credential_id,
            actor_principal_id,
            actor_account_id,
            actor_credential_id,
            status: "pending".to_string(),
            event: serde_json::json!({
                "event_id": event_id,
                "action": action,
                "request_id": request_id,
                "challenge_id": challenge_id,
                "space_uid": space_uid,
                "subject_principal_id": principal_id,
                "subject_account_id": account_id,
                "actor_principal_id": serde_json::to_value(actor_principal_id)
                    .unwrap_or(serde_json::Value::Null),
                "actor_account_id": serde_json::to_value(actor_account_id)
                    .unwrap_or(serde_json::Value::Null),
                "credential_id": serde_json::to_value(actor_credential_id)
                    .unwrap_or(serde_json::Value::Null),
                "metadata": safe_metadata,
                "issuer_principal_id": issuer_principal_id,
                "issuer_account_id": issuer_account_id,
                "issuer_credential_id": issuer_credential_id,
                "outcome": "success"
            }),
        },
    );
}

fn node_audit_fingerprint(
    subject_account_id: Option<Uuid>,
    actor_account_id: Option<Uuid>,
    credential_id: Option<Uuid>,
    action: &str,
    target_type: &str,
    target_id: Option<&str>,
    outcome: &str,
    request_id: Option<&str>,
    safe_metadata: &serde_json::Value,
) -> String {
    serde_json::to_string(&serde_json::json!({
        "subject_account_id": subject_account_id,
        "actor_account_id": actor_account_id,
        "credential_id": credential_id,
        "action": action,
        "target_type": target_type,
        "target_id": target_id,
        "outcome": outcome,
        "request_id": request_id,
        "safe_metadata": safe_metadata,
    }))
    .expect("node audit fingerprint serialization cannot fail")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PendingTotpEnrollment {
    encrypted_secret: String,
    expires_at: String,
    #[serde(default)]
    credential_generation: u64,
}

#[derive(Debug)]
pub enum TotpEnrollmentFinishError {
    InvalidOrExpired,
    Internal(anyhow::Error),
}

impl std::fmt::Display for TotpEnrollmentFinishError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOrExpired => {
                formatter.write_str("invalid or expired TOTP enrollment code")
            }
            Self::Internal(error) => write!(formatter, "finish TOTP enrollment: {error}"),
        }
    }
}

impl std::error::Error for TotpEnrollmentFinishError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidOrExpired => None,
            Self::Internal(error) => Some(error.as_ref()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceCredential {
    pub credential_id: Uuid,
    pub device_name: String,
    pub public_key_jwk: serde_json::Value,
    pub account_id: Uuid,
    #[serde(default)]
    pub credential_generation: u64,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceAuthorizationRequest {
    pub device_code_hash: String,
    pub user_code_hash: String,
    pub device_name: String,
    pub public_key_jwk: serde_json::Value,
    pub requested_space_uid: Option<Uuid>,
    pub requested_actions: BTreeSet<String>,
    pub approved_account_id: Option<Uuid>,
    pub approved_principal_id: Option<Uuid>,
    #[serde(default)]
    pub approved_credential_generation: Option<u64>,
    pub expires_at: String,
    pub used_at: Option<String>,
    #[serde(default)]
    pub last_polled_at: Option<String>,
    #[serde(default = "default_polling_interval")]
    pub polling_interval_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthorizationCodeGrant {
    pub code_hash: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub code_challenge: String,
    pub public_key_jwk: serde_json::Value,
    pub account_id: Uuid,
    #[serde(default)]
    pub credential_generation: u64,
    pub principal_id: Uuid,
    pub space_uid: Uuid,
    pub granted_actions: BTreeSet<String>,
    pub expires_at: String,
    pub used_at: Option<String>,
}

const fn default_polling_interval() -> u64 {
    5
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AgentCredential {
    pub credential_id: Uuid,
    pub agent_id: Uuid,
    pub public_key_jwk: serde_json::Value,
    pub created_at: String,
    #[serde(default)]
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OidcProvider {
    pub provider_id: Uuid,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OidcLoginAttempt {
    pub state_hash: String,
    pub provider_id: Uuid,
    pub nonce: String,
    pub pkce_verifier: String,
    pub invitation_hash: Option<String>,
    #[serde(default)]
    pub link_account_id: Option<Uuid>,
    #[serde(default)]
    pub link_account_generation: Option<u64>,
    #[serde(default)]
    pub invitation_account_id: Option<Uuid>,
    #[serde(default)]
    pub invitation_account_generation: Option<u64>,
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RefreshCredential {
    pub refresh_hash: String,
    pub credential_id: Uuid,
    pub account_id: Uuid,
    #[serde(default)]
    pub credential_generation: u64,
    pub principal_id: Uuid,
    pub space_uid: Uuid,
    pub granted_actions: BTreeSet<String>,
    pub expires_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeState {
    #[serde(skip)]
    control_version: Option<String>,
    pub schema_version: u32,
    pub node_id: Uuid,
    pub issuer: String,
    pub lifecycle: NodeLifecycle,
    pub setup: Option<OneTimeSecret>,
    #[serde(default)]
    pub accounts: BTreeMap<Uuid, HumanAccount>,
    /// Monotonic Node-local lifecycle epochs. Binding and account status
    /// changes advance this value and invalidate recovery tuples.
    #[serde(default)]
    pub account_lifecycle_epochs: BTreeMap<Uuid, u64>,
    #[serde(default)]
    pub authentication_methods: BTreeMap<Uuid, AuthenticationMethod>,
    #[serde(default)]
    pub passkeys: BTreeMap<String, StoredPasskey>,
    #[serde(default)]
    registration_challenges: BTreeMap<Uuid, RegistrationChallenge>,
    #[serde(default)]
    authentication_challenges: BTreeMap<Uuid, AuthenticationChallenge>,
    #[serde(default)]
    pub invitations: BTreeMap<Uuid, AccountInvitation>,
    #[serde(default)]
    pub recovery: BTreeMap<Uuid, RecoveryRecord>,
    #[serde(default)]
    pub owner_recovery_approvals: BTreeMap<Uuid, OwnerRecoveryApproval>,
    #[serde(default)]
    pub recovery_reset_markers: BTreeMap<Uuid, RecoveryResetMarker>,
    #[serde(default)]
    pub recovery_challenge_tombstones: BTreeMap<Uuid, RecoveryChallengeTombstone>,
    #[serde(default)]
    pub backup_rotation_requests: BTreeMap<Uuid, BackupRotationRecord>,
    #[serde(default)]
    pub recovery_audit_outbox: BTreeMap<Uuid, RecoveryAuditOutboxRecord>,
    #[serde(default)]
    node_recovery_fences: BTreeMap<Uuid, NodeRecoveryFence>,
    #[serde(default)]
    pending_totp_enrollments: BTreeMap<Uuid, PendingTotpEnrollment>,
    #[serde(default)]
    pub bindings: Vec<PrincipalBinding>,
    #[serde(default)]
    pub device_credentials: BTreeMap<Uuid, DeviceCredential>,
    #[serde(default)]
    pub device_authorizations: BTreeMap<String, DeviceAuthorizationRequest>,
    #[serde(default)]
    pub authorization_codes: BTreeMap<String, AuthorizationCodeGrant>,
    #[serde(default)]
    pub agent_credentials: BTreeMap<Uuid, AgentCredential>,
    #[serde(default)]
    pub refresh_credentials: BTreeMap<String, RefreshCredential>,
    #[serde(default)]
    pub proof_replay_cache: BTreeMap<String, String>,
    #[serde(default)]
    pub oidc_providers: BTreeMap<Uuid, OidcProvider>,
    #[serde(default)]
    pub oidc_attempts: BTreeMap<String, OidcLoginAttempt>,
    /// Per-session durable revocation epochs. Recovery mutations include the
    /// state CAS that observes this map, closing the cross-process window
    /// between validating an Owner session and committing the mutation.
    #[serde(default)]
    pub session_revocation_epochs: BTreeMap<Uuid, u64>,
}

fn bound_principal_for_account(
    state: &NodeState,
    space_uid: Option<Uuid>,
    account_id: Uuid,
) -> Result<Option<Uuid>> {
    let Some(space_uid) = space_uid else {
        return Ok(None);
    };
    let matches = state
        .bindings
        .iter()
        .find(|binding| binding.space_uid == space_uid && binding.node_account_id == account_id)
        .map(|binding| binding.principal_id);
    let count = state
        .bindings
        .iter()
        .filter(|binding| binding.space_uid == space_uid && binding.node_account_id == account_id)
        .count();
    match count {
        0 => Ok(None),
        1 => Ok(matches),
        _ => bail!("account binding is not unique"),
    }
}

fn node_recovery_fence_is_active(fence: &NodeRecoveryFence) -> bool {
    // Expiry is not an implicit release. An active fence remains a durable
    // write barrier until reconciliation or an explicit abort records the
    // terminal state.
    fence.status == "active"
}

fn node_recovery_fence_is_expired(fence: &NodeRecoveryFence) -> Result<bool> {
    Ok(node_recovery_fence_is_active(fence) && parse_timestamp(&fence.expires_at)? <= Utc::now())
}

fn node_write_was_committed_with_ambiguous_response(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains("node control write committed with an ambiguous response")
}

fn ensure_node_recovery_mutation_allowed(state: &mut NodeState, space_uid: Uuid) -> Result<()> {
    for fence in state
        .node_recovery_fences
        .values()
        .filter(|fence| node_recovery_fence_is_active(fence))
    {
        parse_timestamp(&fence.expires_at)?;
    }
    if state
        .node_recovery_fences
        .values()
        .any(|fence| fence.space_uid == space_uid && node_recovery_fence_is_active(fence))
    {
        bail!("RECOVERY_FENCE_UNAVAILABLE")
    }
    Ok(())
}

fn ensure_node_account_recovery_mutation_allowed(
    state: &mut NodeState,
    account_id: Uuid,
) -> Result<()> {
    for fence in state
        .node_recovery_fences
        .values()
        .filter(|fence| node_recovery_fence_is_active(fence))
    {
        parse_timestamp(&fence.expires_at)?;
    }
    if state.node_recovery_fences.values().any(|fence| {
        node_recovery_fence_is_active(fence)
            && state.bindings.iter().any(|binding| {
                binding.space_uid == fence.space_uid && binding.node_account_id == account_id
            })
    }) {
        bail!("RECOVERY_FENCE_UNAVAILABLE")
    }
    Ok(())
}

fn acquire_node_recovery_fence(
    state: &mut NodeState,
    space_uid: Uuid,
    principal_id: Uuid,
    account_id: Uuid,
    issuer_account_id: Uuid,
    snapshot: &RecoveryBindingSnapshot,
) -> Result<()> {
    if state
        .node_recovery_fences
        .get(&snapshot.recovery_fence_id)
        .is_some_and(|fence| fence.status != "active")
    {
        // A paired Space fence may have been released or completed while a
        // stale challenge was still in flight. Terminal Node fences must not
        // be reopened by that old challenge.
        bail!("RECOVERY_FENCE_UNAVAILABLE");
    }
    if state
        .accounts
        .get(&account_id)
        .is_none_or(|account| !matches!(account.status, AccountStatus::Active))
        || state
            .accounts
            .get(&issuer_account_id)
            .is_none_or(|account| !matches!(account.status, AccountStatus::Active))
    {
        bail!("recovery tuple is stale")
    }
    let target_generation = state
        .accounts
        .get(&account_id)
        .map(|account| account.credential_generation)
        .unwrap_or_default();
    let issuer_generation = state
        .accounts
        .get(&issuer_account_id)
        .map(|account| account.credential_generation)
        .unwrap_or_default();
    let target_epoch = state
        .account_lifecycle_epochs
        .get(&account_id)
        .copied()
        .unwrap_or_default();
    let issuer_epoch = state
        .account_lifecycle_epochs
        .get(&issuer_account_id)
        .copied()
        .unwrap_or_default();
    if target_generation != snapshot.target_generation
        || issuer_generation != snapshot.issuer_generation
        || target_epoch != snapshot.target_node_lifecycle_epoch
        || issuer_epoch != snapshot.issuer_node_lifecycle_epoch
        || state
            .bindings
            .iter()
            .filter(|binding| {
                binding.space_uid == space_uid
                    && binding.principal_id == principal_id
                    && binding.node_account_id == account_id
            })
            .count()
            != 1
        || state
            .bindings
            .iter()
            .filter(|binding| {
                binding.space_uid == space_uid && binding.node_account_id == issuer_account_id
            })
            .count()
            != 1
    {
        bail!("recovery tuple is stale")
    }

    if state
        .node_recovery_fences
        .values()
        .any(|fence| fence.status == "active" && fence.fence_id != snapshot.recovery_fence_id)
    {
        bail!("RECOVERY_FENCE_UNAVAILABLE")
    }
    if state
        .node_recovery_fences
        .get(&snapshot.recovery_fence_id)
        .is_some_and(|fence| fence.status == "active")
    {
        let fence = state
            .node_recovery_fences
            .get(&snapshot.recovery_fence_id)
            .expect("active fence was checked above");
        if fence.request_id != snapshot.request_id
            || fence.space_uid != space_uid
            || fence.principal_id != principal_id
            || fence.account_id != account_id
            || fence.issuer_account_id != issuer_account_id
            || fence.issuer_node_lifecycle_epoch != snapshot.issuer_node_lifecycle_epoch
            || fence.target_node_lifecycle_epoch != snapshot.target_node_lifecycle_epoch
            || fence.issuer_generation != snapshot.issuer_generation
            || fence.target_generation != snapshot.target_generation
        {
            bail!("recovery tuple is stale")
        }
        if snapshot.space_authorization_revision != 0 {
            state
                .node_recovery_fences
                .get_mut(&snapshot.recovery_fence_id)
                .expect("active fence was checked above")
                .phase = default_node_recovery_fence_phase();
        }
        return Ok(());
    }

    state.node_recovery_fences.insert(
        snapshot.recovery_fence_id,
        NodeRecoveryFence {
            fence_id: snapshot.recovery_fence_id,
            request_id: snapshot.request_id,
            space_uid,
            principal_id,
            account_id,
            issuer_account_id,
            issuer_node_lifecycle_epoch: snapshot.issuer_node_lifecycle_epoch,
            target_node_lifecycle_epoch: snapshot.target_node_lifecycle_epoch,
            issuer_generation: snapshot.issuer_generation,
            target_generation: snapshot.target_generation,
            expires_at: snapshot.recovery_fence_expires_at.clone(),
            status: "active".to_string(),
            phase: if snapshot.space_authorization_revision == 0 {
                "provisional".to_string()
            } else {
                default_node_recovery_fence_phase()
            },
        },
    );
    Ok(())
}

fn release_node_recovery_fence(state: &mut NodeState, fence_id: Option<Uuid>, status: &str) {
    if let Some(fence) = fence_id.and_then(|fence_id| state.node_recovery_fences.get_mut(&fence_id))
    {
        if fence.status == "active" {
            fence.status = status.to_string();
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BootstrapResult {
    pub setup_secret: String,
    pub setup_url: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
pub struct RegistrationStart {
    pub challenge_id: Uuid,
    pub public_key: CreationChallengeResponse,
}

#[derive(Debug, Serialize)]
pub struct AuthenticationStart {
    pub challenge_id: Uuid,
    pub public_key: RequestChallengeResponse,
}

#[derive(Debug, Serialize)]
pub struct RegistrationFinish {
    pub account: HumanAccount,
    pub session_id: String,
    pub recovery_codes: Vec<String>,
}

#[derive(Debug)]
pub struct InvitationRegistrationFinish {
    pub account: HumanAccount,
    pub session_id: String,
    pub invitation: AccountInvitation,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum InvitationRegistrationStart {
    Register {
        challenge_id: Uuid,
        public_key: Box<CreationChallengeResponse>,
    },
    Resume,
}

#[derive(Debug)]
pub struct RecoveryRegistrationFinish {
    pub account: HumanAccount,
    pub session_id: String,
    pub recovery_codes: Vec<String>,
    pub recovery_space_uid: Option<Uuid>,
    pub recovery_principal_id: Option<Uuid>,
    pub recovery_issuer_principal_id: Option<Uuid>,
    pub recovery_issuer_account_id: Option<Uuid>,
    pub recovery_issuer_credential_id: Option<Uuid>,
    pub recovery_request_id: Option<Uuid>,
}

#[derive(Clone)]
pub struct NodeIdentityService {
    state_store: Arc<dyn NodeControlStore>,
    webauthn: Webauthn,
    rp_id: String,
    public_origin: String,
    encryption_key: Arc<String>,
    state_lock: Arc<Mutex<()>>,
}

impl NodeIdentityService {
    pub fn public_origin(&self) -> &str {
        &self.public_origin
    }

    pub async fn append_node_audit(&self, input: NodeAuditInput<'_>) -> Result<NodeAuditEvent> {
        self.append_node_audit_with_id(Uuid::now_v7(), input).await
    }

    pub async fn append_node_audit_with_id(
        &self,
        event_id: Uuid,
        input: NodeAuditInput<'_>,
    ) -> Result<NodeAuditEvent> {
        if input.action.trim().is_empty() || input.target_type.trim().is_empty() {
            bail!("node audit action and target type are required");
        }
        if !input.safe_metadata.is_object() {
            bail!("node audit safe metadata must be an object");
        }
        let state = self.read_state().await?;
        let event = NodeAuditEvent {
            event_id,
            timestamp: timestamp(Utc::now()),
            node_id: state.node_id,
            subject_account_id: input.subject_account_id,
            actor_account_id: input.actor_account_id,
            credential_id: input.credential_id,
            action: input.action.trim().to_string(),
            target_type: input.target_type.trim().to_string(),
            target_id: input.target_id,
            outcome: input.outcome.trim().to_string(),
            request_id: input.request_id,
            safe_metadata: input.safe_metadata,
            canonical_event: state
                .recovery_audit_outbox
                .get(&event_id)
                .map(|record| record.event.clone()),
        };
        let key = format!("nodes/{}/audit/{}.json", state.node_id, event.event_id);
        if let Some(existing) = self.state_store.get(&key).await? {
            let existing: NodeAuditEvent = serde_json::from_slice(&existing.value)?;
            let expected_fingerprint = node_audit_fingerprint(
                event.subject_account_id,
                event.actor_account_id,
                event.credential_id,
                &event.action,
                &event.target_type,
                event.target_id.as_deref(),
                &event.outcome,
                event.request_id.as_deref(),
                &event.safe_metadata,
            );
            let actual_fingerprint = node_audit_fingerprint(
                existing.subject_account_id,
                existing.actor_account_id,
                existing.credential_id,
                &existing.action,
                &existing.target_type,
                existing.target_id.as_deref(),
                &existing.outcome,
                existing.request_id.as_deref(),
                &existing.safe_metadata,
            );
            if expected_fingerprint != actual_fingerprint {
                bail!("node audit event id conflicts with canonical payload");
            }
            return Ok(existing);
        }
        if let Err(create_error) = self
            .state_store
            .create_if_absent(&key, serde_json::to_vec(&event)?)
            .await
        {
            let Some(existing) = self.state_store.get(&key).await? else {
                return Err(create_error.into());
            };
            let existing: NodeAuditEvent = serde_json::from_slice(&existing.value)?;
            let expected_fingerprint = node_audit_fingerprint(
                event.subject_account_id,
                event.actor_account_id,
                event.credential_id,
                &event.action,
                &event.target_type,
                event.target_id.as_deref(),
                &event.outcome,
                event.request_id.as_deref(),
                &event.safe_metadata,
            );
            let actual_fingerprint = node_audit_fingerprint(
                existing.subject_account_id,
                existing.actor_account_id,
                existing.credential_id,
                &existing.action,
                &existing.target_type,
                existing.target_id.as_deref(),
                &existing.outcome,
                existing.request_id.as_deref(),
                &existing.safe_metadata,
            );
            if expected_fingerprint != actual_fingerprint {
                bail!("node audit event id conflicts with canonical payload");
            }
            return Ok(existing);
        }
        Ok(event)
    }

    pub async fn mark_recovery_audit_stage(&self, event_id: Uuid, status: &str) -> Result<()> {
        if !matches!(status, "pending" | "node" | "space" | "delivered") {
            bail!("invalid recovery audit outbox status");
        }
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if let Some(record) = state.recovery_audit_outbox.get_mut(&event_id) {
            let valid_transition = matches!(
                (record.status.as_str(), status),
                ("pending", "node") | ("node", "space") | ("space", "delivered")
            );
            if !valid_transition && record.status != status {
                bail!("invalid recovery audit outbox transition");
            }
            record.status = status.to_string();
            self.write_state(&state).await?;
        }
        Ok(())
    }

    pub async fn mark_recovery_audit_delivered(&self, event_id: Uuid) -> Result<()> {
        self.mark_recovery_audit_stage(event_id, "delivered").await
    }

    pub async fn pending_recovery_audits(&self) -> Result<Vec<RecoveryAuditOutboxRecord>> {
        Ok(self
            .read_state()
            .await?
            .recovery_audit_outbox
            .values()
            .filter(|record| record.status != "delivered")
            .cloned()
            .collect())
    }

    pub async fn recovery_audit_event(&self, event_id: Uuid) -> Result<Option<serde_json::Value>> {
        Ok(self
            .read_state()
            .await?
            .recovery_audit_outbox
            .get(&event_id)
            .map(|record| record.event.clone()))
    }

    pub async fn pending_recovery_fence_ids(&self, space_uid: Uuid) -> Result<Vec<Uuid>> {
        let state = self.read_state().await?;
        let mut ids = state
            .recovery_reset_markers
            .values()
            .filter(|marker| {
                marker.space_uid == space_uid
                    && marker.space_fence_status == "node_committed_space_fence_pending"
            })
            .map(|marker| marker.recovery_fence_id)
            .collect::<Vec<_>>();
        ids.extend(
            state
                .backup_rotation_requests
                .values()
                .filter(|record| {
                    record.space_uid == space_uid
                        && record.space_fence_status == "node_committed_space_fence_pending"
                })
                .filter_map(|record| record.recovery_fence_id),
        );
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    pub async fn active_recovery_fence_ids(&self, space_uid: Uuid) -> Result<Vec<Uuid>> {
        let state = self.read_state().await?;
        Ok(state
            .node_recovery_fences
            .values()
            .filter(|fence| fence.space_uid == space_uid && node_recovery_fence_is_active(fence))
            .map(|fence| fence.fence_id)
            .collect())
    }

    pub async fn recovery_fence_for_request(
        &self,
        request_id: Uuid,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_account_id: Uuid,
    ) -> Result<Option<RecoveryBindingSnapshot>> {
        let state = self.read_state().await?;
        Ok(state
            .node_recovery_fences
            .values()
            .find(|fence| {
                fence.status == "active"
                    && fence.request_id == request_id
                    && fence.space_uid == space_uid
                    && fence.principal_id == principal_id
                    && fence.account_id == account_id
                    && fence.issuer_account_id == issuer_account_id
            })
            .map(|fence| RecoveryBindingSnapshot {
                request_id,
                recovery_fence_id: fence.fence_id,
                recovery_fence_expires_at: fence.expires_at.clone(),
                space_authorization_revision: 0,
                issuer_space_lifecycle_epoch: 0,
                target_space_lifecycle_epoch: 0,
                issuer_node_lifecycle_epoch: fence.issuer_node_lifecycle_epoch,
                target_node_lifecycle_epoch: fence.target_node_lifecycle_epoch,
                issuer_generation: fence.issuer_generation,
                target_generation: fence.target_generation,
            }))
    }

    pub async fn recovery_fence_phase(&self, fence_id: Uuid) -> Result<Option<String>> {
        Ok(self
            .read_state()
            .await?
            .node_recovery_fences
            .get(&fence_id)
            .map(|fence| fence.phase.clone()))
    }

    pub async fn recovery_fence_status(&self, fence_id: Uuid) -> Result<Option<String>> {
        Ok(self
            .read_state()
            .await?
            .node_recovery_fences
            .get(&fence_id)
            .map(|fence| fence.status.clone()))
    }

    pub async fn owner_recovery_fence_ids_for_target(&self, account_id: Uuid) -> Result<Vec<Uuid>> {
        let state = self.read_state().await?;
        Ok(state
            .owner_recovery_approvals
            .values()
            .filter(|approval| {
                approval.account_id == account_id
                    && approval.used_at.is_none()
                    && approval.invalidated_at.is_none()
            })
            .filter_map(|approval| approval.recovery_fence_id)
            .collect())
    }

    pub async fn expired_recovery_fence(&self, fence_id: Uuid) -> Result<bool> {
        let state = self.read_state().await?;
        match state.node_recovery_fences.get(&fence_id) {
            Some(fence) => node_recovery_fence_is_expired(fence),
            None => Ok(false),
        }
    }

    /// Reserve the Node-side half of a recovery fence with the same
    /// conditional-CAS state transition used by all Node lifecycle writes.
    /// The server first records a provisional Node fence and then reserves the
    /// Space half. A later call promotes the same Node identity to paired once
    /// the Space CAS is observable.
    pub async fn acquire_recovery_fence(
        &self,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_account_id: Uuid,
        snapshot: Option<&RecoveryBindingSnapshot>,
    ) -> Result<()> {
        let Some(snapshot) = snapshot else {
            return Ok(());
        };
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        acquire_node_recovery_fence(
            &mut state,
            space_uid,
            principal_id,
            account_id,
            issuer_account_id,
            snapshot,
        )?;
        self.write_state(&state).await
    }

    pub async fn complete_recovery_fence(&self, fence_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let Some(fence) = state.node_recovery_fences.get(&fence_id).cloned() else {
            bail!("recovery fence is unavailable")
        };
        if fence.status == "completed" {
            return Ok(());
        }
        if fence.status != "active" {
            bail!("recovery fence is not active")
        }
        if parse_timestamp(&fence.expires_at)? <= Utc::now() {
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        let issuer_epoch = state
            .account_lifecycle_epochs
            .get(&fence.issuer_account_id)
            .copied()
            .unwrap_or_default();
        let target_epoch = state
            .account_lifecycle_epochs
            .get(&fence.account_id)
            .copied()
            .unwrap_or_default();
        let target_epoch_after_reset = fence.target_node_lifecycle_epoch.checked_add(1);
        if issuer_epoch != fence.issuer_node_lifecycle_epoch
            || (target_epoch != fence.target_node_lifecycle_epoch
                && Some(target_epoch) != target_epoch_after_reset)
        {
            bail!("RECOVERY_FENCE_UNAVAILABLE")
        }
        state
            .node_recovery_fences
            .get_mut(&fence_id)
            .expect("fence was checked above")
            .status = "completed".to_string();
        self.write_state(&state).await
    }

    pub async fn release_recovery_fence(&self, fence_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let Some(fence) = state.node_recovery_fences.get_mut(&fence_id) else {
            return Ok(());
        };
        if fence.status == "active" {
            fence.status = "released".to_string();
            self.write_state(&state).await?;
        }
        Ok(())
    }

    pub async fn recovery_account_lifecycle_epoch(&self, account_id: Uuid) -> Result<u64> {
        Ok(self
            .read_state()
            .await?
            .account_lifecycle_epochs
            .get(&account_id)
            .copied()
            .unwrap_or_default())
    }

    pub async fn backup_rotation_request(
        &self,
        request_id: Uuid,
    ) -> Result<Option<BackupRotationRecord>> {
        Ok(self
            .read_state()
            .await?
            .backup_rotation_requests
            .get(&request_id)
            .cloned())
    }

    /// Atomically claim the encrypted plaintext codes for their single
    /// response. The ciphertext remains for forensic durability, but the
    /// delivery timestamp makes subsequent idempotency retries terminal.
    pub async fn take_backup_rotation_codes(&self, request_id: Uuid) -> Result<Vec<String>> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let record = state
            .backup_rotation_requests
            .get(&request_id)
            .cloned()
            .ok_or_else(|| anyhow!("backup rotation request is missing"))?;
        if record.codes_delivered_at.is_some() {
            bail!("backup rotation codes already delivered");
        }
        if record.codes_invalidated_at.is_some() {
            bail!("backup rotation codes are no longer current");
        }
        let encrypted_codes = record
            .encrypted_codes
            .as_deref()
            .ok_or_else(|| anyhow!("backup rotation response is unavailable"))?;
        let codes: Vec<String> = serde_json::from_slice(&decrypt_recovery_secret(
            &self.encryption_key,
            encrypted_codes,
        )?)
        .context("decode backup rotation response")?;
        let delivery_id = Uuid::now_v7();
        state
            .backup_rotation_requests
            .get_mut(&request_id)
            .expect("backup rotation request was checked above")
            .codes_delivered_at = Some(timestamp(Utc::now()));
        state
            .backup_rotation_requests
            .get_mut(&request_id)
            .expect("backup rotation request was checked above")
            .codes_delivery_id = Some(delivery_id);
        if let Err(error) = self.write_state(&state).await {
            let observed =
                self.read_state().await.ok().and_then(|observed| {
                    observed.backup_rotation_requests.get(&request_id).cloned()
                });
            if observed
                .as_ref()
                .and_then(|record| record.codes_delivery_id)
                == Some(delivery_id)
            {
                // The claim is ours even if a later state write changed the
                // enclosing Node document before verification completed.
            } else if observed
                .as_ref()
                .and_then(|record| record.codes_delivered_at.as_ref())
                .is_some()
            {
                bail!("backup rotation codes already delivered");
            } else {
                // An unknown/pre-CAS outcome is fail-closed. A later retry
                // can still recover the encrypted material if this marker
                // write did not commit; only a matching claim may be returned
                // from this delivery attempt.
                return Err(error);
            }
        }
        Ok(codes)
    }

    pub async fn recovery_reset_marker(
        &self,
        reset_id: Uuid,
    ) -> Result<Option<RecoveryResetMarker>> {
        Ok(self
            .read_state()
            .await?
            .recovery_reset_markers
            .get(&reset_id)
            .cloned())
    }

    /// Claim the encrypted one-time owner-reset response after the paired
    /// recovery fence is terminal. Retrying requires the same WebAuthn
    /// completion payload, so a challenge identifier alone cannot disclose a
    /// replacement session or recovery codes.
    pub async fn take_owner_recovery_response_for_challenge(
        &self,
        challenge_id: Uuid,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<Option<(HumanAccount, String, Vec<String>, RecoveryResetMarker)>> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let Some((reset_id, marker)) = state
            .recovery_reset_markers
            .iter()
            .find(|(_, marker)| marker.challenge_id == challenge_id)
            .map(|(reset_id, marker)| (*reset_id, marker.clone()))
        else {
            return Ok(None);
        };
        if marker.response_delivered_at.is_some() {
            bail!("owner reset response already delivered");
        }
        if marker.response_invalidated_at.is_some() {
            bail!("owner reset response is no longer current");
        }
        if marker.space_fence_status != "reconciled" {
            bail!("RECOVERY_FENCE_UNAVAILABLE");
        }
        let proof = marker
            .completion_proof_hash
            .as_deref()
            .ok_or_else(|| anyhow!("owner reset response is unavailable"))?;
        if proof != token_hash(&serde_json::to_string(credential)?) {
            bail!("owner reset response proof is invalid");
        }
        let encrypted_response = marker
            .encrypted_response
            .as_deref()
            .ok_or_else(|| anyhow!("owner reset response is unavailable"))?;
        let (session_token, recovery_codes): (String, Vec<String>) = serde_json::from_slice(
            &decrypt_recovery_secret(&self.encryption_key, encrypted_response)?,
        )
        .context("decode owner reset response")?;
        let account = state
            .accounts
            .get(&marker.account_id)
            .cloned()
            .ok_or_else(|| anyhow!("recovery account is missing"))?;
        let delivery_id = Uuid::now_v7();
        state
            .recovery_reset_markers
            .get_mut(&reset_id)
            .expect("owner reset marker was checked above")
            .response_delivered_at = Some(timestamp(Utc::now()));
        state
            .recovery_reset_markers
            .get_mut(&reset_id)
            .expect("owner reset marker was checked above")
            .response_delivery_id = Some(delivery_id);
        if let Err(error) = self.write_state(&state).await {
            let observed = self
                .read_state()
                .await
                .ok()
                .and_then(|observed| observed.recovery_reset_markers.get(&reset_id).cloned());
            if observed
                .as_ref()
                .and_then(|marker| marker.response_delivery_id)
                == Some(delivery_id)
            {
                // The claim is ours even if a later state write changed the
                // enclosing Node document before verification completed.
            } else if observed
                .as_ref()
                .and_then(|marker| marker.response_delivered_at.as_ref())
                .is_some()
            {
                bail!("owner reset response already delivered");
            } else {
                return Err(error);
            }
        }
        Ok(Some((account, session_token, recovery_codes, marker)))
    }

    pub async fn mark_recovery_fence_reconciled(&self, fence_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let mut changed = false;
        for marker in state.recovery_reset_markers.values_mut() {
            if marker.recovery_fence_id == fence_id
                && marker.space_fence_status == "node_committed_space_fence_pending"
            {
                marker.space_fence_status = "reconciled".to_string();
                changed = true;
            }
        }
        for record in state.backup_rotation_requests.values_mut() {
            if record.recovery_fence_id == Some(fence_id)
                && record.space_fence_status == "node_committed_space_fence_pending"
            {
                record.space_fence_status = "reconciled".to_string();
                changed = true;
            }
        }
        if changed {
            self.write_state(&state).await?;
        }
        Ok(())
    }

    /// Terminally reconcile a durable Node recovery mutation when the paired
    /// Space fence was explicitly released or expired before it could be
    /// completed. No Space membership mutation is made by the recovery
    /// operation, so releasing both barriers is safe once the pending marker
    /// proves the Node mutation is durable.
    pub async fn abort_recovery_fence_after_space_abort(&self, fence_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let mut changed = false;
        let mut has_pending_marker = false;
        for marker in state.recovery_reset_markers.values_mut() {
            if marker.recovery_fence_id == fence_id
                && marker.space_fence_status == "node_committed_space_fence_pending"
            {
                marker.space_fence_status = "reconciled".to_string();
                has_pending_marker = true;
                changed = true;
            }
        }
        for record in state.backup_rotation_requests.values_mut() {
            if record.recovery_fence_id == Some(fence_id)
                && record.space_fence_status == "node_committed_space_fence_pending"
            {
                record.space_fence_status = "reconciled".to_string();
                has_pending_marker = true;
                changed = true;
            }
        }
        if has_pending_marker {
            if let Some(fence) = state.node_recovery_fences.get_mut(&fence_id) {
                if fence.status == "active" {
                    fence.status = "released".to_string();
                    changed = true;
                }
            }
        }
        if changed {
            self.write_state(&state).await?;
        }
        Ok(())
    }

    pub async fn list_node_audit(&self, limit: usize) -> Result<Vec<NodeAuditEvent>> {
        let state = self.read_state().await?;
        let prefix = format!("nodes/{}/audit", state.node_id);
        let mut events = Vec::new();
        for key in self.state_store.list_prefix(&prefix).await? {
            if let Some(record) = self.state_store.get(&key).await? {
                events.push(serde_json::from_slice::<NodeAuditEvent>(&record.value)?);
            }
        }
        events.sort_by(|left, right| right.timestamp.cmp(&left.timestamp));
        events.truncate(limit.clamp(1, 500));
        Ok(events)
    }

    pub fn new(
        operator: Operator,
        rp_id: impl Into<String>,
        public_origin: impl Into<String>,
    ) -> Result<Self> {
        let state_store: Arc<dyn NodeControlStore> =
            Arc::new(OpenDalNodeControlStore::new(operator)?);
        let secret_store = EnvironmentSecretStore;
        Self::from_parts(
            state_store,
            secret_store.encryption_root_key()?,
            rp_id,
            public_origin,
        )
    }

    #[doc(hidden)]
    pub fn new_for_tests(
        rp_id: impl Into<String>,
        public_origin: impl Into<String>,
    ) -> Result<Self> {
        Self::from_parts(
            Arc::new(OpenDalNodeControlStore::memory_for_tests()),
            Arc::from([0x5a; 32]),
            rp_id,
            public_origin,
        )
    }

    fn from_parts(
        state_store: Arc<dyn NodeControlStore>,
        secret_key: Arc<[u8]>,
        rp_id: impl Into<String>,
        public_origin: impl Into<String>,
    ) -> Result<Self> {
        let rp_id = rp_id.into();
        let public_origin = public_origin.into();
        let origin = Url::parse(&public_origin).context("UGOITE_PUBLIC_ORIGIN must be a URL")?;
        if origin.path() != "/" || origin.query().is_some() || origin.fragment().is_some() {
            bail!("UGOITE_PUBLIC_ORIGIN must contain only scheme, host, and optional port");
        }
        let host = origin
            .host_str()
            .ok_or_else(|| anyhow!("UGOITE_PUBLIC_ORIGIN must contain a host"))?;
        let loopback = host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if origin.scheme() != "https" && !loopback {
            bail!("HTTPS is required for non-loopback Passkey deployments");
        }
        let webauthn = WebauthnBuilder::new(&rp_id, &origin)
            .context("invalid WebAuthn relying-party configuration")?
            .rp_name("Ugoite")
            .build()
            .context("build WebAuthn configuration")?;
        Ok(Self {
            state_store,
            webauthn,
            rp_id,
            public_origin,
            encryption_key: Arc::new(URL_SAFE_NO_PAD.encode(secret_key.as_ref())),
            state_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn bootstrap_if_needed(&self) -> Result<Option<BootstrapResult>> {
        let _guard = self.state_lock.lock().await;
        if self.state_exists().await? {
            let state = self.read_state().await?;
            if state.issuer != self.public_origin {
                bail!(
                    "canonical public origin changed from {}; explicit Passkey migration is required",
                    state.issuer
                );
            }
            if state
                .passkeys
                .values()
                .any(|passkey| passkey.rp_id != self.rp_id)
            {
                bail!("WebAuthn RP ID changed; refusing to invalidate enrolled Passkeys silently");
            }
            return Ok(None);
        }
        let setup_secret = random_token(32)?;
        let expires_at = timestamp(Utc::now() + Duration::minutes(SETUP_LIFETIME_MINUTES));
        let state = NodeState {
            control_version: None,
            schema_version: 1,
            node_id: Uuid::now_v7(),
            issuer: self.public_origin.clone(),
            lifecycle: NodeLifecycle::Uninitialized,
            setup: Some(OneTimeSecret {
                token_hash: token_hash(&setup_secret),
                expires_at: expires_at.clone(),
                used_at: None,
            }),
            accounts: BTreeMap::new(),
            account_lifecycle_epochs: BTreeMap::new(),
            authentication_methods: BTreeMap::new(),
            passkeys: BTreeMap::new(),
            registration_challenges: BTreeMap::new(),
            authentication_challenges: BTreeMap::new(),
            invitations: BTreeMap::new(),
            recovery: BTreeMap::new(),
            owner_recovery_approvals: BTreeMap::new(),
            recovery_reset_markers: BTreeMap::new(),
            recovery_challenge_tombstones: BTreeMap::new(),
            backup_rotation_requests: BTreeMap::new(),
            recovery_audit_outbox: BTreeMap::new(),
            node_recovery_fences: BTreeMap::new(),
            pending_totp_enrollments: BTreeMap::new(),
            bindings: Vec::new(),
            device_credentials: BTreeMap::new(),
            device_authorizations: BTreeMap::new(),
            authorization_codes: BTreeMap::new(),
            agent_credentials: BTreeMap::new(),
            refresh_credentials: BTreeMap::new(),
            proof_replay_cache: BTreeMap::new(),
            oidc_providers: BTreeMap::new(),
            oidc_attempts: BTreeMap::new(),
            session_revocation_epochs: BTreeMap::new(),
        };
        self.write_state(&state).await?;
        Ok(Some(BootstrapResult {
            setup_url: format!(
                "{}/setup#secret={setup_secret}",
                self.public_origin.trim_end_matches('/')
            ),
            setup_secret,
            expires_at,
        }))
    }

    pub async fn state_summary(&self) -> Result<serde_json::Value> {
        let state = self.read_state().await?;
        Ok(serde_json::json!({
            "status": state.lifecycle,
            "node_id": state.node_id,
            "issuer": state.issuer,
            "rp_id": self.rp_id,
            "passkey": true,
            "oidc": state.oidc_providers.values().any(|provider| provider.enabled),
            "login_required": true
        }))
    }

    pub async fn start_setup_registration(
        &self,
        setup_secret: &str,
        display_name: &str,
    ) -> Result<RegistrationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if !matches!(state.lifecycle, NodeLifecycle::Uninitialized) {
            bail!("node setup is already complete");
        }
        validate_secret(state.setup.as_ref(), setup_secret, "setup secret")?;
        let account_id = Uuid::now_v7();
        let display_name = normalized_display_name(display_name)?;
        let (mut public_key, registration) = self
            .webauthn
            .start_passkey_registration(account_id, &account_id.to_string(), &display_name, None)
            .context("start passkey registration")?;
        // Ugoite registration always asks the authenticator for a discoverable credential.
        if let Some(selection) = public_key.public_key.authenticator_selection.as_mut() {
            selection.resident_key = Some(
                serde_json::from_value(serde_json::json!("required"))
                    .context("construct resident-key requirement")?,
            );
            selection.require_resident_key = true;
        }
        let challenge_id = Uuid::now_v7();
        state.registration_challenges.insert(
            challenge_id,
            RegistrationChallenge {
                account_id,
                credential_generation: 0,
                display_name,
                state: registration,
                public_key: Some(public_key.clone()),
                purpose: RegistrationPurpose::Setup,
                expires_at: timestamp(Utc::now() + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
            },
        );
        self.write_state(&state).await?;
        Ok(RegistrationStart {
            challenge_id,
            public_key,
        })
    }

    pub async fn finish_setup_registration(
        &self,
        setup_secret: &str,
        challenge_id: Uuid,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<RegistrationFinish> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        validate_secret(state.setup.as_ref(), setup_secret, "setup secret")?;
        let challenge = state
            .registration_challenges
            .remove(&challenge_id)
            .ok_or_else(|| anyhow!("unknown or consumed registration challenge"))?;
        validate_expiry(&challenge.expires_at, "registration challenge")?;
        if !matches!(challenge.purpose, RegistrationPurpose::Setup) {
            bail!("registration challenge has the wrong purpose");
        }
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &challenge.state)
            .context("verify passkey registration")?;
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        if state.passkeys.contains_key(&credential_id) {
            bail!("credential is already registered");
        }
        let now = timestamp(Utc::now());
        let account = HumanAccount {
            account_id: challenge.account_id,
            display_name: challenge.display_name,
            status: AccountStatus::Active,
            created_at: now.clone(),
            node_roles: [NodeRole::NodeAdmin].into_iter().collect(),
            credential_generation: 0,
        };
        state.accounts.insert(account.account_id, account.clone());
        let method_id = Uuid::now_v7();
        state.authentication_methods.insert(
            method_id,
            AuthenticationMethod {
                method_id,
                account_id: account.account_id,
                kind: AuthenticationMethodKind::Passkey,
                external_subject: None,
                created_at: now.clone(),
                last_used_at: Some(now.clone()),
            },
        );
        state.passkeys.insert(
            credential_id.clone(),
            StoredPasskey {
                credential_id,
                account_id: account.account_id,
                method_id,
                passkey,
                created_at: now.clone(),
                last_used_at: Some(now.clone()),
                rp_id: self.rp_id.clone(),
            },
        );
        if let Some(setup) = state.setup.as_mut() {
            setup.used_at = Some(now.clone());
        }
        let recovery_codes = (0..8)
            .map(|_| random_recovery_code())
            .collect::<Result<Vec<_>>>()?;
        state.recovery.insert(
            account.account_id,
            RecoveryRecord {
                account_id: account.account_id,
                code_hashes: recovery_codes.iter().map(|code| token_hash(code)).collect(),
                totp_secret_encrypted: None,
                created_at: now,
                failed_attempts: 0,
                locked_until: None,
            },
        );
        let session_id = self
            .create_session(
                &state,
                account.account_id,
                method_id,
                AssuranceLevel::PhishingResistant,
            )
            .await?;
        self.write_state(&state).await?;
        Ok(RegistrationFinish {
            account,
            session_id,
            recovery_codes,
        })
    }

    pub async fn start_authentication(&self) -> Result<AuthenticationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if !matches!(state.lifecycle, NodeLifecycle::Active) && state.passkeys.is_empty() {
            bail!("node setup is not complete");
        }
        let credentials = state
            .passkeys
            .values()
            .filter(|stored| {
                state
                    .accounts
                    .get(&stored.account_id)
                    .is_some_and(|account| matches!(account.status, AccountStatus::Active))
            })
            .map(|stored| stored.passkey.clone())
            .collect::<Vec<_>>();
        if credentials.is_empty() {
            bail!("no active passkey credentials are registered");
        }
        let (public_key, auth_state) = self
            .webauthn
            .start_passkey_authentication(&credentials)
            .context("start passkey authentication")?;
        let challenge_id = Uuid::now_v7();
        state.authentication_challenges.insert(
            challenge_id,
            AuthenticationChallenge {
                state: auth_state,
                expires_at: timestamp(Utc::now() + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
            },
        );
        self.write_state(&state).await?;
        Ok(AuthenticationStart {
            challenge_id,
            public_key,
        })
    }

    pub async fn start_invitation_registration(
        &self,
        invitation_token: &str,
    ) -> Result<InvitationRegistrationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let invitation = state
            .invitations
            .values()
            .find(|invitation| invitation.token_hash == token_hash(invitation_token))
            .cloned()
            .ok_or_else(|| anyhow!("invitation is invalid"))?;
        if let Some(space_uid) = invitation.space_uid {
            ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
        }
        if let Some(acceptance) = invitation.acceptance.as_ref() {
            if matches!(
                acceptance.kind(),
                InvitationAcceptanceKind::PasskeyRegistration
            ) && state
                .accounts
                .get(&acceptance.account_id())
                .is_some_and(|account| matches!(account.status, AccountStatus::Active))
            {
                return Ok(InvitationRegistrationStart::Resume);
            }
            bail!("invitation was already used");
        }
        validate_expiry(&invitation.expires_at, "invitation")?;
        let account_id = Uuid::now_v7();
        let (mut public_key, registration) = self
            .webauthn
            .start_passkey_registration(
                account_id,
                &account_id.to_string(),
                &invitation.display_name,
                None,
            )
            .context("start invited Passkey registration")?;
        if let Some(selection) = public_key.public_key.authenticator_selection.as_mut() {
            selection.resident_key = Some(serde_json::from_value(serde_json::json!("required"))?);
            selection.require_resident_key = true;
        }
        let challenge_id = Uuid::now_v7();
        state.registration_challenges.insert(
            challenge_id,
            RegistrationChallenge {
                account_id,
                credential_generation: 0,
                display_name: invitation.display_name,
                state: registration,
                public_key: Some(public_key.clone()),
                purpose: RegistrationPurpose::Invitation {
                    invitation_id: invitation.invitation_id,
                },
                expires_at: timestamp(Utc::now() + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
            },
        );
        self.write_state(&state).await?;
        Ok(InvitationRegistrationStart::Register {
            challenge_id,
            public_key: Box::new(public_key),
        })
    }

    pub async fn finish_invitation_registration(
        &self,
        invitation_token: &str,
        challenge_id: Uuid,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<InvitationRegistrationFinish> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let challenge = state
            .registration_challenges
            .get(&challenge_id)
            .cloned()
            .ok_or_else(|| anyhow!("unknown or consumed registration challenge"))?;
        validate_expiry(&challenge.expires_at, "registration challenge")?;
        let RegistrationPurpose::Invitation { invitation_id } = challenge.purpose else {
            bail!("registration challenge has the wrong purpose");
        };
        let invitation = state
            .invitations
            .get(&invitation_id)
            .cloned()
            .ok_or_else(|| anyhow!("invitation not found"))?;
        if invitation.token_hash != token_hash(invitation_token) || invitation.acceptance.is_some()
        {
            bail!("invitation is invalid or used");
        }
        if let Some(space_uid) = invitation.space_uid {
            ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
        }
        validate_expiry(&invitation.expires_at, "invitation")?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &challenge.state)
            .context("verify invited Passkey registration")?;
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        if state.passkeys.contains_key(&credential_id) {
            bail!("credential is already registered");
        }
        let now = timestamp(Utc::now());
        let account = HumanAccount {
            account_id: challenge.account_id,
            display_name: challenge.display_name,
            status: AccountStatus::Active,
            created_at: now.clone(),
            node_roles: BTreeSet::new(),
            credential_generation: 0,
        };
        state.accounts.insert(account.account_id, account.clone());
        let method_id = Uuid::now_v7();
        state.authentication_methods.insert(
            method_id,
            AuthenticationMethod {
                method_id,
                account_id: account.account_id,
                kind: AuthenticationMethodKind::Passkey,
                external_subject: None,
                created_at: now.clone(),
                last_used_at: Some(now.clone()),
            },
        );
        state.passkeys.insert(
            credential_id.clone(),
            StoredPasskey {
                credential_id,
                account_id: account.account_id,
                method_id,
                passkey,
                created_at: now.clone(),
                last_used_at: Some(now),
                rp_id: self.rp_id.clone(),
            },
        );
        let invitation = state
            .invitations
            .get_mut(&invitation_id)
            .ok_or_else(|| anyhow!("invitation not found"))?;
        invitation.acceptance = Some(InvitationAcceptance::Pending {
            account_id: account.account_id,
            principal_id: Uuid::now_v7(),
            kind: InvitationAcceptanceKind::PasskeyRegistration,
            claimed_at: timestamp(Utc::now()),
            credential_generation: 0,
        });
        let invitation = invitation.clone();
        state.registration_challenges.remove(&challenge_id);
        let session_id = self
            .create_session(
                &state,
                account.account_id,
                method_id,
                AssuranceLevel::PhishingResistant,
            )
            .await?;
        self.write_state(&state).await?;
        Ok(InvitationRegistrationFinish {
            account,
            session_id,
            invitation,
        })
    }

    pub async fn finish_authentication(
        &self,
        challenge_id: Uuid,
        credential: &PublicKeyCredential,
    ) -> Result<(HumanAccount, String)> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let challenge = state
            .authentication_challenges
            .remove(&challenge_id)
            .ok_or_else(|| anyhow!("unknown or consumed authentication challenge"))?;
        validate_expiry(&challenge.expires_at, "authentication challenge")?;
        let result = self
            .webauthn
            .finish_passkey_authentication(credential, &challenge.state)
            .context("verify passkey authentication")?;
        let credential_id = URL_SAFE_NO_PAD.encode(result.cred_id());
        let stored = state
            .passkeys
            .get_mut(&credential_id)
            .ok_or_else(|| anyhow!("authenticated credential is not registered"))?;
        stored.passkey.update_credential(&result);
        stored.last_used_at = Some(timestamp(Utc::now()));
        let method_id = stored.method_id;
        let account = state
            .accounts
            .get(&stored.account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .cloned()
            .ok_or_else(|| anyhow!("account is not active"))?;
        let now = timestamp(Utc::now());
        for method in state.authentication_methods.values_mut().filter(|method| {
            method.account_id == account.account_id
                && matches!(method.kind, AuthenticationMethodKind::Passkey)
        }) {
            method.last_used_at = Some(now.clone());
        }
        let session_id = self
            .create_session(
                &state,
                account.account_id,
                method_id,
                AssuranceLevel::PhishingResistant,
            )
            .await?;
        self.write_state(&state).await?;
        Ok((account, session_id))
    }

    pub async fn list_passkeys(&self, account_id: Uuid) -> Result<Vec<serde_json::Value>> {
        let state = self.read_state().await?;
        Ok(state
            .passkeys
            .values()
            .filter(|credential| credential.account_id == account_id)
            .map(|credential| {
                serde_json::json!({
                    "credential_id": credential.credential_id,
                    "created_at": credential.created_at,
                    "last_used_at": credential.last_used_at,
                    "rp_id": credential.rp_id,
                })
            })
            .collect())
    }

    pub async fn start_add_passkey(&self, account_id: Uuid) -> Result<RegistrationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        let account = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .cloned()
            .ok_or_else(|| anyhow!("account is not active"))?;
        let (mut public_key, registration) = self.webauthn.start_passkey_registration(
            // Discoverable credentials are keyed by the RP and user handle. A fresh handle
            // keeps an additional passkey on the same authenticator from replacing an
            // existing credential for this account.
            Uuid::now_v7(),
            &account_id.to_string(),
            &account.display_name,
            None,
        )?;
        if let Some(selection) = public_key.public_key.authenticator_selection.as_mut() {
            selection.resident_key = Some(serde_json::from_value(serde_json::json!("required"))?);
            selection.require_resident_key = true;
        }
        let challenge_id = Uuid::now_v7();
        state.registration_challenges.insert(
            challenge_id,
            RegistrationChallenge {
                account_id,
                credential_generation: state
                    .accounts
                    .get(&account_id)
                    .map(|account| account.credential_generation)
                    .unwrap_or_default(),
                display_name: account.display_name,
                state: registration,
                public_key: Some(public_key.clone()),
                purpose: RegistrationPurpose::AddCredential,
                expires_at: timestamp(Utc::now() + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
            },
        );
        self.write_state(&state).await?;
        Ok(RegistrationStart {
            challenge_id,
            public_key,
        })
    }

    pub async fn finish_add_passkey(
        &self,
        account_id: Uuid,
        challenge_id: Uuid,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<serde_json::Value> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let challenge = state
            .registration_challenges
            .remove(&challenge_id)
            .ok_or_else(|| anyhow!("unknown registration challenge"))?;
        if challenge.account_id != account_id
            || !matches!(challenge.purpose, RegistrationPurpose::AddCredential)
        {
            bail!("registration challenge has wrong account or purpose");
        }
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        if state
            .accounts
            .get(&account_id)
            .is_none_or(|account| account.credential_generation != challenge.credential_generation)
        {
            bail!("registration challenge is stale");
        }
        validate_expiry(&challenge.expires_at, "registration challenge")?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &challenge.state)?;
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        if state.passkeys.contains_key(&credential_id) {
            bail!("credential is already registered");
        }
        let now = timestamp(Utc::now());
        invalidate_pending_recovery_responses(&mut state, account_id, &now);
        let method_id = Uuid::now_v7();
        state.passkeys.insert(
            credential_id.clone(),
            StoredPasskey {
                credential_id: credential_id.clone(),
                account_id,
                method_id,
                passkey,
                created_at: now.clone(),
                last_used_at: None,
                rp_id: self.rp_id.clone(),
            },
        );
        state.authentication_methods.insert(
            method_id,
            AuthenticationMethod {
                method_id,
                account_id,
                kind: AuthenticationMethodKind::Passkey,
                external_subject: None,
                created_at: now.clone(),
                last_used_at: None,
            },
        );
        let setup_activated = if !matches!(state.lifecycle, NodeLifecycle::Active)
            && state
                .accounts
                .get(&account_id)
                .is_some_and(|account| account.node_roles.contains(&NodeRole::NodeAdmin))
            && state
                .passkeys
                .values()
                .filter(|credential| credential.account_id == account_id)
                .count()
                >= 2
        {
            state.lifecycle = NodeLifecycle::Active;
            true
        } else {
            false
        };
        self.write_state(&state).await?;
        Ok(
            serde_json::json!({"credential_id": credential_id, "created_at": now, "rp_id": self.rp_id, "setup_activated": setup_activated}),
        )
    }

    pub async fn revoke_passkey(
        &self,
        account_id: Uuid,
        expected_generation: u64,
        credential_id: &str,
    ) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        if state
            .accounts
            .get(&account_id)
            .is_none_or(|account| account.credential_generation != expected_generation)
        {
            bail!("credential generation is stale");
        }
        let owned_count = state
            .passkeys
            .values()
            .filter(|credential| credential.account_id == account_id)
            .count();
        if owned_count <= 1 {
            bail!("cannot revoke the account's last Passkey");
        }
        if state
            .passkeys
            .get(credential_id)
            .is_none_or(|credential| credential.account_id != account_id)
        {
            bail!("Passkey not found");
        }
        let now = timestamp(Utc::now());
        invalidate_pending_recovery_responses(&mut state, account_id, &now);
        state.passkeys.remove(credential_id);
        self.write_state(&state).await
    }

    pub async fn start_totp_enrollment(&self, account_id: Uuid) -> Result<serde_json::Value> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        let account = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .ok_or_else(|| anyhow!("account is not active"))?;
        let secret = random_bytes(20)?;
        let encoded = BASE32_NOPAD.encode(&secret);
        let encrypted_secret = encrypt_recovery_secret(&self.encryption_key, &secret)?;
        state.pending_totp_enrollments.insert(
            account_id,
            PendingTotpEnrollment {
                encrypted_secret,
                expires_at: timestamp(
                    Utc::now() + Duration::minutes(TOTP_ENROLLMENT_LIFETIME_MINUTES),
                ),
                credential_generation: account.credential_generation,
            },
        );
        let label = format!("Ugoite:{}", account.account_id);
        let uri = format!(
            "otpauth://totp/{label}?secret={encoded}&issuer=Ugoite&algorithm=SHA256&digits=6&period=30"
        );
        self.write_state(&state).await?;
        Ok(serde_json::json!({"secret": encoded, "otpauth_uri": uri}))
    }

    pub async fn finish_totp_enrollment(
        &self,
        account_id: Uuid,
        code: &str,
    ) -> std::result::Result<(), TotpEnrollmentFinishError> {
        let _guard = self.state_lock.lock().await;
        let mut state = self
            .read_state()
            .await
            .map_err(TotpEnrollmentFinishError::Internal)?;
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)
            .map_err(TotpEnrollmentFinishError::Internal)?;
        let pending = state
            .pending_totp_enrollments
            .get(&account_id)
            .cloned()
            .ok_or(TotpEnrollmentFinishError::InvalidOrExpired)?;
        let expires_at =
            parse_timestamp(&pending.expires_at).map_err(TotpEnrollmentFinishError::Internal)?;
        if expires_at <= Utc::now() {
            return Err(TotpEnrollmentFinishError::InvalidOrExpired);
        }
        if state
            .accounts
            .get(&account_id)
            .is_none_or(|account| account.credential_generation != pending.credential_generation)
        {
            return Err(TotpEnrollmentFinishError::InvalidOrExpired);
        }
        let secret = decrypt_recovery_secret(&self.encryption_key, &pending.encrypted_secret)
            .map_err(TotpEnrollmentFinishError::Internal)?;
        if !verify_totp(&secret, code, Utc::now().timestamp())
            .map_err(TotpEnrollmentFinishError::Internal)?
        {
            return Err(TotpEnrollmentFinishError::InvalidOrExpired);
        }
        let now = timestamp(Utc::now());
        invalidate_pending_recovery_responses(&mut state, account_id, &now);
        state.pending_totp_enrollments.remove(&account_id);
        let recovery = state.recovery.get_mut(&account_id).ok_or_else(|| {
            TotpEnrollmentFinishError::Internal(anyhow!("recovery record not found"))
        })?;
        recovery.totp_secret_encrypted = Some(pending.encrypted_secret);
        if !matches!(state.lifecycle, NodeLifecycle::Active)
            && state
                .accounts
                .get(&account_id)
                .is_some_and(|account| account.node_roles.contains(&NodeRole::NodeAdmin))
            && !recovery.code_hashes.is_empty()
        {
            state.lifecycle = NodeLifecycle::Active;
        }
        self.write_state(&state)
            .await
            .map_err(TotpEnrollmentFinishError::Internal)
    }

    pub async fn start_recovery_registration(
        &self,
        account_id: Uuid,
        recovery_code: &str,
        totp_code: &str,
    ) -> Result<RegistrationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        let account = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .cloned()
            .ok_or_else(|| anyhow!("recovery credentials are invalid"))?;
        let recovery = state
            .recovery
            .get(&account_id)
            .cloned()
            .ok_or_else(|| anyhow!("recovery credentials are invalid"))?;
        if recovery
            .locked_until
            .as_deref()
            .is_some_and(|locked_until| {
                DateTime::parse_from_rfc3339(locked_until)
                    .map(|value| value.with_timezone(&Utc) > Utc::now())
                    .unwrap_or(true)
            })
        {
            bail!("recovery credentials are temporarily locked");
        }
        let code_hash = token_hash(&recovery_code.trim().to_uppercase());
        let code_index = recovery
            .code_hashes
            .iter()
            .position(|candidate| candidate == &code_hash);
        let encrypted_secret = recovery
            .totp_secret_encrypted
            .as_deref()
            .and_then(|encrypted| decrypt_recovery_secret(&self.encryption_key, encrypted).ok());
        let valid_totp = encrypted_secret.as_deref().is_some_and(|secret| {
            verify_totp(secret, totp_code, Utc::now().timestamp()).unwrap_or(false)
        });
        if code_index.is_none() || !valid_totp {
            let recovery = state
                .recovery
                .get_mut(&account_id)
                .ok_or_else(|| anyhow!("recovery credentials are invalid"))?;
            recovery.failed_attempts = recovery.failed_attempts.saturating_add(1);
            if recovery.failed_attempts >= 5 {
                recovery.locked_until = Some(timestamp(Utc::now() + Duration::minutes(15)));
                recovery.failed_attempts = 0;
            }
            self.write_state(&state).await?;
            bail!("recovery credentials are invalid");
        }
        let recovery = state
            .recovery
            .get_mut(&account_id)
            .ok_or_else(|| anyhow!("recovery credentials are invalid"))?;
        recovery.failed_attempts = 0;
        recovery.locked_until = None;
        recovery
            .code_hashes
            .remove(code_index.expect("validated above"));

        let (mut public_key, registration) = self.webauthn.start_passkey_registration(
            // Recovery must add a credential without overwriting a surviving passkey on the
            // same authenticator. The account association remains in RegistrationChallenge.
            Uuid::now_v7(),
            &account_id.to_string(),
            &account.display_name,
            None,
        )?;
        if let Some(selection) = public_key.public_key.authenticator_selection.as_mut() {
            selection.resident_key = Some(serde_json::from_value(serde_json::json!("required"))?);
            selection.require_resident_key = true;
        }
        let challenge_id = Uuid::now_v7();
        state.registration_challenges.insert(
            challenge_id,
            RegistrationChallenge {
                account_id,
                credential_generation: state
                    .accounts
                    .get(&account_id)
                    .map(|account| account.credential_generation)
                    .unwrap_or_default(),
                display_name: account.display_name,
                state: registration,
                public_key: Some(public_key.clone()),
                purpose: RegistrationPurpose::Recovery,
                expires_at: timestamp(Utc::now() + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
            },
        );
        self.write_state(&state).await?;
        Ok(RegistrationStart {
            challenge_id,
            public_key,
        })
    }

    pub async fn finish_recovery_registration(
        &self,
        account_id: Uuid,
        challenge_id: Uuid,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<RecoveryRegistrationFinish> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let challenge = state
            .registration_challenges
            .remove(&challenge_id)
            .ok_or_else(|| anyhow!("unknown or consumed recovery challenge"))?;
        validate_expiry(&challenge.expires_at, "recovery challenge")?;
        if challenge.account_id != account_id
            || !matches!(challenge.purpose, RegistrationPurpose::Recovery)
        {
            bail!("recovery challenge has the wrong account or purpose");
        }
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        if state
            .accounts
            .get(&account_id)
            .is_none_or(|account| account.credential_generation != challenge.credential_generation)
        {
            bail!("recovery challenge is stale");
        }
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &challenge.state)
            .context("verify recovery Passkey registration")?;
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        if state.passkeys.contains_key(&credential_id) {
            bail!("credential is already registered");
        }
        let now = timestamp(Utc::now());
        invalidate_pending_recovery_responses(&mut state, account_id, &now);
        let method_id = Uuid::now_v7();
        state.passkeys.insert(
            credential_id.clone(),
            StoredPasskey {
                credential_id,
                account_id,
                method_id,
                passkey,
                created_at: now.clone(),
                last_used_at: Some(now.clone()),
                rp_id: self.rp_id.clone(),
            },
        );
        state.authentication_methods.insert(
            method_id,
            AuthenticationMethod {
                method_id,
                account_id,
                kind: AuthenticationMethodKind::Passkey,
                external_subject: None,
                created_at: now.clone(),
                last_used_at: Some(now),
            },
        );
        let account = state
            .accounts
            .get(&account_id)
            .cloned()
            .ok_or_else(|| anyhow!("account not found"))?;
        let recovery_codes = (0..8)
            .map(|_| random_recovery_code())
            .collect::<Result<Vec<_>>>()?;
        let recovery = state
            .recovery
            .get_mut(&account_id)
            .ok_or_else(|| anyhow!("recovery record not found"))?;
        recovery.code_hashes = recovery_codes.iter().map(|code| token_hash(code)).collect();
        recovery.failed_attempts = 0;
        recovery.locked_until = None;
        let session_id = self
            .create_session(
                &state,
                account_id,
                method_id,
                AssuranceLevel::PhishingResistant,
            )
            .await?;
        self.write_state(&state).await?;
        Ok(RecoveryRegistrationFinish {
            account,
            session_id,
            recovery_codes,
            recovery_space_uid: None,
            recovery_principal_id: None,
            recovery_issuer_principal_id: None,
            recovery_issuer_account_id: None,
            recovery_issuer_credential_id: None,
            recovery_request_id: None,
        })
    }

    /// Issue a short-lived, tuple-bound approval for a forced recovery. The
    /// bearer hash is authoritative, while the encrypted bearer supports a
    /// retry with the same request id when the first response outcome was
    /// ambiguous. A different request id still supersedes the approval.
    pub async fn supersede_owner_recovery_approvals(
        &self,
        account_id: Uuid,
    ) -> Result<Vec<(Uuid, Uuid)>> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let now = timestamp(Utc::now());
        let approvals = state
            .owner_recovery_approvals
            .values()
            .filter(|approval| {
                approval.account_id == account_id
                    && approval.used_at.is_none()
                    && approval.invalidated_at.is_none()
            })
            .cloned()
            .collect::<Vec<_>>();
        let had_approvals = !approvals.is_empty();
        let mut fences = Vec::new();
        for approval in approvals {
            if let Some(challenge_id) = approval.challenge_id {
                state.registration_challenges.remove(&challenge_id);
                state.recovery_challenge_tombstones.insert(
                    challenge_id,
                    RecoveryChallengeTombstone {
                        challenge_id,
                        approval_id: approval.approval_id,
                        reset_id: approval.reset_id.unwrap_or_else(Uuid::now_v7),
                        reason: "superseded".to_string(),
                        created_at: now.clone(),
                    },
                );
            }
            if let Some(approval_mut) = state
                .owner_recovery_approvals
                .get_mut(&approval.approval_id)
            {
                approval_mut.invalidated_at = Some(now.clone());
                approval_mut.challenge_id = None;
                approval_mut.reset_id = None;
            }
            if let Some(fence_id) = approval.recovery_fence_id {
                release_node_recovery_fence(&mut state, Some(fence_id), "superseded");
                fences.push((approval.space_uid, fence_id));
            }
        }
        if had_approvals {
            self.write_state(&state).await?;
        }
        Ok(fences)
    }

    pub async fn owner_recovery_approval(
        &self,
        approval_id: Uuid,
    ) -> Result<Option<OwnerRecoveryApproval>> {
        Ok(self
            .read_state()
            .await?
            .owner_recovery_approvals
            .get(&approval_id)
            .cloned())
    }

    /// Mark an expired approval terminal and release its Node-side fence.
    /// The server releases the paired Space fence after this CAS succeeds.
    pub async fn expire_owner_recovery_approval(
        &self,
        approval_id: Uuid,
    ) -> Result<Option<(Uuid, Uuid)>> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let Some(approval) = state.owner_recovery_approvals.get(&approval_id).cloned() else {
            return Ok(None);
        };
        if approval.used_at.is_some() {
            return Ok(None);
        }
        let expired = parse_timestamp(&approval.expires_at)? <= Utc::now();
        if approval.invalidated_at.is_none() && !expired {
            return Ok(None);
        }
        let now = timestamp(Utc::now());
        let approval_mut = state
            .owner_recovery_approvals
            .get_mut(&approval_id)
            .expect("approval was checked above");
        if approval_mut.invalidated_at.is_none() {
            approval_mut.invalidated_at = Some(now);
            approval_mut.challenge_id = None;
            approval_mut.reset_id = None;
        }
        if let Some(fence_id) = approval.recovery_fence_id {
            if let Some(fence) = state.node_recovery_fences.get_mut(&fence_id) {
                if fence.status == "active" {
                    fence.status = "released".to_string();
                }
            }
        }
        self.write_state(&state).await?;
        Ok(approval
            .recovery_fence_id
            .map(|fence_id| (approval.space_uid, fence_id)))
    }

    pub async fn owner_recovery_approval_token(&self, approval_id: Uuid) -> Result<String> {
        let state = self.read_state().await?;
        let approval = state
            .owner_recovery_approvals
            .get(&approval_id)
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        if approval.invalidated_at.is_some() || approval.used_at.is_some() {
            bail!("owner approval is no longer current");
        }
        validate_expiry(&approval.expires_at, "owner approval")?;
        let encrypted_token = approval
            .encrypted_token
            .as_deref()
            .ok_or_else(|| anyhow!("owner approval response is unavailable"))?;
        let token: String = serde_json::from_slice(&decrypt_recovery_secret(
            &self.encryption_key,
            encrypted_token,
        )?)
        .context("decode owner approval response")?;
        if token_hash(&token) != approval.token_hash {
            bail!("owner approval response is invalid");
        }
        Ok(token)
    }

    pub async fn issue_owner_recovery_approval_with_snapshot_and_credential(
        &self,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        snapshot: RecoveryBindingSnapshot,
        issuer_credential_id: Option<Uuid>,
    ) -> Result<(Uuid, String, String)> {
        self.issue_owner_recovery_approval_unchecked(
            space_uid,
            principal_id,
            account_id,
            issuer_principal_id,
            issuer_account_id,
            Some(snapshot),
            issuer_credential_id,
            None,
        )
        .await
    }

    pub async fn issue_owner_recovery_approval_with_snapshot_credential_and_session(
        &self,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        snapshot: RecoveryBindingSnapshot,
        issuer_credential_id: Option<Uuid>,
        session_token: &str,
    ) -> Result<(Uuid, String, String)> {
        self.issue_owner_recovery_approval_unchecked(
            space_uid,
            principal_id,
            account_id,
            issuer_principal_id,
            issuer_account_id,
            Some(snapshot),
            issuer_credential_id,
            Some(session_token),
        )
        .await
    }

    async fn issue_owner_recovery_approval_unchecked(
        &self,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        snapshot: Option<RecoveryBindingSnapshot>,
        issuer_credential_id: Option<Uuid>,
        session_token: Option<&str>,
    ) -> Result<(Uuid, String, String)> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if let Some(session_token) = session_token {
            self.validate_recent_passkey_session_state(
                &state,
                session_token,
                issuer_account_id,
                issuer_credential_id
                    .ok_or_else(|| anyhow!("owner recovery credential is missing"))?,
                state
                    .accounts
                    .get(&issuer_account_id)
                    .map(|account| account.credential_generation)
                    .unwrap_or_default(),
            )
            .await?;
        }
        if account_id == issuer_account_id || principal_id == issuer_principal_id {
            bail!("owner cannot approve their own recovery");
        }
        if !state
            .accounts
            .get(&issuer_account_id)
            .is_some_and(|account| matches!(account.status, AccountStatus::Active))
        {
            bail!("recovery issuer is inactive");
        }
        let target = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .ok_or_else(|| anyhow!("recovery target is invalid"))?;
        let target_generation = target.credential_generation;
        let target_node_lifecycle_epoch = state
            .account_lifecycle_epochs
            .get(&account_id)
            .copied()
            .unwrap_or_default();
        let issuer_node_lifecycle_epoch = state
            .account_lifecycle_epochs
            .get(&issuer_account_id)
            .copied()
            .unwrap_or_default();
        let issuer_generation = state
            .accounts
            .get(&issuer_account_id)
            .map(|account| account.credential_generation)
            .unwrap_or_default();
        if let Some(snapshot) = &snapshot {
            if snapshot.issuer_node_lifecycle_epoch != issuer_node_lifecycle_epoch
                || snapshot.target_node_lifecycle_epoch != target_node_lifecycle_epoch
                || snapshot.issuer_generation != issuer_generation
                || snapshot.target_generation != target_generation
            {
                bail!("recovery tuple is stale")
            }
        }
        let target_bindings = state
            .bindings
            .iter()
            .filter(|binding| {
                binding.space_uid == space_uid
                    && binding.principal_id == principal_id
                    && binding.node_account_id == account_id
            })
            .count();
        if target_bindings != 1 {
            bail!("recovery target binding is not unique");
        }
        let issuer_bindings = state
            .bindings
            .iter()
            .filter(|binding| {
                binding.space_uid == space_uid
                    && binding.principal_id == issuer_principal_id
                    && binding.node_account_id == issuer_account_id
            })
            .count();
        if issuer_bindings != 1 {
            bail!("recovery issuer binding is not unique");
        }
        let now = Utc::now();
        let token = random_token(32)?;
        let encrypted_token =
            encrypt_recovery_secret(&self.encryption_key, &serde_json::to_vec(&token)?)?;
        let approval_id = snapshot
            .as_ref()
            .map(|snapshot| snapshot.request_id)
            .unwrap_or_else(Uuid::now_v7);
        let expires_at = timestamp(now + Duration::minutes(15));
        let superseded_challenges = state
            .owner_recovery_approvals
            .values()
            .filter(|approval| approval.account_id == account_id && approval.used_at.is_none())
            .filter_map(|approval| {
                Some((
                    approval.challenge_id?,
                    approval.approval_id,
                    approval.reset_id?,
                ))
            })
            .collect::<Vec<_>>();
        let superseded_fence_ids = state
            .owner_recovery_approvals
            .values()
            .filter(|approval| approval.account_id == account_id && approval.used_at.is_none())
            .filter_map(|approval| approval.recovery_fence_id)
            .collect::<Vec<_>>();
        for approval in state
            .owner_recovery_approvals
            .values_mut()
            .filter(|approval| approval.account_id == account_id && approval.used_at.is_none())
        {
            approval.invalidated_at = Some(timestamp(now));
        }
        for fence_id in superseded_fence_ids {
            release_node_recovery_fence(&mut state, Some(fence_id), "superseded");
        }
        for (challenge_id, approval_id, reset_id) in superseded_challenges {
            state.registration_challenges.remove(&challenge_id);
            state.recovery_challenge_tombstones.insert(
                challenge_id,
                RecoveryChallengeTombstone {
                    challenge_id,
                    approval_id,
                    reset_id,
                    reason: "superseded".to_string(),
                    created_at: timestamp(now),
                },
            );
        }
        state.owner_recovery_approvals.insert(
            approval_id,
            OwnerRecoveryApproval {
                approval_id,
                token_hash: token_hash(&token),
                space_uid,
                principal_id,
                account_id,
                issuer_principal_id,
                issuer_account_id,
                issuer_credential_id,
                target_generation,
                issuer_generation,
                issuer_space_lifecycle_epoch: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.issuer_space_lifecycle_epoch)
                    .unwrap_or_default(),
                target_space_lifecycle_epoch: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.target_space_lifecycle_epoch)
                    .unwrap_or_default(),
                issuer_node_lifecycle_epoch,
                target_node_lifecycle_epoch,
                space_authorization_revision: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.space_authorization_revision)
                    .unwrap_or_default(),
                recovery_fence_id: snapshot.as_ref().map(|snapshot| snapshot.recovery_fence_id),
                issued_at: timestamp(now),
                expires_at: expires_at.clone(),
                challenge_id: None,
                reset_id: None,
                used_at: None,
                invalidated_at: None,
                encrypted_token: Some(encrypted_token),
            },
        );
        queue_recovery_audit(
            &mut state,
            approval_id,
            "recovery.owner_approval_issued",
            approval_id,
            None,
            space_uid,
            principal_id,
            account_id,
            Some(issuer_principal_id),
            Some(issuer_account_id),
            issuer_credential_id,
            Some(issuer_principal_id),
            Some(issuer_account_id),
            issuer_credential_id,
            serde_json::json!({
                "space_uid": space_uid,
                "principal_id": principal_id,
                "issuer_principal_id": issuer_principal_id
            }),
        );
        if let Err(error) = self.write_state(&state).await {
            let committed = self.read_state().await.ok().is_some_and(|observed| {
                observed
                    .owner_recovery_approvals
                    .get(&approval_id)
                    .is_some_and(|approval| approval.token_hash == token_hash(&token))
            });
            if !committed {
                return Err(error);
            }
        }
        Ok((approval_id, token, expires_at))
    }

    pub async fn owner_recovery_approval_context(
        &self,
        token: &str,
    ) -> Result<OwnerRecoveryContext> {
        let state = self.read_state().await?;
        let approval = state
            .owner_recovery_approvals
            .values()
            .find(|approval| approval.token_hash == token_hash(token.trim()))
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        if approval.invalidated_at.is_some() || approval.used_at.is_some() {
            bail!("owner approval is invalid");
        }
        validate_expiry(&approval.expires_at, "owner approval")?;
        Ok(OwnerRecoveryContext {
            space_uid: approval.space_uid,
            principal_id: approval.principal_id,
            account_id: approval.account_id,
            issuer_principal_id: approval.issuer_principal_id,
            issuer_account_id: approval.issuer_account_id,
            target_generation: approval.target_generation,
            issuer_generation: approval.issuer_generation,
            issuer_space_lifecycle_epoch: approval.issuer_space_lifecycle_epoch,
            target_space_lifecycle_epoch: approval.target_space_lifecycle_epoch,
            issuer_node_lifecycle_epoch: approval.issuer_node_lifecycle_epoch,
            target_node_lifecycle_epoch: approval.target_node_lifecycle_epoch,
            space_authorization_revision: approval.space_authorization_revision,
            recovery_fence_id: approval.recovery_fence_id,
        })
    }

    /// Return the paired fence coordinates for an approval that is already
    /// terminal without mutating either store. The server uses this before
    /// mapping an expired/invalidated bearer so it can abort the Space half
    /// together with the Node half.
    pub async fn owner_recovery_abort_fence_for_token(
        &self,
        token: &str,
    ) -> Result<Option<(Uuid, Uuid)>> {
        let state = self.read_state().await?;
        let Some(approval) = state
            .owner_recovery_approvals
            .values()
            .find(|approval| approval.token_hash == token_hash(token.trim()))
        else {
            return Ok(None);
        };
        if approval.used_at.is_some() {
            return Ok(None);
        }
        let approval_expired = parse_timestamp(&approval.expires_at)? <= Utc::now();
        if approval.invalidated_at.is_some() || approval_expired {
            return Ok(approval
                .recovery_fence_id
                .map(|fence_id| (approval.space_uid, fence_id)));
        }
        Ok(None)
    }

    pub async fn owner_recovery_abort_fence_for_challenge(
        &self,
        challenge_id: Uuid,
    ) -> Result<Option<(Uuid, Uuid)>> {
        let state = self.read_state().await?;
        let (approval_id, terminal) = if let Some(challenge) =
            state.registration_challenges.get(&challenge_id)
        {
            let RegistrationPurpose::OwnerRecovery { approval_id, .. } = &challenge.purpose else {
                return Ok(None);
            };
            parse_timestamp(&challenge.expires_at)?;
            (*approval_id, false)
        } else if let Some(tombstone) = state.recovery_challenge_tombstones.get(&challenge_id) {
            (
                tombstone.approval_id,
                matches!(tombstone.reason.as_str(), "superseded" | "account_reset"),
            )
        } else {
            return Ok(None);
        };
        let Some(approval) = state.owner_recovery_approvals.get(&approval_id) else {
            return Ok(None);
        };
        if approval.challenge_id != Some(challenge_id) {
            return Ok(None);
        }
        let approval_invalidated = approval.invalidated_at.is_some()
            || parse_timestamp(&approval.expires_at)? <= Utc::now();
        if approval.used_at.is_some() || (!terminal && !approval_invalidated) {
            return Ok(None);
        }
        Ok(approval
            .recovery_fence_id
            .map(|fence_id| (approval.space_uid, fence_id)))
    }

    pub async fn owner_recovery_challenge_context(
        &self,
        challenge_id: Uuid,
    ) -> Result<OwnerRecoveryContext> {
        let mut state = self.read_state().await?;
        let challenge = match state.registration_challenges.get(&challenge_id) {
            Some(challenge) => challenge,
            None => {
                if let Some(tombstone) = state.recovery_challenge_tombstones.get(&challenge_id) {
                    if let Some(marker) = state.recovery_reset_markers.get(&tombstone.reset_id) {
                        if marker.space_fence_status != "reconciled" {
                            bail!("RECOVERY_FENCE_UNAVAILABLE");
                        }
                        bail!("owner reset already completed");
                    }
                    if matches!(
                        tombstone.reason.as_str(),
                        "expired" | "superseded" | "account_reset"
                    ) {
                        bail!("owner recovery challenge expired");
                    }
                }
                bail!("owner recovery challenge is invalid");
            }
        };
        let RegistrationPurpose::OwnerRecovery { approval_id, .. } = &challenge.purpose else {
            bail!("recovery challenge has the wrong purpose");
        };
        let approval = state
            .owner_recovery_approvals
            .get(approval_id)
            .filter(|approval| {
                approval.challenge_id == Some(challenge_id)
                    && approval.used_at.is_none()
                    && approval.invalidated_at.is_none()
            })
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        validate_expiry(&approval.expires_at, "owner approval")?;
        if parse_timestamp(&challenge.expires_at)? <= Utc::now() {
            if let RegistrationPurpose::OwnerRecovery {
                approval_id,
                reset_id,
                ..
            } = challenge.purpose.clone()
            {
                let now = timestamp(Utc::now());
                state.registration_challenges.remove(&challenge_id);
                state.recovery_challenge_tombstones.insert(
                    challenge_id,
                    RecoveryChallengeTombstone {
                        challenge_id,
                        approval_id,
                        reset_id,
                        reason: "expired".to_string(),
                        created_at: now.clone(),
                    },
                );
                if let Some(approval) = state.owner_recovery_approvals.get_mut(&approval_id) {
                    approval.challenge_id = None;
                    approval.reset_id = None;
                }
                self.write_state(&state).await?;
            }
            bail!("owner recovery challenge expired");
        }
        Ok(OwnerRecoveryContext {
            space_uid: approval.space_uid,
            principal_id: approval.principal_id,
            account_id: approval.account_id,
            issuer_principal_id: approval.issuer_principal_id,
            issuer_account_id: approval.issuer_account_id,
            target_generation: approval.target_generation,
            issuer_generation: approval.issuer_generation,
            issuer_space_lifecycle_epoch: approval.issuer_space_lifecycle_epoch,
            target_space_lifecycle_epoch: approval.target_space_lifecycle_epoch,
            issuer_node_lifecycle_epoch: approval.issuer_node_lifecycle_epoch,
            target_node_lifecycle_epoch: approval.target_node_lifecycle_epoch,
            space_authorization_revision: approval.space_authorization_revision,
            recovery_fence_id: approval.recovery_fence_id,
        })
    }

    pub async fn start_owner_recovery_registration(
        &self,
        token: &str,
    ) -> Result<RegistrationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let now = Utc::now();
        let approval_id = state
            .owner_recovery_approvals
            .values()
            .find(|approval| approval.token_hash == token_hash(token.trim()))
            .map(|approval| approval.approval_id)
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        let approval = state
            .owner_recovery_approvals
            .get(&approval_id)
            .cloned()
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        if approval.invalidated_at.is_some() || approval.used_at.is_some() {
            bail!("owner approval is invalid");
        }
        validate_expiry(&approval.expires_at, "owner approval")?;
        if let Some(challenge_id) = approval.challenge_id {
            let pending = state
                .registration_challenges
                .get(&challenge_id)
                .map(|challenge| {
                    parse_timestamp(&challenge.expires_at).map(|expires_at| expires_at > now)
                })
                .transpose()?
                .unwrap_or(false);
            if pending {
                let challenge = state
                    .registration_challenges
                    .get(&challenge_id)
                    .cloned()
                    .ok_or_else(|| anyhow!("owner recovery challenge is unavailable"))?;
                if let Some(public_key) = challenge.public_key.clone() {
                    return Ok(RegistrationStart {
                        challenge_id,
                        public_key,
                    });
                }

                // Older v0.1 pending challenges did not persist the browser
                // response. Retire that challenge and issue a replacement
                // bound to the same approval/fence; the new public response
                // is persisted so a retry after an ambiguous write can
                // converge through the normal pending-challenge path.
                let RegistrationPurpose::OwnerRecovery {
                    approval_id,
                    reset_id,
                    space_uid,
                    principal_id,
                } = challenge.purpose.clone()
                else {
                    bail!("owner recovery challenge has the wrong purpose");
                };
                let (mut public_key, registration) = self.webauthn.start_passkey_registration(
                    Uuid::now_v7(),
                    &challenge.account_id.to_string(),
                    &challenge.display_name,
                    None,
                )?;
                if let Some(selection) = public_key.public_key.authenticator_selection.as_mut() {
                    selection.resident_key =
                        Some(serde_json::from_value(serde_json::json!("required"))?);
                    selection.require_resident_key = true;
                }
                let replacement_id = Uuid::now_v7();
                state.registration_challenges.remove(&challenge_id);
                state.recovery_challenge_tombstones.insert(
                    challenge_id,
                    RecoveryChallengeTombstone {
                        challenge_id,
                        approval_id,
                        reset_id,
                        reason: "superseded".to_string(),
                        created_at: timestamp(now),
                    },
                );
                state.registration_challenges.insert(
                    replacement_id,
                    RegistrationChallenge {
                        account_id: challenge.account_id,
                        credential_generation: challenge.credential_generation,
                        display_name: challenge.display_name,
                        state: registration,
                        public_key: Some(public_key.clone()),
                        purpose: RegistrationPurpose::OwnerRecovery {
                            approval_id,
                            reset_id,
                            space_uid,
                            principal_id,
                        },
                        expires_at: timestamp(now + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
                    },
                );
                state
                    .owner_recovery_approvals
                    .get_mut(&approval_id)
                    .ok_or_else(|| anyhow!("owner approval is invalid"))?
                    .challenge_id = Some(replacement_id);
                self.write_state(&state).await?;
                return Ok(RegistrationStart {
                    challenge_id: replacement_id,
                    public_key,
                });
            }
            state.registration_challenges.remove(&challenge_id);
            state.recovery_challenge_tombstones.insert(
                challenge_id,
                RecoveryChallengeTombstone {
                    challenge_id,
                    approval_id,
                    reset_id: approval.reset_id.unwrap_or_else(Uuid::now_v7),
                    reason: "expired".to_string(),
                    created_at: timestamp(now),
                },
            );
            let approval_mut = state
                .owner_recovery_approvals
                .get_mut(&approval_id)
                .ok_or_else(|| anyhow!("owner approval is invalid"))?;
            approval_mut.challenge_id = None;
            approval_mut.reset_id = None;
        }
        let account = state
            .accounts
            .get(&approval.account_id)
            .filter(|account| {
                matches!(account.status, AccountStatus::Active)
                    && account.credential_generation == approval.target_generation
            })
            .cloned()
            .ok_or_else(|| anyhow!("owner approval is stale"))?;
        if state
            .bindings
            .iter()
            .filter(|binding| {
                binding.space_uid == approval.space_uid
                    && binding.principal_id == approval.principal_id
                    && binding.node_account_id == approval.account_id
            })
            .count()
            != 1
        {
            bail!("owner approval binding is stale");
        }
        if !state
            .accounts
            .get(&approval.issuer_account_id)
            .is_some_and(|account| matches!(account.status, AccountStatus::Active))
            || state
                .bindings
                .iter()
                .filter(|binding| {
                    binding.space_uid == approval.space_uid
                        && binding.principal_id == approval.issuer_principal_id
                        && binding.node_account_id == approval.issuer_account_id
                })
                .count()
                != 1
        {
            bail!("owner approval issuer is stale");
        }
        let recovery_snapshot =
            approval
                .recovery_fence_id
                .map(|recovery_fence_id| RecoveryBindingSnapshot {
                    request_id: approval.approval_id,
                    recovery_fence_id,
                    recovery_fence_expires_at: approval.expires_at.clone(),
                    space_authorization_revision: approval.space_authorization_revision,
                    issuer_space_lifecycle_epoch: approval.issuer_space_lifecycle_epoch,
                    target_space_lifecycle_epoch: approval.target_space_lifecycle_epoch,
                    issuer_node_lifecycle_epoch: approval.issuer_node_lifecycle_epoch,
                    target_node_lifecycle_epoch: approval.target_node_lifecycle_epoch,
                    issuer_generation: approval.issuer_generation,
                    target_generation: approval.target_generation,
                });
        if let Some(recovery_snapshot) = recovery_snapshot.as_ref() {
            acquire_node_recovery_fence(
                &mut state,
                approval.space_uid,
                approval.principal_id,
                approval.account_id,
                approval.issuer_account_id,
                recovery_snapshot,
            )?;
        }
        let (mut public_key, registration) = self.webauthn.start_passkey_registration(
            Uuid::now_v7(),
            &approval.account_id.to_string(),
            &account.display_name,
            None,
        )?;
        if let Some(selection) = public_key.public_key.authenticator_selection.as_mut() {
            selection.resident_key = Some(serde_json::from_value(serde_json::json!("required"))?);
            selection.require_resident_key = true;
        }
        let challenge_id = Uuid::now_v7();
        let reset_id = Uuid::now_v7();
        state.registration_challenges.insert(
            challenge_id,
            RegistrationChallenge {
                account_id: approval.account_id,
                credential_generation: approval.target_generation,
                display_name: account.display_name,
                state: registration,
                public_key: Some(public_key.clone()),
                purpose: RegistrationPurpose::OwnerRecovery {
                    approval_id,
                    reset_id,
                    space_uid: approval.space_uid,
                    principal_id: approval.principal_id,
                },
                expires_at: timestamp(now + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
            },
        );
        let approval = state
            .owner_recovery_approvals
            .get_mut(&approval_id)
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        approval.challenge_id = Some(challenge_id);
        approval.reset_id = Some(reset_id);
        self.write_state(&state).await?;
        Ok(RegistrationStart {
            challenge_id,
            public_key,
        })
    }

    pub async fn finish_owner_recovery_registration(
        &self,
        challenge_id: Uuid,
        credential: &RegisterPublicKeyCredential,
    ) -> Result<RecoveryRegistrationFinish> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let challenge = match state.registration_challenges.get(&challenge_id).cloned() {
            Some(challenge) => challenge,
            None => {
                if let Some(tombstone) = state.recovery_challenge_tombstones.get(&challenge_id) {
                    if let Some(marker) = state.recovery_reset_markers.get(&tombstone.reset_id) {
                        if marker.space_fence_status != "reconciled" {
                            bail!("RECOVERY_FENCE_UNAVAILABLE");
                        }
                        bail!("owner reset already completed");
                    }
                    bail!("owner recovery challenge expired");
                }
                bail!("unknown or consumed owner recovery challenge");
            }
        };
        if parse_timestamp(&challenge.expires_at)? <= Utc::now() {
            if let RegistrationPurpose::OwnerRecovery {
                approval_id,
                reset_id,
                ..
            } = challenge.purpose.clone()
            {
                let now = timestamp(Utc::now());
                state.registration_challenges.remove(&challenge_id);
                state.recovery_challenge_tombstones.insert(
                    challenge_id,
                    RecoveryChallengeTombstone {
                        challenge_id,
                        approval_id,
                        reset_id,
                        reason: "expired".to_string(),
                        created_at: now.clone(),
                    },
                );
                if let Some(approval) = state.owner_recovery_approvals.get_mut(&approval_id) {
                    approval.challenge_id = None;
                    approval.reset_id = None;
                }
                self.write_state(&state).await?;
            }
            bail!("owner recovery challenge expired");
        }
        let RegistrationPurpose::OwnerRecovery {
            approval_id,
            reset_id,
            space_uid,
            principal_id,
        } = challenge.purpose.clone()
        else {
            bail!("recovery challenge has the wrong purpose");
        };
        let approval = state
            .owner_recovery_approvals
            .get(&approval_id)
            .cloned()
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        validate_expiry(&approval.expires_at, "owner approval")?;
        if approval.challenge_id != Some(challenge_id)
            || approval.reset_id != Some(reset_id)
            || approval.used_at.is_some()
            || approval.invalidated_at.is_some()
        {
            if let Some(marker) = state.recovery_reset_markers.get(&reset_id) {
                if marker.space_fence_status != "reconciled" {
                    bail!("RECOVERY_FENCE_UNAVAILABLE");
                }
                bail!("owner reset already completed");
            }
            bail!("owner approval is invalid");
        }
        let account_before = state
            .accounts
            .get(&challenge.account_id)
            .cloned()
            .ok_or_else(|| anyhow!("recovery account is missing"))?;
        if account_before.credential_generation != approval.target_generation {
            bail!("owner approval is stale");
        }
        let target_node_lifecycle_epoch = state
            .account_lifecycle_epochs
            .get(&challenge.account_id)
            .copied()
            .unwrap_or_default();
        let issuer_node_lifecycle_epoch = state
            .account_lifecycle_epochs
            .get(&approval.issuer_account_id)
            .copied()
            .unwrap_or_default();
        if target_node_lifecycle_epoch != approval.target_node_lifecycle_epoch
            || issuer_node_lifecycle_epoch != approval.issuer_node_lifecycle_epoch
            || !state
                .accounts
                .get(&approval.issuer_account_id)
                .is_some_and(|account| matches!(account.status, AccountStatus::Active))
        {
            bail!("owner approval is stale");
        }
        let recovery_fence_id = approval
            .recovery_fence_id
            .ok_or_else(|| anyhow!("owner recovery fence is unavailable"))?;
        let fence = state
            .node_recovery_fences
            .get(&recovery_fence_id)
            .ok_or_else(|| anyhow!("RECOVERY_FENCE_UNAVAILABLE"))?;
        if !node_recovery_fence_is_active(fence)
            || parse_timestamp(&fence.expires_at)? <= Utc::now()
        {
            bail!("RECOVERY_FENCE_UNAVAILABLE");
        }
        acquire_node_recovery_fence(
            &mut state,
            space_uid,
            principal_id,
            challenge.account_id,
            approval.issuer_account_id,
            &RecoveryBindingSnapshot {
                request_id: approval.approval_id,
                recovery_fence_id,
                recovery_fence_expires_at: approval.expires_at.clone(),
                space_authorization_revision: approval.space_authorization_revision,
                issuer_space_lifecycle_epoch: approval.issuer_space_lifecycle_epoch,
                target_space_lifecycle_epoch: approval.target_space_lifecycle_epoch,
                issuer_node_lifecycle_epoch: approval.issuer_node_lifecycle_epoch,
                target_node_lifecycle_epoch: approval.target_node_lifecycle_epoch,
                issuer_generation: approval.issuer_generation,
                target_generation: approval.target_generation,
            },
        )?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &challenge.state)
            .context("verify owner recovery Passkey registration")?;
        let completion_proof_hash = token_hash(&serde_json::to_string(credential)?);
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        if state.passkeys.contains_key(&credential_id) {
            bail!("credential is already registered");
        }

        let now = timestamp(Utc::now());
        let superseded_owner_challenges = state
            .registration_challenges
            .iter()
            .filter_map(|(candidate_id, candidate)| {
                if *candidate_id == challenge_id || candidate.account_id != challenge.account_id {
                    return None;
                }
                let RegistrationPurpose::OwnerRecovery {
                    approval_id,
                    reset_id,
                    ..
                } = &candidate.purpose
                else {
                    return None;
                };
                Some((*candidate_id, *approval_id, *reset_id))
            })
            .collect::<Vec<_>>();
        let generation_after = account_before
            .credential_generation
            .checked_add(1)
            .ok_or_else(|| anyhow!("credential generation exhausted"))?;
        let method_id = Uuid::now_v7();
        let session_token = self
            .prepare_owner_reset(
                &mut state,
                &account_before,
                generation_after,
                method_id,
                reset_id,
            )
            .await?;
        state.passkeys.insert(
            credential_id.clone(),
            StoredPasskey {
                credential_id,
                account_id: challenge.account_id,
                method_id,
                passkey,
                created_at: now.clone(),
                last_used_at: Some(now.clone()),
                rp_id: self.rp_id.clone(),
            },
        );
        state.authentication_methods.insert(
            method_id,
            AuthenticationMethod {
                method_id,
                account_id: challenge.account_id,
                kind: AuthenticationMethodKind::Passkey,
                external_subject: None,
                created_at: now.clone(),
                last_used_at: Some(now.clone()),
            },
        );
        let recovery_codes = (0..8)
            .map(|_| random_recovery_code())
            .collect::<Result<Vec<_>>>()?;
        let recovery = state
            .recovery
            .entry(challenge.account_id)
            .or_insert_with(|| RecoveryRecord {
                account_id: challenge.account_id,
                code_hashes: Vec::new(),
                totp_secret_encrypted: None,
                created_at: now.clone(),
                failed_attempts: 0,
                locked_until: None,
            });
        recovery.code_hashes = recovery_codes.iter().map(|code| token_hash(code)).collect();
        recovery.totp_secret_encrypted = None;
        recovery.failed_attempts = 0;
        recovery.locked_until = None;
        let session_id = self.session_id_for_token(&session_token).await?;
        for (superseded_challenge_id, superseded_approval_id, superseded_reset_id) in
            superseded_owner_challenges
        {
            state.recovery_challenge_tombstones.insert(
                superseded_challenge_id,
                RecoveryChallengeTombstone {
                    challenge_id: superseded_challenge_id,
                    approval_id: superseded_approval_id,
                    reset_id: superseded_reset_id,
                    reason: "account_reset".to_string(),
                    created_at: now.clone(),
                },
            );
        }
        for approval in state
            .owner_recovery_approvals
            .values_mut()
            .filter(|approval| {
                approval.account_id == challenge.account_id
                    && approval.approval_id != approval_id
                    && approval.used_at.is_none()
            })
        {
            approval.invalidated_at = Some(now.clone());
        }
        *state
            .account_lifecycle_epochs
            .entry(challenge.account_id)
            .or_insert(0) += 1;
        state.accounts.insert(
            challenge.account_id,
            HumanAccount {
                credential_generation: generation_after,
                ..account_before.clone()
            },
        );
        state.registration_challenges.remove(&challenge_id);
        state.recovery_challenge_tombstones.insert(
            challenge_id,
            RecoveryChallengeTombstone {
                challenge_id,
                approval_id,
                reset_id,
                reason: "committed".to_string(),
                created_at: now.clone(),
            },
        );
        let approval_mut = state
            .owner_recovery_approvals
            .get_mut(&approval_id)
            .ok_or_else(|| anyhow!("owner approval is invalid"))?;
        approval_mut.used_at = Some(now.clone());
        let marker = RecoveryResetMarker {
            reset_id,
            challenge_id,
            approval_id,
            account_id: challenge.account_id,
            generation_before: account_before.credential_generation,
            generation_after,
            session_id,
            space_authorization_revision: approval.space_authorization_revision,
            recovery_fence_id: approval.recovery_fence_id.unwrap_or_default(),
            space_uid,
            principal_id,
            issuer_principal_id: approval.issuer_principal_id,
            space_fence_status: default_space_fence_status(),
            committed_at: now,
            encrypted_response: Some(encrypt_recovery_secret(
                &self.encryption_key,
                &serde_json::to_vec(&(session_token.clone(), recovery_codes.clone()))?,
            )?),
            response_delivered_at: None,
            response_delivery_id: None,
            response_invalidated_at: None,
            completion_proof_hash: Some(completion_proof_hash),
        };
        state.recovery_reset_markers.insert(reset_id, marker);
        queue_recovery_audit(
            &mut state,
            reset_id,
            "recovery.owner_reset_completed",
            reset_id,
            Some(challenge_id),
            space_uid,
            principal_id,
            challenge.account_id,
            None,
            None,
            None,
            Some(approval.issuer_principal_id),
            Some(approval.issuer_account_id),
            approval.issuer_credential_id,
            serde_json::json!({
                "credential_generation": generation_after
            }),
        );
        if let Err(error) = self.write_state(&state).await {
            let committed = self.read_state().await.ok().is_some_and(|observed| {
                observed
                    .recovery_reset_markers
                    .get(&reset_id)
                    .is_some_and(|marker| {
                        marker.session_id == session_id
                            && marker.generation_after == generation_after
                    })
            });
            if !committed {
                return Err(error);
            }
        }
        let _ = self
            .revoke_account_sessions_except(
                state.node_id,
                challenge.account_id,
                &timestamp(Utc::now()),
                Some(session_id),
            )
            .await;
        let account = state
            .accounts
            .get(&challenge.account_id)
            .cloned()
            .ok_or_else(|| anyhow!("recovery account is missing"))?;
        Ok(RecoveryRegistrationFinish {
            account,
            session_id: session_token,
            recovery_codes,
            recovery_space_uid: Some(space_uid),
            recovery_principal_id: Some(principal_id),
            recovery_issuer_principal_id: Some(approval.issuer_principal_id),
            recovery_issuer_account_id: Some(approval.issuer_account_id),
            recovery_issuer_credential_id: approval.issuer_credential_id,
            recovery_request_id: Some(reset_id),
        })
    }

    async fn prepare_owner_reset(
        &self,
        state: &mut NodeState,
        account: &HumanAccount,
        generation_after: u64,
        credential_id: Uuid,
        reset_id: Uuid,
    ) -> Result<String> {
        let now = timestamp(Utc::now());
        invalidate_pending_recovery_responses(state, account.account_id, &now);
        state
            .passkeys
            .retain(|_, passkey| passkey.account_id != account.account_id);
        state
            .authentication_methods
            .retain(|_, method| method.account_id != account.account_id);
        state.pending_totp_enrollments.remove(&account.account_id);
        state
            .registration_challenges
            .retain(|_, challenge| challenge.account_id != account.account_id);
        for credential in state.device_credentials.values_mut() {
            if credential.account_id == account.account_id {
                credential.revoked_at = Some(now.clone());
            }
        }
        for credential in state.refresh_credentials.values_mut() {
            if credential.account_id == account.account_id {
                credential.revoked_at = Some(now.clone());
            }
        }
        let invitation_accounts = state
            .invitations
            .values()
            .filter_map(|invitation| {
                invitation
                    .acceptance
                    .as_ref()
                    .map(|acceptance| (invitation.token_hash.clone(), acceptance.account_id()))
            })
            .collect::<BTreeMap<_, _>>();
        state.oidc_attempts.retain(|_, attempt| {
            attempt.link_account_id != Some(account.account_id)
                && attempt.invitation_account_id != Some(account.account_id)
                && !attempt
                    .invitation_hash
                    .as_ref()
                    .is_some_and(|hash| invitation_accounts.get(hash) == Some(&account.account_id))
        });
        state
            .authorization_codes
            .retain(|_, grant| grant.account_id != account.account_id);
        state
            .device_authorizations
            .retain(|_, request| request.approved_account_id != Some(account.account_id));
        state.accounts.insert(
            account.account_id,
            HumanAccount {
                credential_generation: generation_after,
                ..account.clone()
            },
        );
        self.create_session_with_recovery(
            state,
            account.account_id,
            credential_id,
            AssuranceLevel::PhishingResistant,
            Some(reset_id),
        )
        .await
    }

    async fn session_id_for_token(&self, token: &str) -> Result<Uuid> {
        let state = self.read_state().await?;
        let record = self
            .state_store
            .get(&session_key(state.node_id, &token_hash(token)))
            .await?
            .ok_or_else(|| anyhow!("new recovery session is missing"))?;
        Ok(serde_json::from_slice::<BrowserSession>(&record.value)?.session_id)
    }

    pub async fn rotate_recovery_codes_with_snapshot_and_credential(
        &self,
        request_id: Uuid,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        snapshot: RecoveryBindingSnapshot,
        issuer_credential_id: Option<Uuid>,
    ) -> Result<Vec<String>> {
        self.rotate_recovery_codes_unchecked(
            request_id,
            space_uid,
            principal_id,
            account_id,
            issuer_principal_id,
            issuer_account_id,
            Some(snapshot),
            issuer_credential_id,
            None,
        )
        .await
    }

    pub async fn rotate_recovery_codes_with_snapshot_credential_and_session(
        &self,
        request_id: Uuid,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        snapshot: RecoveryBindingSnapshot,
        issuer_credential_id: Option<Uuid>,
        session_token: &str,
    ) -> Result<Vec<String>> {
        self.rotate_recovery_codes_unchecked(
            request_id,
            space_uid,
            principal_id,
            account_id,
            issuer_principal_id,
            issuer_account_id,
            Some(snapshot),
            issuer_credential_id,
            Some(session_token),
        )
        .await
    }

    async fn rotate_recovery_codes_unchecked(
        &self,
        request_id: Uuid,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
        issuer_principal_id: Uuid,
        issuer_account_id: Uuid,
        snapshot: Option<RecoveryBindingSnapshot>,
        issuer_credential_id: Option<Uuid>,
        session_token: Option<&str>,
    ) -> Result<Vec<String>> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if let Some(session_token) = session_token {
            self.validate_recent_passkey_session_state(
                &state,
                session_token,
                issuer_account_id,
                issuer_credential_id
                    .ok_or_else(|| anyhow!("owner recovery credential is missing"))?,
                state
                    .accounts
                    .get(&issuer_account_id)
                    .map(|account| account.credential_generation)
                    .unwrap_or_default(),
            )
            .await?;
        }
        if let Some(existing) = state.backup_rotation_requests.get(&request_id) {
            if existing.space_uid != space_uid
                || existing.principal_id != principal_id
                || existing.account_id != account_id
                || existing.issuer_principal_id != issuer_principal_id
                || existing.issuer_account_id != issuer_account_id
                || existing.issuer_credential_id != issuer_credential_id
                || snapshot.as_ref().is_some_and(|snapshot| {
                    existing.recovery_fence_id != Some(snapshot.recovery_fence_id)
                        || existing.space_authorization_revision
                            != snapshot.space_authorization_revision
                        || existing.issuer_space_lifecycle_epoch
                            != snapshot.issuer_space_lifecycle_epoch
                        || existing.target_space_lifecycle_epoch
                            != snapshot.target_space_lifecycle_epoch
                        || existing.issuer_node_lifecycle_epoch
                            != snapshot.issuer_node_lifecycle_epoch
                        || existing.target_node_lifecycle_epoch
                            != snapshot.target_node_lifecycle_epoch
                        || existing.issuer_generation != snapshot.issuer_generation
                        || existing.target_generation != snapshot.target_generation
                })
            {
                bail!("backup rotation request key mismatch");
            }
            bail!("backup rotation already committed");
        }
        let account = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .cloned()
            .ok_or_else(|| anyhow!("recovery target is invalid"))?;
        let now = timestamp(Utc::now());
        invalidate_pending_recovery_responses(&mut state, account_id, &now);
        if !state
            .accounts
            .get(&issuer_account_id)
            .is_some_and(|account| matches!(account.status, AccountStatus::Active))
        {
            bail!("recovery issuer is inactive");
        }
        let target_node_lifecycle_epoch = state
            .account_lifecycle_epochs
            .get(&account_id)
            .copied()
            .unwrap_or_default();
        let issuer_node_lifecycle_epoch = state
            .account_lifecycle_epochs
            .get(&issuer_account_id)
            .copied()
            .unwrap_or_default();
        let issuer_generation = state
            .accounts
            .get(&issuer_account_id)
            .map(|account| account.credential_generation)
            .unwrap_or_default();
        if let Some(snapshot) = &snapshot {
            if snapshot.target_node_lifecycle_epoch != target_node_lifecycle_epoch
                || snapshot.issuer_node_lifecycle_epoch != issuer_node_lifecycle_epoch
                || snapshot.issuer_generation != issuer_generation
                || snapshot.target_generation != account.credential_generation
            {
                bail!("recovery tuple is stale");
            }
        }
        if state
            .bindings
            .iter()
            .filter(|binding| {
                binding.space_uid == space_uid
                    && binding.principal_id == principal_id
                    && binding.node_account_id == account_id
            })
            .count()
            != 1
        {
            bail!("recovery target binding is not unique");
        }
        if let Some(snapshot) = snapshot.as_ref() {
            acquire_node_recovery_fence(
                &mut state,
                space_uid,
                principal_id,
                account_id,
                issuer_account_id,
                snapshot,
            )?;
        }
        let codes = (0..8)
            .map(|_| random_recovery_code())
            .collect::<Result<Vec<_>>>()?;
        let code_hashes: Vec<String> = codes.iter().map(|code| token_hash(code)).collect();
        let recovery = state
            .recovery
            .entry(account_id)
            .or_insert_with(|| RecoveryRecord {
                account_id,
                code_hashes: Vec::new(),
                totp_secret_encrypted: None,
                created_at: timestamp(Utc::now()),
                failed_attempts: 0,
                locked_until: None,
            });
        recovery.code_hashes = code_hashes.clone();
        recovery.failed_attempts = 0;
        recovery.locked_until = None;
        state.registration_challenges.retain(|_, challenge| {
            challenge.account_id != account_id
                || !matches!(challenge.purpose, RegistrationPurpose::Recovery)
        });
        state.backup_rotation_requests.insert(
            request_id,
            BackupRotationRecord {
                request_id,
                space_uid,
                principal_id,
                account_id,
                issuer_principal_id,
                issuer_account_id,
                issuer_credential_id,
                target_generation: account.credential_generation,
                issuer_generation,
                issuer_space_lifecycle_epoch: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.issuer_space_lifecycle_epoch)
                    .unwrap_or_default(),
                target_space_lifecycle_epoch: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.target_space_lifecycle_epoch)
                    .unwrap_or_default(),
                issuer_node_lifecycle_epoch,
                target_node_lifecycle_epoch,
                space_authorization_revision: snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.space_authorization_revision)
                    .unwrap_or_default(),
                recovery_fence_id: snapshot.as_ref().map(|snapshot| snapshot.recovery_fence_id),
                space_fence_status: default_space_fence_status(),
                issued_at: timestamp(Utc::now()),
                code_hashes: code_hashes.clone(),
                encrypted_codes: Some(encrypt_recovery_secret(
                    &self.encryption_key,
                    &serde_json::to_vec(&codes)?,
                )?),
                codes_delivered_at: None,
                codes_delivery_id: None,
                codes_invalidated_at: None,
            },
        );
        queue_recovery_audit(
            &mut state,
            request_id,
            "recovery.backup_codes_rotated",
            request_id,
            None,
            space_uid,
            principal_id,
            account_id,
            Some(issuer_principal_id),
            Some(issuer_account_id),
            issuer_credential_id,
            Some(issuer_principal_id),
            Some(issuer_account_id),
            issuer_credential_id,
            serde_json::json!({
                "space_uid": space_uid,
                "principal_id": principal_id,
                "issuer_principal_id": issuer_principal_id,
                "code_count": codes.len()
            }),
        );
        if let Err(error) = self.write_state(&state).await {
            let committed = self.read_state().await.ok().is_some_and(|observed| {
                observed
                    .backup_rotation_requests
                    .get(&request_id)
                    .is_some_and(|record| record.code_hashes == code_hashes)
            });
            if !committed {
                return Err(error);
            }
        }
        Ok(codes)
    }

    pub async fn authenticate_session(&self, session_token: &str) -> Result<AuthenticatedSession> {
        let _guard = self.state_lock.lock().await;
        let state = self.read_state().await?;
        let hash = token_hash(session_token);
        let key = session_key(state.node_id, &hash);
        let record = self
            .state_store
            .get(&key)
            .await?
            .ok_or_else(|| anyhow!("invalid session"))?;
        let mut session: BrowserSession = serde_json::from_slice(&record.value)?;
        if session.revoked_at.is_some() {
            bail!("session is revoked");
        }
        validate_expiry(&session.expires_at, "session")?;
        let last_seen = parse_timestamp(&session.last_seen_at)?;
        if Utc::now() - last_seen > Duration::hours(SESSION_IDLE_HOURS) {
            bail!("session idle timeout exceeded");
        }
        let refresh_idle_deadline = Utc::now() - last_seen > Duration::minutes(5);
        if refresh_idle_deadline {
            session.last_seen_at = timestamp(Utc::now());
        }
        let account = state
            .accounts
            .get(&session.account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .cloned()
            .ok_or_else(|| anyhow!("account is not active"))?;
        if session.credential_generation != account.credential_generation {
            bail!("session credential generation is stale");
        }
        if session.revocation_epoch
            != state
                .session_revocation_epochs
                .get(&session.session_id)
                .copied()
                .unwrap_or_default()
        {
            bail!("session revocation epoch is stale");
        }
        if !recovery_session_is_committed(&session, &state.recovery_reset_markers) {
            bail!("recovery session was not the committed reset winner");
        }
        let authenticated = AuthenticatedSession {
            account,
            session_id: session.session_id,
            credential_id: session.credential_id,
            assurance: session.assurance.clone(),
        };
        if refresh_idle_deadline
            && self
                .state_store
                .compare_and_swap(&key, &record.version, serde_json::to_vec(&session)?)
                .await
                .is_err()
        {
            let current = self
                .state_store
                .get(&key)
                .await?
                .ok_or_else(|| anyhow!("session was revoked"))?;
            let current: BrowserSession = serde_json::from_slice(&current.value)?;
            if current.revoked_at.is_some() {
                bail!("session was revoked");
            }
        }
        Ok(authenticated)
    }

    pub async fn node_is_active(&self) -> Result<bool> {
        Ok(matches!(
            self.read_state().await?.lifecycle,
            NodeLifecycle::Active
        ))
    }

    pub async fn revoke_session(&self, session_token: &str) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let key = session_key(state.node_id, &token_hash(session_token));
        let Some(record) = self.state_store.get(&key).await? else {
            return Ok(());
        };
        let mut session: BrowserSession = serde_json::from_slice(&record.value)?;
        if session.revoked_at.is_some() {
            return Ok(());
        }
        let revocation_epoch = state
            .session_revocation_epochs
            .entry(session.session_id)
            .or_insert(0);
        *revocation_epoch = revocation_epoch
            .checked_add(1)
            .ok_or_else(|| anyhow!("session revocation epoch exhausted"))?;
        session.revocation_epoch = *revocation_epoch;
        self.write_state(&state).await?;
        session.revoked_at = Some(timestamp(Utc::now()));
        self.state_store
            .compare_and_swap(&key, &record.version, serde_json::to_vec(&session)?)
            .await?;
        Ok(())
    }

    pub async fn list_sessions(&self, account_id: Uuid) -> Result<Vec<serde_json::Value>> {
        let state = self.read_state().await?;
        let prefix = format!("nodes/{}/sessions", state.node_id);
        let mut sessions = Vec::new();
        for key in self.state_store.list_prefix(&prefix).await? {
            let Some(record) = self.state_store.get(&key).await? else {
                continue;
            };
            let session: BrowserSession = serde_json::from_slice(&record.value)?;
            if session.account_id == account_id {
                sessions.push(serde_json::json!({
                    "session_id": session.session_id,
                    "credential_id": session.credential_id,
                    "assurance": session.assurance,
                    "created_at": session.created_at,
                    "last_seen_at": session.last_seen_at,
                    "expires_at": session.expires_at,
                    "revoked_at": session.revoked_at,
                }));
            }
        }
        sessions.sort_by(|left, right| {
            right["created_at"]
                .as_str()
                .cmp(&left["created_at"].as_str())
        });
        Ok(sessions)
    }

    pub async fn revoke_session_by_id(&self, account_id: Uuid, session_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let prefix = format!("nodes/{}/sessions", state.node_id);
        for key in self.state_store.list_prefix(&prefix).await? {
            let Some(record) = self.state_store.get(&key).await? else {
                continue;
            };
            let mut session: BrowserSession = serde_json::from_slice(&record.value)?;
            if session.session_id != session_id || session.account_id != account_id {
                continue;
            }
            if session.revoked_at.is_none() {
                let revocation_epoch = state
                    .session_revocation_epochs
                    .entry(session.session_id)
                    .or_insert(0);
                *revocation_epoch = revocation_epoch
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("session revocation epoch exhausted"))?;
                session.revocation_epoch = *revocation_epoch;
                self.write_state(&state).await?;
                session.revoked_at = Some(timestamp(Utc::now()));
                self.state_store
                    .compare_and_swap(&key, &record.version, serde_json::to_vec(&session)?)
                    .await?;
            }
            return Ok(());
        }
        bail!("session not found")
    }

    pub async fn list_accounts(&self) -> Result<Vec<HumanAccount>> {
        let state = self.read_state().await?;
        Ok(state.accounts.values().cloned().collect())
    }

    pub async fn set_account_status(
        &self,
        account_id: Uuid,
        status: AccountStatus,
    ) -> Result<HumanAccount> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        let target = state
            .accounts
            .get(&account_id)
            .cloned()
            .ok_or_else(|| anyhow!("account not found"))?;
        if !matches!(status, AccountStatus::Active)
            && target.node_roles.contains(&NodeRole::NodeAdmin)
        {
            let active_admins = state
                .accounts
                .values()
                .filter(|account| {
                    matches!(account.status, AccountStatus::Active)
                        && account.node_roles.contains(&NodeRole::NodeAdmin)
                })
                .count();
            if active_admins <= 1 {
                bail!("cannot suspend or revoke the last active node admin");
            }
        }
        let now = timestamp(Utc::now());
        if target.status != status {
            invalidate_pending_recovery_responses(&mut state, account_id, &now);
            *state
                .account_lifecycle_epochs
                .entry(account_id)
                .or_insert(0) += 1;
        }
        let account = state.accounts.get_mut(&account_id).expect("checked above");
        account.status = status.clone();
        let account = account.clone();
        if !matches!(status, AccountStatus::Active) {
            for credential in state
                .device_credentials
                .values_mut()
                .filter(|credential| credential.account_id == account_id)
            {
                credential.revoked_at = Some(now.clone());
            }
        }
        self.write_state(&state).await?;
        if !matches!(status, AccountStatus::Active) {
            self.revoke_account_sessions(state.node_id, account_id, &now)
                .await?;
        }
        Ok(account)
    }

    pub async fn session_has_recent_passkey(&self, session_token: &str) -> Result<bool> {
        let state = self.read_state().await?;
        let record = self
            .state_store
            .get(&session_key(state.node_id, &token_hash(session_token)))
            .await?
            .ok_or_else(|| anyhow!("invalid session"))?;
        let session: BrowserSession = serde_json::from_slice(&record.value)?;
        if !matches!(session.assurance, AssuranceLevel::PhishingResistant) {
            return Ok(false);
        }
        Ok(Utc::now() - parse_timestamp(&session.authenticated_at)? <= Duration::minutes(5))
    }

    pub async fn revalidate_recent_passkey_session(
        &self,
        session_token: &str,
        account_id: Uuid,
        credential_id: Uuid,
        expected_generation: u64,
    ) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let state = self.read_state().await?;
        self.validate_recent_passkey_session_state(
            &state,
            session_token,
            account_id,
            credential_id,
            expected_generation,
        )
        .await
    }

    async fn validate_recent_passkey_session_state(
        &self,
        state: &NodeState,
        session_token: &str,
        account_id: Uuid,
        credential_id: Uuid,
        expected_generation: u64,
    ) -> Result<()> {
        let account = state
            .accounts
            .get(&account_id)
            .filter(|account| {
                matches!(account.status, AccountStatus::Active)
                    && account.credential_generation == expected_generation
            })
            .ok_or_else(|| anyhow!("owner recovery session account is stale"))?;
        let record = self
            .state_store
            .get(&session_key(state.node_id, &token_hash(session_token)))
            .await?
            .ok_or_else(|| anyhow!("owner recovery session is invalid"))?;
        let session: BrowserSession = serde_json::from_slice(&record.value)?;
        if session.revoked_at.is_some()
            || session.account_id != account.account_id
            || session.credential_id != credential_id
            || session.credential_generation != expected_generation
            || session.revocation_epoch
                != state
                    .session_revocation_epochs
                    .get(&session.session_id)
                    .copied()
                    .unwrap_or_default()
            || !matches!(session.assurance, AssuranceLevel::PhishingResistant)
            || parse_timestamp(&session.expires_at)? <= Utc::now()
            || Utc::now() - parse_timestamp(&session.authenticated_at)? > Duration::minutes(5)
        {
            bail!("owner recovery session is no longer valid")
        }
        if !state
            .passkeys
            .values()
            .any(|passkey| passkey.account_id == account_id && passkey.method_id == credential_id)
        {
            bail!("owner recovery Passkey is no longer registered")
        }
        Ok(())
    }

    pub async fn issue_invitation(
        &self,
        actor: Uuid,
        display_name: &str,
        space_uid: Option<Uuid>,
        role: Option<String>,
    ) -> Result<(AccountInvitation, String)> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let actor_account = state
            .accounts
            .get(&actor)
            .ok_or_else(|| anyhow!("unknown actor"))?;
        if !actor_account.node_roles.contains(&NodeRole::NodeAdmin) && space_uid.is_none() {
            bail!("node admin role is required");
        }
        if let Some(space_uid) = space_uid {
            ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
        }
        let token = random_token(32)?;
        let invitation_id = Uuid::now_v7();
        let invitation = AccountInvitation {
            invitation_id,
            token_hash: token_hash(&token),
            display_name: normalized_display_name(display_name)?,
            space_uid,
            role,
            expires_at: timestamp(Utc::now() + Duration::hours(INVITATION_LIFETIME_HOURS)),
            acceptance: None,
            created_by: actor,
        };
        state.invitations.insert(invitation_id, invitation.clone());
        self.write_state(&state).await?;
        Ok((invitation, token))
    }

    pub async fn accept_invitation_for_account(
        &self,
        invitation_token: &str,
        account_id: Uuid,
    ) -> Result<(HumanAccount, AccountInvitation)> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let account = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .cloned()
            .ok_or_else(|| anyhow!("invitation is invalid"))?;
        let invitation_id = state
            .invitations
            .values()
            .find(|invitation| invitation.token_hash == token_hash(invitation_token))
            .map(|invitation| invitation.invitation_id)
            .ok_or_else(|| anyhow!("invitation is invalid"))?;
        if let Some(space_uid) = state
            .invitations
            .get(&invitation_id)
            .and_then(|invitation| invitation.space_uid)
        {
            ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        }
        let existing_principal_id = bound_principal_for_account(
            &state,
            state
                .invitations
                .get(&invitation_id)
                .and_then(|invitation| invitation.space_uid),
            account_id,
        )?;
        let mut write_state = false;
        let invitation = {
            let invitation = state
                .invitations
                .get_mut(&invitation_id)
                .ok_or_else(|| anyhow!("invitation is invalid"))?;
            if let Some(acceptance) = invitation.acceptance.as_ref() {
                if acceptance.account_id() == account_id {
                    if matches!(acceptance, InvitationAcceptance::Pending { .. })
                        && acceptance.credential_generation() != account.credential_generation
                    {
                        bail!("invitation acceptance is stale");
                    }
                    invitation.clone()
                } else {
                    bail!("invitation is invalid");
                }
            } else {
                validate_expiry(&invitation.expires_at, "invitation")?;
                invitation.acceptance = Some(InvitationAcceptance::Pending {
                    account_id,
                    principal_id: existing_principal_id.unwrap_or_else(Uuid::now_v7),
                    kind: InvitationAcceptanceKind::ExistingAccount,
                    claimed_at: timestamp(Utc::now()),
                    credential_generation: account.credential_generation,
                });
                write_state = true;
                invitation.clone()
            }
        };
        if write_state {
            self.write_state(&state).await?;
        }
        Ok((account, invitation))
    }

    pub async fn add_binding(&self, binding: PrincipalBinding) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if state.bindings.iter().any(|candidate| candidate == &binding) {
            return Ok(());
        }
        ensure_node_recovery_mutation_allowed(&mut state, binding.space_uid)?;
        ensure_node_account_recovery_mutation_allowed(&mut state, binding.node_account_id)?;
        if state.bindings.iter().any(|candidate| {
            candidate.space_uid == binding.space_uid
                && (candidate.principal_id == binding.principal_id
                    || candidate.node_account_id == binding.node_account_id)
        }) {
            bail!("principal or account is already bound in this space");
        }
        let account_id = binding.node_account_id;
        state.bindings.push(binding);
        let now = timestamp(Utc::now());
        invalidate_pending_recovery_responses(&mut state, account_id, &now);
        *state
            .account_lifecycle_epochs
            .entry(account_id)
            .or_insert(0) += 1;
        self.write_state(&state).await
    }

    /// Atomically commits the Node half of a Space invitation finalization.
    ///
    /// The acceptance generation, binding, and completed marker must be one
    /// Node CAS. The server can therefore commit the Space membership after
    /// this operation; a recovery that wins before this CAS leaves no active
    /// Space membership to clean up.
    pub async fn finalize_invitation_binding(
        &self,
        invitation_id: Uuid,
        account_id: Uuid,
        principal_id: Uuid,
        binding_method: BindingMethod,
    ) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let space_uid = state
            .invitations
            .get(&invitation_id)
            .and_then(|invitation| invitation.space_uid)
            .ok_or_else(|| anyhow!("invitation is not Space-scoped"))?;
        ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
        ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        let current_generation = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .map(|account| account.credential_generation)
            .ok_or_else(|| anyhow!("invitation account is invalid"))?;
        let acceptance = state
            .invitations
            .get(&invitation_id)
            .and_then(|invitation| invitation.acceptance.clone())
            .ok_or_else(|| anyhow!("invitation acceptance is incomplete"))?;
        match acceptance {
            InvitationAcceptance::Pending {
                account_id: claimed_account_id,
                principal_id: claimed_principal_id,
                kind,
                claimed_at,
                credential_generation,
            } if claimed_account_id == account_id
                && claimed_principal_id == principal_id
                && credential_generation == current_generation =>
            {
                if state.bindings.iter().any(|binding| {
                    binding.space_uid == space_uid && binding.principal_id == principal_id
                }) {
                    bail!("invitation principal is already bound");
                }
                if state.bindings.iter().any(|binding| {
                    binding.space_uid == space_uid && binding.node_account_id == account_id
                }) {
                    bail!("invitation account is already bound");
                }
                state.bindings.push(PrincipalBinding {
                    space_uid,
                    principal_id,
                    node_account_id: account_id,
                    binding_method,
                });
                let now = timestamp(Utc::now());
                invalidate_pending_recovery_responses(&mut state, account_id, &now);
                *state
                    .account_lifecycle_epochs
                    .entry(account_id)
                    .or_insert(0) += 1;
                state
                    .invitations
                    .get_mut(&invitation_id)
                    .expect("invitation was checked above")
                    .acceptance = Some(InvitationAcceptance::Completed {
                    account_id,
                    principal_id,
                    kind,
                    claimed_at,
                    credential_generation,
                    completed_at: now,
                });
                self.write_state(&state).await
            }
            InvitationAcceptance::Completed {
                account_id: claimed_account_id,
                principal_id: claimed_principal_id,
                ..
            } if claimed_account_id == account_id && claimed_principal_id == principal_id => {
                if state.bindings.iter().any(|binding| {
                    binding.space_uid == space_uid
                        && binding.principal_id == principal_id
                        && binding.node_account_id == account_id
                }) {
                    Ok(())
                } else {
                    bail!("completed invitation binding is missing")
                }
            }
            _ => bail!("invitation acceptance does not match finalization"),
        }
    }

    pub async fn complete_invitation_acceptance(
        &self,
        invitation_id: Uuid,
        account_id: Uuid,
        principal_id: Uuid,
    ) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let space_uid = state
            .invitations
            .get(&invitation_id)
            .and_then(|invitation| invitation.space_uid);
        if let Some(space_uid) = space_uid {
            ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        }
        let current_generation = state
            .accounts
            .get(&account_id)
            .map(|account| account.credential_generation)
            .ok_or_else(|| anyhow!("invitation account is invalid"))?;
        let invitation = state
            .invitations
            .get_mut(&invitation_id)
            .ok_or_else(|| anyhow!("invitation is invalid"))?;
        let acceptance = invitation
            .acceptance
            .take()
            .ok_or_else(|| anyhow!("invitation acceptance is incomplete"))?;
        let (acceptance, changed) = match acceptance {
            InvitationAcceptance::Pending {
                account_id: claimed_account_id,
                principal_id: claimed_principal_id,
                kind,
                claimed_at,
                credential_generation,
            } if claimed_account_id == account_id
                && claimed_principal_id == principal_id
                && credential_generation == current_generation =>
            {
                (
                    InvitationAcceptance::Completed {
                        account_id,
                        principal_id,
                        kind,
                        claimed_at,
                        credential_generation,
                        completed_at: timestamp(Utc::now()),
                    },
                    true,
                )
            }
            completed @ InvitationAcceptance::Completed {
                account_id: claimed_account_id,
                principal_id: claimed_principal_id,
                ..
            } if claimed_account_id == account_id && claimed_principal_id == principal_id => {
                (completed, false)
            }
            _ => bail!("invitation acceptance does not match finalization"),
        };
        invitation.acceptance = Some(acceptance);
        if changed {
            self.write_state(&state).await?;
        }
        Ok(())
    }

    pub async fn binding_for_account(
        &self,
        space_uid: Uuid,
        account_id: Uuid,
    ) -> Result<Option<Uuid>> {
        let state = self.read_state().await?;
        bound_principal_for_account(&state, Some(space_uid), account_id)
    }

    pub async fn principal_for_account(&self, space_uid: Uuid, account_id: Uuid) -> Result<Uuid> {
        self.binding_for_account(space_uid, account_id)
            .await?
            .ok_or_else(|| anyhow!("account is not bound to a principal in this space"))
    }

    pub async fn bind_local_owner(
        &self,
        space_uid: Uuid,
        principal_id: Uuid,
        account_id: Uuid,
    ) -> Result<()> {
        self.add_binding(PrincipalBinding {
            space_uid,
            principal_id,
            node_account_id: account_id,
            binding_method: BindingMethod::Setup,
        })
        .await
    }

    pub async fn start_device_authorization(
        &self,
        device_name: &str,
        public_key_jwk: serde_json::Value,
        requested_space_uid: Option<Uuid>,
        requested_actions: BTreeSet<String>,
    ) -> Result<serde_json::Value> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let device_code = random_token(32)?;
        let user_code = random_user_code()?;
        let expires_at = timestamp(Utc::now() + Duration::minutes(10));
        state.device_authorizations.insert(
            token_hash(&device_code),
            DeviceAuthorizationRequest {
                device_code_hash: token_hash(&device_code),
                user_code_hash: token_hash(&user_code),
                device_name: normalized_display_name(device_name)?,
                public_key_jwk,
                requested_space_uid,
                requested_actions,
                approved_account_id: None,
                approved_principal_id: None,
                approved_credential_generation: None,
                expires_at: expires_at.clone(),
                used_at: None,
                last_polled_at: None,
                polling_interval_seconds: 5,
            },
        );
        self.write_state(&state).await?;
        Ok(serde_json::json!({
            "device_code": device_code,
            "user_code": user_code,
            "verification_uri": format!("{}/device", self.public_origin.trim_end_matches('/')),
            "verification_uri_complete": format!("{}/device?user_code={user_code}", self.public_origin.trim_end_matches('/')),
            "expires_in": 600,
            "interval": 5,
            "expires_at": expires_at,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn issue_authorization_code(
        &self,
        client_id: &str,
        redirect_uri: &str,
        code_challenge: &str,
        public_key_jwk: serde_json::Value,
        account_id: Uuid,
        principal_id: Uuid,
        space_uid: Uuid,
        granted_actions: BTreeSet<String>,
    ) -> Result<String> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if !state
            .accounts
            .get(&account_id)
            .is_some_and(|account| matches!(account.status, AccountStatus::Active))
        {
            bail!("authorization account is not active");
        }
        let credential_generation = state
            .accounts
            .get(&account_id)
            .map(|account| account.credential_generation)
            .ok_or_else(|| anyhow!("authorization account is not active"))?;
        let code = random_token(32)?;
        let code_hash = token_hash(&code);
        state.authorization_codes.insert(
            code_hash.clone(),
            AuthorizationCodeGrant {
                code_hash,
                client_id: normalized_display_name(client_id)?,
                redirect_uri: redirect_uri.to_string(),
                code_challenge: code_challenge.to_string(),
                public_key_jwk,
                account_id,
                credential_generation,
                principal_id,
                space_uid,
                granted_actions,
                expires_at: timestamp(Utc::now() + Duration::minutes(5)),
                used_at: None,
            },
        );
        self.write_state(&state).await?;
        Ok(code)
    }

    pub async fn exchange_authorization_code(
        &self,
        code: &str,
        client_id: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<(
        DeviceCredential,
        RefreshCredential,
        String,
        serde_json::Value,
    )> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let grant = state
            .authorization_codes
            .get_mut(&token_hash(code))
            .ok_or_else(|| anyhow!("authorization code is invalid"))?;
        validate_expiry(&grant.expires_at, "authorization code")?;
        if grant.used_at.is_some()
            || grant.client_id != client_id
            || grant.redirect_uri != redirect_uri
            || URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()))
                != grant.code_challenge
            || state.accounts.get(&grant.account_id).is_none_or(|account| {
                !matches!(account.status, AccountStatus::Active)
                    || account.credential_generation != grant.credential_generation
            })
        {
            bail!("authorization code is invalid");
        }
        grant.used_at = Some(timestamp(Utc::now()));
        let grant = grant.clone();
        let credential_id = Uuid::now_v7();
        let credential = DeviceCredential {
            credential_id,
            device_name: grant.client_id,
            public_key_jwk: grant.public_key_jwk,
            account_id: grant.account_id,
            credential_generation: grant.credential_generation,
            created_at: timestamp(Utc::now()),
            last_used_at: None,
            expires_at: None,
            revoked_at: None,
        };
        state
            .device_credentials
            .insert(credential_id, credential.clone());
        let refresh_token = random_token(48)?;
        let refresh = RefreshCredential {
            refresh_hash: token_hash(&refresh_token),
            credential_id,
            account_id: grant.account_id,
            credential_generation: grant.credential_generation,
            principal_id: grant.principal_id,
            space_uid: grant.space_uid,
            granted_actions: grant.granted_actions,
            expires_at: timestamp(Utc::now() + Duration::days(30)),
            revoked_at: None,
        };
        state
            .refresh_credentials
            .insert(refresh.refresh_hash.clone(), refresh.clone());
        let context = serde_json::json!({"issuer": state.issuer});
        self.write_state(&state).await?;
        Ok((credential, refresh, refresh_token, context))
    }

    pub async fn pending_authorization_code(&self, code: &str) -> Result<AuthorizationCodeGrant> {
        let state = self.read_state().await?;
        let grant = state
            .authorization_codes
            .get(&token_hash(code))
            .filter(|grant| grant.used_at.is_none())
            .cloned()
            .ok_or_else(|| anyhow!("authorization code is invalid"))?;
        validate_expiry(&grant.expires_at, "authorization code")?;
        Ok(grant)
    }

    pub async fn pending_device_authorization(
        &self,
        user_code: &str,
    ) -> Result<DeviceAuthorizationRequest> {
        let state = self.read_state().await?;
        let hash = token_hash(&user_code.trim().to_uppercase());
        let request = state
            .device_authorizations
            .values()
            .find(|request| request.user_code_hash == hash)
            .cloned()
            .ok_or_else(|| anyhow!("unknown user code"))?;
        validate_expiry(&request.expires_at, "device authorization")?;
        if request.used_at.is_some() {
            bail!("device authorization was already used");
        }
        Ok(request)
    }

    pub async fn pending_device_by_device_code(
        &self,
        device_code: &str,
    ) -> Result<DeviceAuthorizationRequest> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let request = state
            .device_authorizations
            .get_mut(&token_hash(device_code))
            .ok_or_else(|| anyhow!("invalid device code"))?;
        validate_expiry(&request.expires_at, "device authorization")?;
        if request.used_at.is_some() {
            bail!("device authorization was already used");
        }
        let now = Utc::now();
        if request.last_polled_at.as_deref().is_some_and(|last| {
            parse_timestamp(last).is_ok_and(|last| {
                now - last < Duration::seconds(request.polling_interval_seconds as i64)
            })
        }) {
            bail!("slow_down");
        }
        request.last_polled_at = Some(timestamp(now));
        let request = request.clone();
        self.write_state(&state).await?;
        Ok(request)
    }

    pub async fn approve_device_authorization(
        &self,
        user_code: &str,
        account_id: Uuid,
        principal_id: Uuid,
        space_uid: Uuid,
        granted_actions: BTreeSet<String>,
    ) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let approved_credential_generation = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .map(|account| account.credential_generation)
            .ok_or_else(|| anyhow!("device authorization account is not active"))?;
        let hash = token_hash(&user_code.trim().to_uppercase());
        let request = state
            .device_authorizations
            .values_mut()
            .find(|request| request.user_code_hash == hash)
            .ok_or_else(|| anyhow!("unknown user code"))?;
        validate_expiry(&request.expires_at, "device authorization")?;
        if request.approved_account_id.is_some() {
            bail!("device authorization is already approved");
        }
        request.requested_space_uid = Some(space_uid);
        request.requested_actions = granted_actions;
        request.approved_account_id = Some(account_id);
        request.approved_principal_id = Some(principal_id);
        request.approved_credential_generation = Some(approved_credential_generation);
        self.write_state(&state).await
    }

    pub async fn exchange_device_code(
        &self,
        device_code: &str,
    ) -> Result<(
        DeviceCredential,
        RefreshCredential,
        String,
        serde_json::Value,
    )> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let hash = token_hash(device_code);
        let request = state
            .device_authorizations
            .get_mut(&hash)
            .ok_or_else(|| anyhow!("invalid device code"))?;
        validate_expiry(&request.expires_at, "device authorization")?;
        if request.used_at.is_some() {
            bail!("device code was already consumed");
        }
        let account_id = request
            .approved_account_id
            .ok_or_else(|| anyhow!("authorization_pending"))?;
        let principal_id = request
            .approved_principal_id
            .ok_or_else(|| anyhow!("authorization_pending"))?;
        let space_uid = request
            .requested_space_uid
            .ok_or_else(|| anyhow!("authorization_pending"))?;
        let credential_generation = request.approved_credential_generation.unwrap_or_default();
        if state
            .accounts
            .get(&account_id)
            .is_none_or(|account| account.credential_generation != credential_generation)
        {
            bail!("device authorization is stale");
        }
        request.used_at = Some(timestamp(Utc::now()));
        let credential_id = Uuid::now_v7();
        let credential = DeviceCredential {
            credential_id,
            device_name: request.device_name.clone(),
            public_key_jwk: request.public_key_jwk.clone(),
            account_id,
            credential_generation,
            created_at: timestamp(Utc::now()),
            last_used_at: None,
            expires_at: None,
            revoked_at: None,
        };
        let actions = request.requested_actions.clone();
        state
            .device_credentials
            .insert(credential_id, credential.clone());
        let refresh_token = random_token(48)?;
        let refresh = RefreshCredential {
            refresh_hash: token_hash(&refresh_token),
            credential_id,
            account_id,
            credential_generation,
            principal_id,
            space_uid,
            granted_actions: actions,
            expires_at: timestamp(Utc::now() + Duration::days(30)),
            revoked_at: None,
        };
        state
            .refresh_credentials
            .insert(refresh.refresh_hash.clone(), refresh.clone());
        let token_context = serde_json::json!({"issuer": state.issuer});
        self.write_state(&state).await?;
        Ok((credential, refresh, refresh_token, token_context))
    }

    pub async fn device_credential(&self, credential_id: Uuid) -> Result<DeviceCredential> {
        let state = self.read_state().await?;
        let credential = state
            .device_credentials
            .get(&credential_id)
            .filter(|credential| credential.revoked_at.is_none())
            .cloned()
            .ok_or_else(|| anyhow!("device credential is missing or revoked"))?;
        if state
            .accounts
            .get(&credential.account_id)
            .is_none_or(|account| {
                !matches!(account.status, AccountStatus::Active)
                    || account.credential_generation != credential.credential_generation
            })
        {
            bail!("device credential generation is stale");
        }
        Ok(credential)
    }

    pub async fn refresh_credential(&self, refresh_token: &str) -> Result<RefreshCredential> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let hash = token_hash(refresh_token);
        let refresh = state
            .refresh_credentials
            .get(&hash)
            .cloned()
            .ok_or_else(|| anyhow!("refresh credential is invalid or revoked"))?;
        if refresh.revoked_at.is_some() {
            let now = timestamp(Utc::now());
            if let Some(device) = state.device_credentials.get_mut(&refresh.credential_id) {
                device.revoked_at = Some(now.clone());
            }
            for member in state
                .refresh_credentials
                .values_mut()
                .filter(|member| member.credential_id == refresh.credential_id)
            {
                member.revoked_at = Some(now.clone());
            }
            self.write_state(&state).await?;
            bail!("refresh credential reuse detected; device grant revoked");
        }
        validate_expiry(&refresh.expires_at, "refresh credential")?;
        if state
            .accounts
            .get(&refresh.account_id)
            .is_none_or(|account| {
                !matches!(account.status, AccountStatus::Active)
                    || account.credential_generation != refresh.credential_generation
            })
        {
            bail!("refresh credential generation is stale");
        }
        Ok(refresh)
    }

    pub async fn rotate_refresh_credential(
        &self,
        refresh_token: &str,
    ) -> Result<(String, RefreshCredential, serde_json::Value)> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let old_hash = token_hash(refresh_token);
        if state
            .refresh_credentials
            .get(&old_hash)
            .is_some_and(|refresh| refresh.revoked_at.is_some())
        {
            let credential_id = state
                .refresh_credentials
                .get(&old_hash)
                .expect("checked above")
                .credential_id;
            let now = timestamp(Utc::now());
            if let Some(device) = state.device_credentials.get_mut(&credential_id) {
                device.revoked_at = Some(now.clone());
            }
            for member in state
                .refresh_credentials
                .values_mut()
                .filter(|member| member.credential_id == credential_id)
            {
                member.revoked_at = Some(now.clone());
            }
            self.write_state(&state).await?;
            bail!("refresh credential reuse detected; device grant revoked");
        }
        let old = state
            .refresh_credentials
            .get_mut(&old_hash)
            .filter(|refresh| refresh.revoked_at.is_none())
            .ok_or_else(|| anyhow!("refresh credential is invalid or revoked"))?;
        validate_expiry(&old.expires_at, "refresh credential")?;
        if state.accounts.get(&old.account_id).is_none_or(|account| {
            !matches!(account.status, AccountStatus::Active)
                || account.credential_generation != old.credential_generation
        }) {
            bail!("refresh credential generation is stale");
        }
        old.revoked_at = Some(timestamp(Utc::now()));
        let mut rotated = old.clone();
        let token = random_token(48)?;
        rotated.refresh_hash = token_hash(&token);
        rotated.revoked_at = None;
        rotated.expires_at = timestamp(Utc::now() + Duration::days(30));
        state
            .refresh_credentials
            .insert(rotated.refresh_hash.clone(), rotated.clone());
        let context = serde_json::json!({"issuer": state.issuer});
        self.write_state(&state).await?;
        Ok((token, rotated, context))
    }

    pub async fn list_device_credentials(&self, account_id: Uuid) -> Result<Vec<DeviceCredential>> {
        let state = self.read_state().await?;
        Ok(state
            .device_credentials
            .values()
            .filter(|credential| credential.account_id == account_id)
            .cloned()
            .collect())
    }

    pub async fn revoke_device_credential(&self, actor: Uuid, credential_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let actor_account = state
            .accounts
            .get(&actor)
            .ok_or_else(|| anyhow!("actor not found"))?;
        let credential = state
            .device_credentials
            .get_mut(&credential_id)
            .ok_or_else(|| anyhow!("credential not found"))?;
        if credential.account_id != actor
            && !actor_account.node_roles.contains(&NodeRole::NodeAdmin)
        {
            bail!("credential owner or node admin is required");
        }
        credential.revoked_at = Some(timestamp(Utc::now()));
        for refresh in state
            .refresh_credentials
            .values_mut()
            .filter(|refresh| refresh.credential_id == credential_id)
        {
            refresh.revoked_at = Some(timestamp(Utc::now()));
        }
        self.write_state(&state).await
    }

    pub async fn issuer_metadata(&self) -> Result<(String, Uuid)> {
        let state = self.read_state().await?;
        Ok((state.issuer, state.node_id))
    }

    pub async fn issue_access_credential(&self, claims: AccessTokenClaims) -> Result<String> {
        let _guard = self.state_lock.lock().await;
        let state = self.read_state().await?;
        if claims.iss != state.issuer || claims.exp <= Utc::now().timestamp() {
            bail!("access credential metadata is invalid");
        }
        let token = random_token(32)?;
        let hash = token_hash(&token);
        self.state_store
            .create_if_absent(
                &access_credential_key(state.node_id, &hash),
                serde_json::to_vec(&claims)?,
            )
            .await?;
        Ok(token)
    }

    pub async fn resolve_access_credential(&self, token: &str) -> Result<AccessTokenClaims> {
        let state = self.read_state().await?;
        let record = self
            .state_store
            .get(&access_credential_key(state.node_id, &token_hash(token)))
            .await?
            .ok_or_else(|| anyhow!("access credential is invalid"))?;
        let claims: AccessTokenClaims = serde_json::from_slice(&record.value)?;
        let now = Utc::now().timestamp();
        if claims.exp <= now || claims.iat > now + 60 || claims.iss != state.issuer {
            bail!("access credential is expired or invalid");
        }
        if claims.principal_type == "agent" || claims.actor_principal_id.is_some() {
            let credential = state
                .agent_credentials
                .get(&claims.credential_id)
                .filter(|credential| credential.revoked_at.is_none())
                .ok_or_else(|| anyhow!("agent credential is revoked"))?;
            if credential
                .expires_at
                .as_deref()
                .is_some_and(|expires| validate_expiry(expires, "agent credential").is_err())
            {
                bail!("agent credential has expired");
            }
        } else {
            let credential = state
                .device_credentials
                .get(&claims.credential_id)
                .filter(|credential| credential.revoked_at.is_none())
                .ok_or_else(|| anyhow!("device credential is revoked"))?;
            let _account = state
                .accounts
                .get(&credential.account_id)
                .filter(|account| {
                    matches!(account.status, AccountStatus::Active)
                        && account.credential_generation == credential.credential_generation
                        && claims.credential_generation.unwrap_or_default()
                            == account.credential_generation
                })
                .ok_or_else(|| anyhow!("device account is not active"))?;
        }
        Ok(claims)
    }

    pub async fn oauth_credential_public_key(
        &self,
        credential_id: Uuid,
    ) -> Result<serde_json::Value> {
        let state = self.read_state().await?;
        if let Some(device) = state.device_credentials.get(&credential_id) {
            if device.revoked_at.is_some()
                || state
                    .accounts
                    .get(&device.account_id)
                    .is_none_or(|account| {
                        !matches!(account.status, AccountStatus::Active)
                            || account.credential_generation != device.credential_generation
                    })
            {
                bail!("OAuth client credential is revoked");
            }
            return Ok(device.public_key_jwk.clone());
        }
        state
            .agent_credentials
            .get(&credential_id)
            .filter(|credential| credential.revoked_at.is_none())
            .map(|credential| credential.public_key_jwk.clone())
            .ok_or_else(|| anyhow!("OAuth client credential is unavailable"))
    }

    /// Revokes an opaque access token directly. Revoking a refresh credential
    /// revokes its device grant, which also invalidates every access token
    /// issued to that sender on the next request.
    pub async fn revoke_oauth_token(&self, token: &str, client_credential_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let hash = token_hash(token);
        let access_key = access_credential_key(state.node_id, &hash);
        if let Some(record) = self.state_store.get(&access_key).await? {
            let claims: AccessTokenClaims = serde_json::from_slice(&record.value)?;
            if claims.credential_id != client_credential_id {
                return Ok(());
            }
            self.state_store
                .delete_if_version(&access_key, &record.version)
                .await?;
            return Ok(());
        }
        let Some(refresh) = state.refresh_credentials.get(&hash).cloned() else {
            // RFC 7009 requires the endpoint not to reveal whether a token exists.
            return Ok(());
        };
        if refresh.credential_id != client_credential_id {
            return Ok(());
        }
        let now = timestamp(Utc::now());
        if let Some(device) = state.device_credentials.get_mut(&refresh.credential_id) {
            device.revoked_at = Some(now.clone());
        }
        for family_member in state
            .refresh_credentials
            .values_mut()
            .filter(|candidate| candidate.credential_id == refresh.credential_id)
        {
            family_member.revoked_at = Some(now.clone());
        }
        self.write_state(&state).await
    }

    pub async fn configure_oidc_provider(
        &self,
        actor: Uuid,
        issuer: &str,
        client_id: &str,
        client_secret: Option<String>,
    ) -> Result<OidcProvider> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let actor = state
            .accounts
            .get(&actor)
            .ok_or_else(|| anyhow!("actor not found"))?;
        if !actor.node_roles.contains(&NodeRole::NodeAdmin) {
            bail!("node admin role is required");
        }
        let issuer = issuer.trim().trim_end_matches('/');
        if !issuer.starts_with("https://") {
            bail!("OIDC issuer must use https");
        }
        if client_id.trim().is_empty() {
            bail!("OIDC client_id is required");
        }
        let provider_id = Uuid::now_v7();
        let encrypted_secret = client_secret
            .filter(|secret| !secret.trim().is_empty())
            .map(|secret| {
                encrypt_recovery_secret(&self.encryption_key, secret.trim().as_bytes())
                    .map(|sealed| format!("enc:{sealed}"))
            })
            .transpose()?;
        let provider = OidcProvider {
            provider_id,
            issuer: issuer.to_string(),
            client_id: client_id.trim().to_string(),
            client_secret: encrypted_secret,
            enabled: true,
            created_at: timestamp(Utc::now()),
        };
        state.oidc_providers.insert(provider_id, provider.clone());
        self.write_state(&state).await?;
        let mut response = provider;
        response.client_secret = None;
        Ok(response)
    }

    pub async fn list_oidc_providers(&self) -> Result<Vec<OidcProvider>> {
        let state = self.read_state().await?;
        Ok(state
            .oidc_providers
            .values()
            .cloned()
            .map(|mut provider| {
                provider.client_secret = None;
                provider
            })
            .collect())
    }

    pub async fn oidc_provider(&self, provider_id: Uuid) -> Result<OidcProvider> {
        let state = self.read_state().await?;
        let mut provider = state
            .oidc_providers
            .get(&provider_id)
            .filter(|provider| provider.enabled)
            .cloned()
            .ok_or_else(|| anyhow!("OIDC provider not found or disabled"))?;
        if let Some(sealed) = provider
            .client_secret
            .as_deref()
            .and_then(|value| value.strip_prefix("enc:"))
        {
            provider.client_secret = Some(String::from_utf8(decrypt_recovery_secret(
                &self.encryption_key,
                sealed,
            )?)?);
        }
        Ok(provider)
    }

    pub async fn save_oidc_attempt(
        &self,
        provider_id: Uuid,
        state_token: &str,
        nonce: &str,
        pkce_verifier: &str,
        invitation_token: Option<&str>,
        link_account_id: Option<Uuid>,
    ) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if !state
            .oidc_providers
            .get(&provider_id)
            .is_some_and(|provider| provider.enabled)
        {
            bail!("OIDC provider is not enabled");
        }
        if let Some(account_id) = link_account_id {
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        }
        if let Some(space_uid) = invitation_token.and_then(|token| {
            state
                .invitations
                .values()
                .find(|invitation| invitation.token_hash == token_hash(token))
                .and_then(|invitation| invitation.space_uid)
        }) {
            ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
        }
        let invitation_account_id = invitation_token.and_then(|token| {
            state
                .invitations
                .values()
                .find(|invitation| invitation.token_hash == token_hash(token))
                .and_then(|invitation| invitation.acceptance.as_ref())
                .map(InvitationAcceptance::account_id)
        });
        if let Some(account_id) = invitation_account_id {
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
        }
        let invitation_account_generation = invitation_account_id.and_then(|account_id| {
            state
                .accounts
                .get(&account_id)
                .map(|account| account.credential_generation)
        });
        let link_account_generation = link_account_id.and_then(|account_id| {
            state
                .accounts
                .get(&account_id)
                .map(|account| account.credential_generation)
        });
        state.oidc_attempts.insert(
            token_hash(state_token),
            OidcLoginAttempt {
                state_hash: token_hash(state_token),
                provider_id,
                nonce: nonce.to_string(),
                pkce_verifier: pkce_verifier.to_string(),
                invitation_hash: invitation_token.map(token_hash),
                link_account_id,
                link_account_generation,
                invitation_account_id,
                invitation_account_generation,
                expires_at: timestamp(Utc::now() + Duration::minutes(10)),
            },
        );
        self.write_state(&state).await
    }

    pub async fn consume_oidc_attempt(&self, state_token: &str) -> Result<OidcLoginAttempt> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let attempt = state
            .oidc_attempts
            .remove(&token_hash(state_token))
            .ok_or_else(|| anyhow!("invalid OIDC state"))?;
        validate_expiry(&attempt.expires_at, "OIDC login attempt")?;
        if let (Some(account_id), Some(expected_generation)) =
            (attempt.link_account_id, attempt.link_account_generation)
        {
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
            if state
                .accounts
                .get(&account_id)
                .is_none_or(|account| account.credential_generation != expected_generation)
            {
                bail!("OIDC login attempt is stale");
            }
        }
        if let (Some(account_id), Some(expected_generation)) = (
            attempt.invitation_account_id,
            attempt.invitation_account_generation,
        ) {
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
            if state
                .accounts
                .get(&account_id)
                .is_none_or(|account| account.credential_generation != expected_generation)
            {
                bail!("OIDC invitation login attempt is stale");
            }
        }
        let legacy_invitation_account = attempt.invitation_hash.as_ref().and_then(|hash| {
            state
                .invitations
                .values()
                .find(|invitation| invitation.token_hash == *hash)
                .and_then(|invitation| invitation.acceptance.as_ref())
                .map(InvitationAcceptance::account_id)
        });
        if let Some(account_id) = legacy_invitation_account {
            let expected_generation = attempt
                .invitation_account_generation
                .ok_or_else(|| anyhow!("OIDC invitation login attempt is stale"))?;
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
            if state
                .accounts
                .get(&account_id)
                .is_none_or(|account| account.credential_generation != expected_generation)
            {
                bail!("OIDC invitation login attempt is stale");
            }
        }
        if let Some(space_uid) = attempt.invitation_hash.as_deref().and_then(|hash| {
            state
                .invitations
                .values()
                .find(|invitation| invitation.token_hash == hash)
                .and_then(|invitation| invitation.space_uid)
        }) {
            ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
        }
        self.write_state(&state).await?;
        Ok(attempt)
    }

    pub async fn complete_oidc_login(
        &self,
        issuer: &str,
        subject: &str,
        display_name: &str,
        invitation_hash: Option<&str>,
        link_account_id: Option<Uuid>,
        link_account_generation: Option<u64>,
        invitation_account_generation: Option<u64>,
    ) -> Result<(HumanAccount, String, Option<AccountInvitation>)> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if let Some(link_account_id) = link_account_id {
            let expected_generation = link_account_generation
                .ok_or_else(|| anyhow!("OIDC login attempt is missing credential generation"))?;
            ensure_node_account_recovery_mutation_allowed(&mut state, link_account_id)?;
            if state
                .accounts
                .get(&link_account_id)
                .is_none_or(|account| account.credential_generation != expected_generation)
            {
                bail!("OIDC login attempt is stale");
            }
        }
        let external_subject = ugoite_domain::identity::oidc_external_subject(issuer, subject)?;
        let existing_account = state
            .authentication_methods
            .values()
            .find(|method| {
                matches!(method.kind, AuthenticationMethodKind::Oidc)
                    && method.external_subject.as_deref() == Some(&external_subject)
            })
            .map(|method| method.account_id);
        let (account, invitation) = if let Some(link_account_id) = link_account_id {
            if existing_account.is_some_and(|account_id| account_id != link_account_id) {
                bail!("OIDC identity is already linked to another account");
            }
            let account = state
                .accounts
                .get(&link_account_id)
                .filter(|account| matches!(account.status, AccountStatus::Active))
                .cloned()
                .ok_or_else(|| anyhow!("account is not active"))?;
            if link_account_generation
                .is_some_and(|generation| account.credential_generation != generation)
            {
                bail!("OIDC login attempt is stale");
            }
            (account, None)
        } else if let Some(account_id) = existing_account {
            let invitation_is_already_bound = invitation_hash.is_some_and(|hash| {
                state
                    .invitations
                    .values()
                    .find(|invitation| invitation.token_hash == hash)
                    .and_then(|invitation| invitation.acceptance.as_ref())
                    .is_some()
            });
            if invitation_is_already_bound && invitation_account_generation.is_none() {
                bail!("OIDC invitation login attempt is stale");
            }
            ensure_node_account_recovery_mutation_allowed(&mut state, account_id)?;
            if invitation_account_generation.is_some_and(|generation| {
                state
                    .accounts
                    .get(&account_id)
                    .is_none_or(|account| account.credential_generation != generation)
            }) {
                bail!("OIDC invitation login attempt is stale");
            }
            let account = state
                .accounts
                .get(&account_id)
                .filter(|account| matches!(account.status, AccountStatus::Active))
                .cloned()
                .ok_or_else(|| anyhow!("OIDC account is not active"))?;
            let invitation = if let Some(invitation_hash) = invitation_hash {
                let invitation_space_uid = state
                    .invitations
                    .values()
                    .find(|invitation| invitation.token_hash == invitation_hash)
                    .and_then(|invitation| invitation.space_uid);
                if let Some(space_uid) = invitation_space_uid {
                    ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
                }
                let existing_principal_id =
                    bound_principal_for_account(&state, invitation_space_uid, account_id)?;
                let invitation = state
                    .invitations
                    .values_mut()
                    .find(|invitation| invitation.token_hash == invitation_hash)
                    .ok_or_else(|| anyhow!("invitation is invalid"))?;
                if let Some(acceptance) = invitation.acceptance.as_ref() {
                    if acceptance.account_id() != account_id
                        || !matches!(acceptance.kind(), InvitationAcceptanceKind::Oidc)
                    {
                        bail!("invitation is invalid");
                    }
                } else {
                    validate_expiry(&invitation.expires_at, "invitation")?;
                    invitation.acceptance = Some(InvitationAcceptance::Pending {
                        account_id,
                        principal_id: existing_principal_id.unwrap_or_else(Uuid::now_v7),
                        kind: InvitationAcceptanceKind::Oidc,
                        claimed_at: timestamp(Utc::now()),
                        credential_generation: account.credential_generation,
                    });
                }
                Some(invitation.clone())
            } else {
                None
            };
            (account, invitation)
        } else {
            let invitation_hash =
                invitation_hash.ok_or_else(|| anyhow!("new OIDC users require an invitation"))?;
            let invitation_space_uid = state
                .invitations
                .values()
                .find(|invitation| invitation.token_hash == invitation_hash)
                .and_then(|invitation| invitation.space_uid);
            if let Some(space_uid) = invitation_space_uid {
                ensure_node_recovery_mutation_allowed(&mut state, space_uid)?;
            }
            let invitation = state
                .invitations
                .values_mut()
                .find(|invitation| invitation.token_hash == invitation_hash)
                .ok_or_else(|| anyhow!("invitation is invalid"))?;
            validate_expiry(&invitation.expires_at, "invitation")?;
            if invitation.acceptance.is_some() {
                bail!("invitation was already used");
            }
            let account = HumanAccount {
                account_id: Uuid::now_v7(),
                display_name: if display_name.trim().is_empty() {
                    invitation.display_name.clone()
                } else {
                    normalized_display_name(display_name)?
                },
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            };
            invitation.acceptance = Some(InvitationAcceptance::Pending {
                account_id: account.account_id,
                principal_id: Uuid::now_v7(),
                kind: InvitationAcceptanceKind::Oidc,
                claimed_at: timestamp(Utc::now()),
                credential_generation: account.credential_generation,
            });
            let invitation_copy = invitation.clone();
            state.accounts.insert(account.account_id, account.clone());
            (account, Some(invitation_copy))
        };
        if existing_account.is_none() {
            let method_id = Uuid::now_v7();
            state.authentication_methods.insert(
                method_id,
                AuthenticationMethod {
                    method_id,
                    account_id: account.account_id,
                    kind: AuthenticationMethodKind::Oidc,
                    external_subject: Some(external_subject.clone()),
                    created_at: timestamp(Utc::now()),
                    last_used_at: Some(timestamp(Utc::now())),
                },
            );
        }
        let method_id = state
            .authentication_methods
            .values()
            .find(|method| {
                method.account_id == account.account_id
                    && matches!(method.kind, AuthenticationMethodKind::Oidc)
                    && method.external_subject.as_deref() == Some(&external_subject)
            })
            .map(|method| method.method_id)
            .ok_or_else(|| anyhow!("OIDC authentication method is unavailable"))?;
        let session = self
            .create_session(
                &state,
                account.account_id,
                method_id,
                AssuranceLevel::Federated,
            )
            .await?;
        self.write_state(&state).await?;
        Ok((account, session, invitation))
    }

    pub async fn record_proof_jti(&self, jti: &str) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let now = Utc::now();
        state
            .proof_replay_cache
            .retain(|_, expires| parse_timestamp(expires).is_ok_and(|value| value > now));
        if state.proof_replay_cache.contains_key(jti) {
            bail!("proof replay detected");
        }
        state
            .proof_replay_cache
            .insert(jti.to_string(), timestamp(now + Duration::minutes(5)));
        self.write_state(&state).await
    }

    pub async fn register_agent_credential(
        &self,
        agent_id: Uuid,
        public_key_jwk: serde_json::Value,
        expires_at: Option<String>,
    ) -> Result<AgentCredential> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let credential = AgentCredential {
            credential_id: Uuid::now_v7(),
            agent_id,
            public_key_jwk,
            created_at: timestamp(Utc::now()),
            last_used_at: None,
            expires_at,
            revoked_at: None,
        };
        state
            .agent_credentials
            .insert(credential.credential_id, credential.clone());
        self.write_state(&state).await?;
        Ok(credential)
    }

    pub async fn agent_credential(&self, credential_id: Uuid) -> Result<AgentCredential> {
        let state = self.read_state().await?;
        let credential = state
            .agent_credentials
            .get(&credential_id)
            .filter(|credential| credential.revoked_at.is_none())
            .cloned()
            .ok_or_else(|| anyhow!("agent credential is missing or revoked"))?;
        if credential
            .expires_at
            .as_deref()
            .is_some_and(|expires| validate_expiry(expires, "agent credential").is_err())
        {
            bail!("agent credential has expired");
        }
        Ok(credential)
    }

    pub async fn mark_agent_credential_used(&self, credential_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let credential = state
            .agent_credentials
            .get_mut(&credential_id)
            .filter(|credential| credential.revoked_at.is_none())
            .ok_or_else(|| anyhow!("agent credential is missing or revoked"))?;
        credential.last_used_at = Some(timestamp(Utc::now()));
        self.write_state(&state).await
    }

    pub async fn revoke_agent_credentials(&self, agent_id: Uuid) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let now = timestamp(Utc::now());
        for credential in state
            .agent_credentials
            .values_mut()
            .filter(|credential| credential.agent_id == agent_id)
        {
            credential.revoked_at = Some(now.clone());
        }
        self.write_state(&state).await
    }

    pub async fn account_for_principal(&self, space_uid: Uuid, principal_id: Uuid) -> Result<Uuid> {
        let state = self.read_state().await?;
        let bindings = state
            .bindings
            .iter()
            .filter(|binding| {
                binding.space_uid == space_uid && binding.principal_id == principal_id
            })
            .collect::<Vec<_>>();
        if bindings.len() != 1 {
            bail!("principal does not have exactly one Node account binding");
        }
        Ok(bindings[0].node_account_id)
    }

    pub async fn bindings_for_space(&self, space_uid: Uuid) -> Result<Vec<PrincipalBinding>> {
        Ok(self
            .read_state()
            .await?
            .bindings
            .into_iter()
            .filter(|binding| binding.space_uid == space_uid)
            .collect())
    }

    async fn create_session(
        &self,
        state: &NodeState,
        account_id: Uuid,
        credential_id: Uuid,
        assurance: AssuranceLevel,
    ) -> Result<String> {
        self.create_session_with_recovery(state, account_id, credential_id, assurance, None)
            .await
    }

    async fn create_session_with_recovery(
        &self,
        state: &NodeState,
        account_id: Uuid,
        credential_id: Uuid,
        assurance: AssuranceLevel,
        recovery_reset_id: Option<Uuid>,
    ) -> Result<String> {
        let session_token = random_token(32)?;
        let now = Utc::now();
        let now_text = timestamp(now);
        let hash = token_hash(&session_token);
        let session_id = Uuid::now_v7();
        let session = BrowserSession {
            session_id,
            session_hash: hash.clone(),
            credential_id,
            assurance,
            account_id,
            credential_generation: state
                .accounts
                .get(&account_id)
                .map(|account| account.credential_generation)
                .unwrap_or_default(),
            created_at: now_text.clone(),
            last_seen_at: now_text.clone(),
            expires_at: timestamp(now + Duration::days(SESSION_ABSOLUTE_DAYS)),
            authenticated_at: now_text,
            revoked_at: None,
            recovery_reset_id,
            revocation_epoch: state
                .session_revocation_epochs
                .get(&session_id)
                .copied()
                .unwrap_or_default(),
        };
        self.state_store
            .create_if_absent(
                &session_key(state.node_id, &hash),
                serde_json::to_vec(&session)?,
            )
            .await?;
        Ok(session_token)
    }

    async fn revoke_account_sessions(
        &self,
        node_id: Uuid,
        account_id: Uuid,
        revoked_at: &str,
    ) -> Result<()> {
        self.revoke_account_sessions_except(node_id, account_id, revoked_at, None)
            .await
    }

    async fn revoke_account_sessions_except(
        &self,
        node_id: Uuid,
        account_id: Uuid,
        revoked_at: &str,
        except_session_id: Option<Uuid>,
    ) -> Result<()> {
        let prefix = format!("nodes/{node_id}/sessions");
        for key in self.state_store.list_prefix(&prefix).await? {
            let Some(record) = self.state_store.get(&key).await? else {
                continue;
            };
            let mut session: BrowserSession = serde_json::from_slice(&record.value)?;
            if session.account_id != account_id
                || session.revoked_at.is_some()
                || except_session_id == Some(session.session_id)
            {
                continue;
            }
            session.revoked_at = Some(revoked_at.to_string());
            self.state_store
                .compare_and_swap(&key, &record.version, serde_json::to_vec(&session)?)
                .await?;
        }
        Ok(())
    }

    pub async fn read_state(&self) -> Result<NodeState> {
        let pointer = self
            .state_store
            .get(NODE_POINTER_KEY)
            .await?
            .ok_or_else(|| anyhow!("Node Identity is not initialized"))?;
        let node_id = serde_json::from_slice::<serde_json::Value>(&pointer.value)?
            .get("node_id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("Node control pointer is missing node_id"))
            .and_then(|value| Uuid::parse_str(value).map_err(anyhow::Error::from))?;
        let record = self
            .state_store
            .get(&node_state_key(node_id))
            .await?
            .ok_or_else(|| anyhow!("Node Identity state is incomplete"))?;
        let mut state: NodeState =
            serde_json::from_slice(&record.value).context("decode Node Identity state")?;
        if state.node_id != node_id {
            bail!("Node control pointer does not match stored Node Identity");
        }
        state.control_version = Some(record.version);
        Ok(state)
    }

    async fn write_state(&self, state: &NodeState) -> Result<()> {
        let bytes = serde_json::to_vec(state)?;
        let state_key = node_state_key(state.node_id);
        if let Some(version) = &state.control_version {
            if let Err(error) = self
                .state_store
                .compare_and_swap(&state_key, version, bytes.clone())
                .await
            {
                // A remote conditional write may have committed before its
                // post-write version probe failed. Read the object once to
                // distinguish a pre-commit conflict from a committed or
                // genuinely unknown outcome. Recovery callers keep their
                // durable fence for the latter two cases so reconciliation
                // can converge without exposing one-time secrets twice.
                match self.read_state().await {
                    Ok(observed) => {
                        if serde_json::to_vec(&observed)
                            .ok()
                            .is_some_and(|value| value == bytes)
                        {
                            return Err(anyhow!(
                                "node control write committed with an ambiguous response: {error}"
                            ));
                        }
                        return Err(error);
                    }
                    Err(read_error) if node_write_was_committed_with_ambiguous_response(&error) => {
                        return Err(anyhow!(
                            "node control write committed with an ambiguous response: {error}; verification failed: {read_error}"
                        ));
                    }
                    Err(read_error) => {
                        return Err(anyhow!(
                            "node control write outcome unknown: {error}; verification failed: {read_error}"
                        ));
                    }
                }
            }
        } else {
            self.state_store.create_if_absent(&state_key, bytes).await?;
            self.state_store
                .create_if_absent(
                    NODE_POINTER_KEY,
                    serde_json::to_vec(&serde_json::json!({
                        "schema_version": 1,
                        "node_id": state.node_id
                    }))?,
                )
                .await?;
        }
        Ok(())
    }

    async fn state_exists(&self) -> Result<bool> {
        Ok(self.state_store.get(NODE_POINTER_KEY).await?.is_some())
    }
}

fn normalized_display_name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 120 || value.contains(['\r', '\n']) {
        bail!("display name must be 1-120 characters on one line");
    }
    Ok(value.to_string())
}

fn session_key(node_id: Uuid, session_hash: &str) -> String {
    format!("nodes/{node_id}/sessions/{session_hash}.json")
}

fn recovery_session_is_committed(
    session: &BrowserSession,
    markers: &BTreeMap<Uuid, RecoveryResetMarker>,
) -> bool {
    let Some(reset_id) = session.recovery_reset_id else {
        return true;
    };
    markers.get(&reset_id).is_some_and(|marker| {
        marker.session_id == session.session_id
            && marker.account_id == session.account_id
            && marker.generation_after == session.credential_generation
            && marker.space_fence_status == "reconciled"
    })
}

fn access_credential_key(node_id: Uuid, token_hash: &str) -> String {
    format!("nodes/{node_id}/oauth_grants/access/{token_hash}.json")
}

fn validate_secret(record: Option<&OneTimeSecret>, supplied: &str, label: &str) -> Result<()> {
    let record = record.ok_or_else(|| anyhow!("{label} is not available"))?;
    if record.used_at.is_some() {
        bail!("{label} has already been used");
    }
    validate_expiry(&record.expires_at, label)?;
    if token_hash(supplied) != record.token_hash {
        bail!("invalid {label}");
    }
    Ok(())
}

fn validate_expiry(value: &str, label: &str) -> Result<()> {
    if parse_timestamp(value)? <= Utc::now() {
        bail!("{label} has expired");
    }
    Ok(())
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .context("invalid stored timestamp")
}

fn timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn random_token(bytes: usize) -> Result<String> {
    Ok(URL_SAFE_NO_PAD.encode(random_bytes(bytes)?))
}

fn random_bytes(bytes: usize) -> Result<Vec<u8>> {
    let mut value = vec![0_u8; bytes];
    rand::rngs::SysRng
        .try_fill_bytes(&mut value)
        .context("secure random generation failed")?;
    Ok(value)
}

fn encrypt_recovery_secret(key_material: &str, secret: &[u8]) -> Result<String> {
    let key = Sha256::digest(key_material.as_bytes());
    let cipher = <Aes256Gcm as KeyInit>::new_from_slice(&key)
        .map_err(|_| anyhow!("invalid recovery encryption key"))?;
    let nonce_bytes = random_bytes(12)?;
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).context("invalid recovery nonce")?;
    let ciphertext = cipher
        .encrypt(&nonce, secret)
        .map_err(|_| anyhow!("encrypt recovery secret"))?;
    let mut sealed = nonce_bytes;
    sealed.extend(ciphertext);
    Ok(URL_SAFE_NO_PAD.encode(sealed))
}

fn decrypt_recovery_secret(key_material: &str, sealed: &str) -> Result<Vec<u8>> {
    let sealed = URL_SAFE_NO_PAD
        .decode(sealed)
        .context("decode recovery secret")?;
    if sealed.len() <= 12 {
        bail!("invalid encrypted recovery secret");
    }
    let key = Sha256::digest(key_material.as_bytes());
    let cipher = <Aes256Gcm as KeyInit>::new_from_slice(&key)
        .map_err(|_| anyhow!("invalid recovery encryption key"))?;
    let nonce = Nonce::try_from(&sealed[..12]).context("invalid recovery nonce")?;
    cipher
        .decrypt(&nonce, &sealed[12..])
        .map_err(|_| anyhow!("decrypt recovery secret"))
}

fn verify_totp(secret: &[u8], code: &str, unix_seconds: i64) -> Result<bool> {
    let normalized = code.trim();
    if normalized.len() != 6 || !normalized.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(false);
    }
    let supplied: u32 = normalized.parse().context("parse TOTP code")?;
    let counter = unix_seconds.div_euclid(30);
    for offset in -1_i64..=1 {
        let counter = counter + offset;
        if counter < 0 {
            continue;
        }
        let mut mac = <Hmac<Sha256> as HmacKeyInit>::new_from_slice(secret)
            .map_err(|_| anyhow!("invalid TOTP secret"))?;
        mac.update(&(counter as u64).to_be_bytes());
        let digest = mac.finalize().into_bytes();
        let index = usize::from(digest[digest.len() - 1] & 0x0f);
        let binary = (u32::from(digest[index] & 0x7f) << 24)
            | (u32::from(digest[index + 1]) << 16)
            | (u32::from(digest[index + 2]) << 8)
            | u32::from(digest[index + 3]);
        if binary % 1_000_000 == supplied {
            return Ok(true);
        }
    }
    Ok(false)
}

fn random_recovery_code() -> Result<String> {
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let token = random_bytes(20)?
        .into_iter()
        .map(|byte| alphabet[usize::from(byte) % alphabet.len()] as char)
        .collect::<String>();
    Ok(token
        .as_bytes()
        .chunks(5)
        .map(|chunk| String::from_utf8_lossy(chunk))
        .collect::<Vec<_>>()
        .join("-"))
}

fn random_user_code() -> Result<String> {
    let bytes = URL_SAFE_NO_PAD.decode(random_token(6)?)?;
    let alphabet = b"BCDFGHJKLMNPQRSTVWXYZ23456789";
    let code: String = bytes
        .into_iter()
        .take(8)
        .map(|byte| alphabet[usize::from(byte) % alphabet.len()] as char)
        .collect();
    Ok(format!("{}-{}", &code[..4], &code[4..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_storage::operator_from_uri;

    fn main_invitation_json(used: bool) -> serde_json::Value {
        let account_id = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let mut invitation = serde_json::json!({
            "invitation_id": Uuid::now_v7(),
            "token_hash": "token-hash",
            "display_name": "Invited user",
            "space_uid": Uuid::now_v7(),
            "role": "viewer",
            "expires_at": "2099-01-01T00:00:00.000Z",
            "used_at": null,
            "accepted_account_id": null,
            "accepted_principal_id": null,
            "created_by": Uuid::now_v7(),
        });
        if used {
            invitation["used_at"] = serde_json::json!("2026-07-31T00:00:00.000Z");
            invitation["accepted_account_id"] = serde_json::json!(account_id);
            invitation["accepted_principal_id"] = serde_json::json!(principal_id);
        }
        invitation
    }

    #[test]
    fn recovery_session_requires_the_committed_reset_marker_identity() {
        let account_id = Uuid::now_v7();
        let reset_id = Uuid::now_v7();
        let session_id = Uuid::now_v7();
        let session = BrowserSession {
            session_id,
            session_hash: "hash".to_string(),
            credential_id: Uuid::now_v7(),
            assurance: AssuranceLevel::PhishingResistant,
            account_id,
            credential_generation: 2,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            last_seen_at: "2026-01-01T00:00:00Z".to_string(),
            expires_at: "2099-01-01T00:00:00Z".to_string(),
            authenticated_at: "2026-01-01T00:00:00Z".to_string(),
            revoked_at: None,
            recovery_reset_id: Some(reset_id),
            revocation_epoch: 0,
        };
        let mut markers = BTreeMap::new();
        assert!(!recovery_session_is_committed(&session, &markers));
        markers.insert(
            reset_id,
            RecoveryResetMarker {
                reset_id,
                challenge_id: Uuid::now_v7(),
                approval_id: Uuid::now_v7(),
                account_id,
                generation_before: 1,
                generation_after: 2,
                session_id,
                space_authorization_revision: 3,
                recovery_fence_id: Uuid::now_v7(),
                space_uid: Uuid::now_v7(),
                principal_id: Uuid::now_v7(),
                issuer_principal_id: Uuid::now_v7(),
                space_fence_status: "reconciled".to_string(),
                committed_at: "2026-01-01T00:00:00Z".to_string(),
                encrypted_response: None,
                response_delivered_at: None,
                response_delivery_id: None,
                response_invalidated_at: None,
                completion_proof_hash: None,
            },
        );
        assert!(recovery_session_is_committed(&session, &markers));
        let mut loser = session.clone();
        loser.session_id = Uuid::now_v7();
        assert!(!recovery_session_is_committed(&loser, &markers));
    }

    #[test]
    fn invitation_acceptance_states_round_trip() -> Result<()> {
        let account_id = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let states = vec![
            None,
            Some(InvitationAcceptance::Pending {
                account_id,
                principal_id,
                kind: InvitationAcceptanceKind::ExistingAccount,
                claimed_at: "2026-07-31T00:00:00.000Z".to_string(),
                credential_generation: 0,
            }),
            Some(InvitationAcceptance::Completed {
                account_id,
                principal_id,
                kind: InvitationAcceptanceKind::PasskeyRegistration,
                claimed_at: "2026-07-31T00:00:00.000Z".to_string(),
                completed_at: "2026-07-31T00:01:00.000Z".to_string(),
                credential_generation: 0,
            }),
        ];

        for acceptance in states {
            let invitation = AccountInvitation {
                invitation_id: Uuid::now_v7(),
                token_hash: "token-hash".to_string(),
                display_name: "Invited user".to_string(),
                space_uid: Some(Uuid::now_v7()),
                role: Some("viewer".to_string()),
                expires_at: "2099-01-01T00:00:00.000Z".to_string(),
                acceptance,
                created_by: Uuid::now_v7(),
            };
            let encoded = serde_json::to_value(&invitation)?;
            let decoded: AccountInvitation = serde_json::from_value(encoded.clone())?;
            assert_eq!(serde_json::to_value(decoded)?, encoded);
        }
        Ok(())
    }

    #[test]
    fn main_invitation_formats_are_rejected_instead_of_resurrected() -> Result<()> {
        for used in [false, true] {
            let error = serde_json::from_value::<AccountInvitation>(main_invitation_json(used))
                .expect_err("the pre-v1 invitation format must not be accepted");
            assert!(error.to_string().contains("unknown field"), "{error}");
        }
        Ok(())
    }

    #[tokio::test]
    async fn legacy_invitation_rejects_the_entire_node_state() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let mut state = serde_json::to_value(service.read_state().await?)?;
        state["invitations"] = serde_json::json!({
            Uuid::now_v7().to_string(): main_invitation_json(true),
        });

        let error = serde_json::from_value::<NodeState>(state)
            .expect_err("a legacy invitation must not be silently resurrected");
        assert!(error.to_string().contains("unknown field"), "{error}");
        Ok(())
    }

    #[tokio::test]
    async fn setup_secret_is_hashed_and_only_emitted_once() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        let first = service
            .bootstrap_if_needed()
            .await?
            .expect("first bootstrap");
        assert!(service.bootstrap_if_needed().await?.is_none());
        let state = service.read_state().await?;
        let serialized = serde_json::to_string(&state)?;
        assert!(!serialized.contains(&first.setup_secret));
        assert_eq!(
            state.setup.unwrap().token_hash,
            token_hash(&first.setup_secret)
        );
        Ok(())
    }

    #[tokio::test]
    async fn filesystem_node_identity_uses_atomic_control_prefix() -> Result<()> {
        let root = tempfile::tempdir()?;
        let operator = operator_from_uri(root.path().to_str().expect("utf-8 path"))?;
        let store: Arc<dyn NodeControlStore> = Arc::new(OpenDalNodeControlStore::new(operator)?);
        let service = NodeIdentityService::from_parts(
            store,
            Arc::from([0x5a; 32]),
            "localhost",
            "http://localhost:8000",
        )?;
        service.bootstrap_if_needed().await?.expect("bootstrap");
        let pointer_path = root.path().join("_ugoite/node.json");
        assert!(pointer_path.exists());
        let node_id = service.read_state().await?.node_id;
        assert!(root
            .path()
            .join(format!("_ugoite/nodes/{node_id}/state.json"))
            .exists());
        assert!(!root.path().join("spaces/_ugoite").exists());
        assert!(!serde_json::to_string(&service.read_state().await?)?
            .contains(service.encryption_key.as_str()));
        Ok(())
    }

    #[test]
    fn recovery_secret_is_authenticated_encryption() -> Result<()> {
        let sealed = encrypt_recovery_secret("node-key", b"totp-secret")?;
        assert!(!sealed.contains("totp-secret"));
        assert_eq!(
            decrypt_recovery_secret("node-key", &sealed)?,
            b"totp-secret"
        );
        assert!(decrypt_recovery_secret("different-key", &sealed).is_err());
        Ok(())
    }

    #[test]
    fn totp_uses_rfc6238_sha256_and_validates_format() -> Result<()> {
        let secret = b"12345678901234567890123456789012";
        assert!(verify_totp(secret, "119246", 59)?);
        assert!(!verify_totp(secret, "119247", 59)?);
        assert!(!verify_totp(secret, "", 59)?);
        Ok(())
    }

    #[tokio::test]
    async fn totp_enrollment_distinguishes_invalid_codes_from_internal_state_errors() -> Result<()>
    {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "TOTP test".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.recovery.insert(
            account_id,
            RecoveryRecord {
                account_id,
                code_hashes: Vec::new(),
                totp_secret_encrypted: None,
                created_at: timestamp(Utc::now()),
                failed_attempts: 0,
                locked_until: None,
            },
        );
        state.pending_totp_enrollments.insert(
            account_id,
            PendingTotpEnrollment {
                encrypted_secret: encrypt_recovery_secret(
                    &service.encryption_key,
                    b"12345678901234567890123456789012",
                )?,
                expires_at: timestamp(Utc::now() + Duration::minutes(5)),
                credential_generation: 0,
            },
        );
        service.write_state(&state).await?;

        assert!(matches!(
            service.finish_totp_enrollment(account_id, "").await,
            Err(TotpEnrollmentFinishError::InvalidOrExpired)
        ));

        let mut state = service.read_state().await?;
        state
            .pending_totp_enrollments
            .get_mut(&account_id)
            .expect("pending enrollment")
            .encrypted_secret = "corrupt".to_string();
        service.write_state(&state).await?;
        assert!(matches!(
            service.finish_totp_enrollment(account_id, "000000").await,
            Err(TotpEnrollmentFinishError::Internal(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn access_credentials_are_opaque_and_only_hashes_are_persisted() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let now = Utc::now().timestamp();
        let agent_id = Uuid::now_v7();
        let credential = service
            .register_agent_credential(
                agent_id,
                serde_json::json!({
                    "kty": "EC",
                    "crv": "P-256",
                    "x": URL_SAFE_NO_PAD.encode([1_u8; 32]),
                    "y": URL_SAFE_NO_PAD.encode([2_u8; 32])
                }),
                Some(timestamp(Utc::now() + Duration::days(1))),
            )
            .await?;
        let claims = AccessTokenClaims {
            iss: "http://localhost:8000".to_string(),
            node_id: service.read_state().await?.node_id,
            sub: agent_id,
            principal_type: "agent".to_string(),
            actor_principal_id: Some(agent_id),
            aud: "http://localhost:8000".to_string(),
            space_uid: Uuid::now_v7(),
            granted_actions: ["read".to_string()].into_iter().collect(),
            actor_chain: Vec::new(),
            exp: now + 300,
            iat: now,
            jti: Uuid::now_v7(),
            credential_id: credential.credential_id,
            credential_generation: None,
            cnf: crate::oauth::Confirmation {
                jkt: "thumbprint".to_string(),
            },
        };
        let token = service.issue_access_credential(claims.clone()).await?;
        assert!(!token.contains('.'));
        assert_eq!(
            service.resolve_access_credential(&token).await?.sub,
            claims.sub
        );
        assert!(!serde_json::to_string(&service.read_state().await?)?.contains(&token));
        Ok(())
    }

    #[tokio::test]
    async fn browser_sessions_are_listed_without_verifiers_and_revoked_by_id() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let credential_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Session user".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        service.write_state(&state).await?;
        let token = service
            .create_session(
                &state,
                account_id,
                credential_id,
                AssuranceLevel::PhishingResistant,
            )
            .await?;
        let sessions = service.list_sessions(account_id).await?;
        assert_eq!(sessions.len(), 1);
        assert!(!serde_json::to_string(&sessions)?.contains(&token));
        assert!(sessions[0].get("session_hash").is_none());
        let session_id = Uuid::parse_str(sessions[0]["session_id"].as_str().unwrap())?;
        service.revoke_session_by_id(account_id, session_id).await?;
        assert!(service.authenticate_session(&token).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn added_passkeys_use_distinct_discoverable_user_handles() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Passkey user".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        service.write_state(&state).await?;

        let first = service.start_add_passkey(account_id).await?;
        let second = service.start_add_passkey(account_id).await?;
        let first_handle = serde_json::to_value(first.public_key.public_key.user.id)?;
        let second_handle = serde_json::to_value(second.public_key.public_key.user.id)?;

        assert_ne!(first_handle, second_handle);
        assert_ne!(
            first_handle,
            serde_json::Value::String(URL_SAFE_NO_PAD.encode(account_id.as_bytes()))
        );
        Ok(())
    }

    #[tokio::test]
    async fn existing_account_can_accept_invitation_idempotently() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Existing user".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        service.write_state(&state).await?;
        let (_, token) = service
            .issue_invitation(
                account_id,
                "Existing user",
                Some(Uuid::now_v7()),
                Some("viewer".to_string()),
            )
            .await?;
        let (_, first) = service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        service
            .complete_invitation_acceptance(
                first.invitation_id,
                account_id,
                first.accepted_principal_id().expect("accepted principal"),
            )
            .await?;
        let (_, retry) = service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        assert_eq!(first.accepted_principal_id(), retry.accepted_principal_id());
        assert!(matches!(
            retry.acceptance,
            Some(InvitationAcceptance::Completed { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn existing_space_binding_accepts_invitation_without_creating_a_second_principal(
    ) -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Existing owner".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.bindings.push(PrincipalBinding {
            space_uid,
            principal_id,
            node_account_id: account_id,
            binding_method: BindingMethod::Setup,
        });
        service.write_state(&state).await?;
        let (_, token) = service
            .issue_invitation(
                account_id,
                "Invited owner",
                Some(space_uid),
                Some("viewer".to_string()),
            )
            .await?;

        let (_, invitation) = service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        assert_eq!(invitation.accepted_principal_id(), Some(principal_id));
        assert!(matches!(
            invitation.acceptance,
            Some(InvitationAcceptance::Pending {
                account_id: claimed_account_id,
                ..
            }) if claimed_account_id == account_id
        ));

        let (_, retry) = service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        assert_eq!(retry.accepted_principal_id(), Some(principal_id));
        assert_eq!(
            service
                .read_state()
                .await?
                .bindings
                .iter()
                .filter(|binding| binding.space_uid == space_uid)
                .count(),
            1
        );
        Ok(())
    }

    #[tokio::test]
    async fn pending_invitation_claim_cannot_finalize_after_account_reset() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let invitation_id = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Invited member".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.invitations.insert(
            invitation_id,
            AccountInvitation {
                invitation_id,
                token_hash: "stale-invitation".to_string(),
                display_name: "Invited member".to_string(),
                space_uid: None,
                role: Some("viewer".to_string()),
                expires_at: timestamp(Utc::now() + Duration::hours(1)),
                acceptance: Some(InvitationAcceptance::Pending {
                    account_id,
                    principal_id,
                    kind: InvitationAcceptanceKind::ExistingAccount,
                    claimed_at: timestamp(Utc::now()),
                    credential_generation: 0,
                }),
                created_by: account_id,
            },
        );
        service.write_state(&state).await?;
        let mut reset_state = service.read_state().await?;
        reset_state
            .accounts
            .get_mut(&account_id)
            .expect("account")
            .credential_generation = 1;
        service.write_state(&reset_state).await?;

        let error = service
            .complete_invitation_acceptance(invitation_id, account_id, principal_id)
            .await
            .expect_err("a pre-reset invitation claim must be stale");
        assert!(error.to_string().contains("does not match finalization"));
        Ok(())
    }

    #[tokio::test]
    async fn space_invitation_binding_checks_generation_before_node_commit() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let invitation_id = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Invited member".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.invitations.insert(
            invitation_id,
            AccountInvitation {
                invitation_id,
                token_hash: "atomic-invitation".to_string(),
                display_name: "Invited member".to_string(),
                space_uid: Some(space_uid),
                role: Some("viewer".to_string()),
                expires_at: timestamp(Utc::now() + Duration::hours(1)),
                acceptance: Some(InvitationAcceptance::Pending {
                    account_id,
                    principal_id,
                    kind: InvitationAcceptanceKind::ExistingAccount,
                    claimed_at: timestamp(Utc::now()),
                    credential_generation: 0,
                }),
                created_by: account_id,
            },
        );
        service.write_state(&state).await?;
        let mut reset_state = service.read_state().await?;
        reset_state
            .accounts
            .get_mut(&account_id)
            .expect("account")
            .credential_generation = 1;
        service.write_state(&reset_state).await?;

        let error = service
            .finalize_invitation_binding(
                invitation_id,
                account_id,
                principal_id,
                BindingMethod::Invite,
            )
            .await
            .expect_err("stale invitation must not create a Node binding");
        assert!(error.to_string().contains("does not match finalization"));
        let state = service.read_state().await?;
        assert!(!state.bindings.iter().any(|binding| {
            binding.space_uid == space_uid && binding.node_account_id == account_id
        }));
        assert!(matches!(
            state
                .invitations
                .get(&invitation_id)
                .and_then(|invitation| invitation.acceptance.as_ref()),
            Some(InvitationAcceptance::Pending { .. })
        ));

        let mut current_state = service.read_state().await?;
        current_state
            .accounts
            .get_mut(&account_id)
            .expect("account")
            .credential_generation = 0;
        service.write_state(&current_state).await?;
        service
            .finalize_invitation_binding(
                invitation_id,
                account_id,
                principal_id,
                BindingMethod::Invite,
            )
            .await?;
        let state = service.read_state().await?;
        assert!(state.bindings.iter().any(|binding| {
            binding.space_uid == space_uid
                && binding.principal_id == principal_id
                && binding.node_account_id == account_id
        }));
        assert!(matches!(
            state
                .invitations
                .get(&invitation_id)
                .and_then(|invitation| invitation.acceptance.as_ref()),
            Some(InvitationAcceptance::Completed { .. })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn claimed_invitation_principal_is_immutable_and_retry_ignores_expiry() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let other_account_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Existing owner".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.accounts.insert(
            other_account_id,
            HumanAccount {
                account_id: other_account_id,
                display_name: "Other account".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.bindings.push(PrincipalBinding {
            space_uid,
            principal_id,
            node_account_id: account_id,
            binding_method: BindingMethod::Setup,
        });
        service.write_state(&state).await?;
        let (_, token) = service
            .issue_invitation(
                account_id,
                "Invited owner",
                Some(space_uid),
                Some("viewer".to_string()),
            )
            .await?;

        let (_, first) = service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        let claimed_principal = first.accepted_principal_id();
        assert!(service
            .accept_invitation_for_account(&token, other_account_id)
            .await
            .is_err());
        let mut state = service.read_state().await?;
        let invitation = state
            .invitations
            .get_mut(&first.invitation_id)
            .expect("issued invitation");
        invitation.expires_at = "2000-01-01T00:00:00.000Z".to_string();
        service.write_state(&state).await?;

        let (_, retry) = service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        assert_eq!(retry.accepted_principal_id(), claimed_principal);
        Ok(())
    }

    #[tokio::test]
    async fn consumed_invitation_cannot_replace_a_registration_challenge() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Invited user".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        service.write_state(&state).await?;
        let (_, token) = service
            .issue_invitation(
                account_id,
                "Invited user",
                Some(Uuid::now_v7()),
                Some("viewer".to_string()),
            )
            .await?;
        service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        let credential: RegisterPublicKeyCredential = serde_json::from_value(serde_json::json!({
            "id": "invalid",
            "rawId": "aW52YWxpZA",
            "response": {
                "attestationObject": "aW52YWxpZA",
                "clientDataJSON": "aW52YWxpZA"
            },
            "type": "public-key"
        }))?;
        let error = service
            .finish_invitation_registration(&token, Uuid::now_v7(), &credential)
            .await
            .expect_err("a consumed invitation must never mint a browser session");
        assert!(error
            .to_string()
            .contains("unknown or consumed registration challenge"));
        assert!(service.list_sessions(account_id).await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn oidc_subject_links_to_exact_existing_account() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let other_account_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        for id in [account_id, other_account_id] {
            state.accounts.insert(
                id,
                HumanAccount {
                    account_id: id,
                    display_name: format!("User {id}"),
                    status: AccountStatus::Active,
                    created_at: timestamp(Utc::now()),
                    node_roles: BTreeSet::new(),
                    credential_generation: 0,
                },
            );
        }
        service.write_state(&state).await?;
        let issuer = "https://identity.example";
        let subject = "stable-subject";
        let (account, _, _) = service
            .complete_oidc_login(
                issuer,
                subject,
                "ignored",
                None,
                Some(account_id),
                Some(0),
                None,
            )
            .await?;
        assert_eq!(account.account_id, account_id);
        let (invitation, invitation_token) = service
            .issue_invitation(
                account_id,
                "OIDC invite",
                Some(Uuid::now_v7()),
                Some("viewer".to_string()),
            )
            .await?;
        let (_, _, accepted) = service
            .complete_oidc_login(
                issuer,
                subject,
                "ignored",
                Some(&token_hash(&invitation_token)),
                None,
                None,
                None,
            )
            .await?;
        assert_eq!(
            accepted.map(|value| value.invitation_id),
            Some(invitation.invitation_id)
        );
        assert!(service
            .complete_oidc_login(
                issuer,
                subject,
                "ignored",
                None,
                Some(other_account_id),
                Some(0),
                None,
            )
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn authorization_code_is_pkce_bound_and_single_use() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "PKCE user".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        service.write_state(&state).await?;
        let verifier = "a".repeat(64);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let code = service
            .issue_authorization_code(
                "mcp-client",
                "https://client.example/callback",
                &challenge,
                serde_json::json!({
                    "kty": "EC",
                    "crv": "P-256",
                    "x": URL_SAFE_NO_PAD.encode([1_u8; 32]),
                    "y": URL_SAFE_NO_PAD.encode([2_u8; 32])
                }),
                account_id,
                principal_id,
                space_uid,
                ["read".to_string()].into_iter().collect(),
            )
            .await?;
        assert!(service
            .exchange_authorization_code(
                &code,
                "mcp-client",
                "https://client.example/callback",
                "wrong",
            )
            .await
            .is_err());
        service
            .exchange_authorization_code(
                &code,
                "mcp-client",
                "https://client.example/callback",
                &verifier,
            )
            .await?;
        assert!(service
            .exchange_authorization_code(
                &code,
                "mcp-client",
                "https://client.example/callback",
                &verifier,
            )
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn test_req_sec_012_owner_approval_and_backup_rotation_are_one_use() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let issuer_account_id = Uuid::now_v7();
        let target_account_id = Uuid::now_v7();
        let issuer_principal_id = Uuid::now_v7();
        let target_principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let mut state = service.read_state().await?;
        for (account_id, name) in [(issuer_account_id, "Issuer"), (target_account_id, "Target")] {
            state.accounts.insert(
                account_id,
                HumanAccount {
                    account_id,
                    display_name: name.to_string(),
                    status: AccountStatus::Active,
                    created_at: timestamp(Utc::now()),
                    node_roles: BTreeSet::new(),
                    credential_generation: 0,
                },
            );
        }
        state.bindings.extend([
            PrincipalBinding {
                space_uid,
                principal_id: issuer_principal_id,
                node_account_id: issuer_account_id,
                binding_method: BindingMethod::Setup,
            },
            PrincipalBinding {
                space_uid,
                principal_id: target_principal_id,
                node_account_id: target_account_id,
                binding_method: BindingMethod::Invite,
            },
        ]);
        state.recovery.insert(
            target_account_id,
            RecoveryRecord {
                account_id: target_account_id,
                code_hashes: Vec::new(),
                totp_secret_encrypted: None,
                created_at: timestamp(Utc::now()),
                failed_attempts: 0,
                locked_until: None,
            },
        );
        service.write_state(&state).await?;

        let issuer_credential_id = Uuid::now_v7();
        let (approval_id, token, _) = service
            .issue_owner_recovery_approval_with_snapshot_and_credential(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_principal_id,
                issuer_account_id,
                RecoveryBindingSnapshot {
                    request_id: Uuid::now_v7(),
                    recovery_fence_id: Uuid::now_v7(),
                    recovery_fence_expires_at: timestamp(Utc::now() + Duration::minutes(15)),
                    space_authorization_revision: 1,
                    issuer_space_lifecycle_epoch: 1,
                    target_space_lifecycle_epoch: 1,
                    issuer_node_lifecycle_epoch: 0,
                    target_node_lifecycle_epoch: 0,
                    issuer_generation: 0,
                    target_generation: 0,
                },
                Some(issuer_credential_id),
            )
            .await?;
        assert!(token.len() >= 43);
        assert_eq!(
            service.owner_recovery_approval_token(approval_id).await?,
            token
        );
        let approval_state = service.read_state().await?;
        let approval_event = &approval_state
            .recovery_audit_outbox
            .values()
            .next()
            .unwrap()
            .event;
        assert_eq!(
            approval_state
                .recovery_audit_outbox
                .values()
                .next()
                .unwrap()
                .credential_id,
            Some(issuer_credential_id)
        );
        assert_eq!(
            approval_event["actor_principal_id"],
            serde_json::json!(issuer_principal_id)
        );
        assert_eq!(
            approval_event["actor_account_id"],
            serde_json::json!(issuer_account_id)
        );
        assert_eq!(
            approval_event["credential_id"],
            serde_json::json!(issuer_credential_id)
        );
        assert!(serde_json::to_string(approval_event)?
            .find(&token)
            .is_none());
        assert_eq!(
            approval_state
                .recovery_audit_outbox
                .values()
                .next()
                .unwrap()
                .status,
            "pending"
        );
        service
            .mark_recovery_audit_stage(approval_id, "node")
            .await?;
        service
            .mark_recovery_audit_stage(approval_id, "space")
            .await?;
        service.mark_recovery_audit_delivered(approval_id).await?;
        assert_eq!(
            service.read_state().await?.recovery_audit_outbox[&approval_id].status,
            "delivered"
        );
        let first_registration = service.start_owner_recovery_registration(&token).await?;
        let resumed_registration = service.start_owner_recovery_registration(&token).await?;
        assert_eq!(
            resumed_registration.challenge_id,
            first_registration.challenge_id
        );
        assert_eq!(
            serde_json::to_value(resumed_registration.public_key)?,
            serde_json::to_value(first_registration.public_key)?
        );

        let (_, replacement_token, _) = service
            .issue_owner_recovery_approval_unchecked(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_principal_id,
                issuer_account_id,
                None,
                None,
                None,
            )
            .await?;
        assert!(service
            .start_owner_recovery_registration(&token)
            .await
            .is_err());
        let superseded_state = service.read_state().await?;
        assert!(superseded_state
            .recovery_challenge_tombstones
            .values()
            .any(|tombstone| tombstone.reason == "superseded"));
        let superseded_error = service
            .owner_recovery_challenge_context(first_registration.challenge_id)
            .await
            .expect_err("superseded challenges must remain a terminal expiry");
        assert!(superseded_error
            .to_string()
            .contains("owner recovery challenge expired"));
        service
            .start_owner_recovery_registration(&replacement_token)
            .await?;

        let request_id = Uuid::new_v4();
        let rotation_snapshot = RecoveryBindingSnapshot {
            request_id,
            recovery_fence_id: Uuid::now_v7(),
            recovery_fence_expires_at: timestamp(Utc::now() + Duration::minutes(5)),
            space_authorization_revision: 1,
            issuer_space_lifecycle_epoch: 1,
            target_space_lifecycle_epoch: 1,
            issuer_node_lifecycle_epoch: 0,
            target_node_lifecycle_epoch: 0,
            issuer_generation: 0,
            target_generation: 0,
        };
        let codes = service
            .rotate_recovery_codes_with_snapshot_and_credential(
                request_id,
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_principal_id,
                issuer_account_id,
                rotation_snapshot.clone(),
                None,
            )
            .await?;
        assert_eq!(codes.len(), 8);
        let delivered_codes = service.take_backup_rotation_codes(request_id).await?;
        assert_eq!(delivered_codes, codes);
        assert!(service
            .take_backup_rotation_codes(request_id)
            .await
            .expect_err("backup codes must be single-delivery")
            .to_string()
            .contains("already delivered"));
        let mut mismatched_snapshot = rotation_snapshot.clone();
        mismatched_snapshot.target_generation = 1;
        let mismatch = service
            .rotate_recovery_codes_with_snapshot_and_credential(
                request_id,
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_principal_id,
                issuer_account_id,
                mismatched_snapshot,
                None,
            )
            .await
            .expect_err("an idempotency key must not cross generations");
        assert!(mismatch.to_string().contains("key mismatch"));
        let rotation_state = service.read_state().await?;
        let rotation_event = rotation_state
            .recovery_audit_outbox
            .values()
            .find(|record| record.action == "recovery.backup_codes_rotated")
            .expect("rotation outbox event");
        assert!(serde_json::to_string(&rotation_event.event)?
            .find(&codes[0])
            .is_none());
        assert!(service
            .rotate_recovery_codes_with_snapshot_and_credential(
                request_id,
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_principal_id,
                issuer_account_id,
                rotation_snapshot,
                None,
            )
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn expired_owner_approval_is_terminal_and_releases_node_fence() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let issuer_account_id = Uuid::now_v7();
        let target_account_id = Uuid::now_v7();
        let issuer_principal_id = Uuid::now_v7();
        let target_principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let request_id = Uuid::new_v4();
        let fence_id = Uuid::now_v7();
        let snapshot = RecoveryBindingSnapshot {
            request_id,
            recovery_fence_id: fence_id,
            recovery_fence_expires_at: timestamp(Utc::now() + Duration::minutes(15)),
            space_authorization_revision: 1,
            issuer_space_lifecycle_epoch: 1,
            target_space_lifecycle_epoch: 1,
            issuer_node_lifecycle_epoch: 0,
            target_node_lifecycle_epoch: 0,
            issuer_generation: 0,
            target_generation: 0,
        };
        let mut state = service.read_state().await?;
        for (account_id, name) in [(issuer_account_id, "Issuer"), (target_account_id, "Target")] {
            state.accounts.insert(
                account_id,
                HumanAccount {
                    account_id,
                    display_name: name.to_string(),
                    status: AccountStatus::Active,
                    created_at: timestamp(Utc::now()),
                    node_roles: BTreeSet::new(),
                    credential_generation: 0,
                },
            );
        }
        state.bindings.extend([
            PrincipalBinding {
                space_uid,
                principal_id: issuer_principal_id,
                node_account_id: issuer_account_id,
                binding_method: BindingMethod::Setup,
            },
            PrincipalBinding {
                space_uid,
                principal_id: target_principal_id,
                node_account_id: target_account_id,
                binding_method: BindingMethod::Invite,
            },
        ]);
        service.write_state(&state).await?;
        service
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&snapshot),
            )
            .await?;
        let (approval_id, _, _) = service
            .issue_owner_recovery_approval_with_snapshot_and_credential(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_principal_id,
                issuer_account_id,
                snapshot,
                None,
            )
            .await?;
        let mut state = service.read_state().await?;
        state
            .owner_recovery_approvals
            .get_mut(&approval_id)
            .expect("approval")
            .expires_at = timestamp(Utc::now() - Duration::seconds(1));
        service.write_state(&state).await?;

        assert_eq!(
            service.expire_owner_recovery_approval(approval_id).await?,
            Some((space_uid, fence_id))
        );
        let state = service.read_state().await?;
        assert!(state.owner_recovery_approvals[&approval_id]
            .invalidated_at
            .is_some());
        assert_eq!(state.node_recovery_fences[&fence_id].status, "released");
        Ok(())
    }

    #[tokio::test]
    async fn binding_lifecycle_changes_invalidate_pending_recovery_responses() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let marker_id = Uuid::now_v7();
        let rotation_id = Uuid::new_v4();
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "Binding target".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.recovery_reset_markers.insert(
            marker_id,
            RecoveryResetMarker {
                reset_id: marker_id,
                challenge_id: Uuid::now_v7(),
                approval_id: Uuid::now_v7(),
                account_id,
                generation_before: 0,
                generation_after: 1,
                session_id: Uuid::now_v7(),
                space_authorization_revision: 1,
                recovery_fence_id: Uuid::now_v7(),
                space_uid,
                principal_id: Uuid::now_v7(),
                issuer_principal_id: Uuid::now_v7(),
                space_fence_status: "reconciled".to_string(),
                committed_at: timestamp(Utc::now()),
                encrypted_response: Some("encrypted".to_string()),
                response_delivered_at: None,
                response_delivery_id: None,
                response_invalidated_at: None,
                completion_proof_hash: None,
            },
        );
        state.backup_rotation_requests.insert(
            rotation_id,
            BackupRotationRecord {
                request_id: rotation_id,
                space_uid,
                principal_id: Uuid::now_v7(),
                account_id,
                issuer_principal_id: Uuid::now_v7(),
                issuer_account_id: Uuid::now_v7(),
                issuer_credential_id: None,
                target_generation: 0,
                issuer_generation: 0,
                issuer_space_lifecycle_epoch: 0,
                target_space_lifecycle_epoch: 0,
                issuer_node_lifecycle_epoch: 0,
                target_node_lifecycle_epoch: 0,
                space_authorization_revision: 1,
                recovery_fence_id: None,
                space_fence_status: "reconciled".to_string(),
                issued_at: timestamp(Utc::now()),
                code_hashes: vec!["hash".to_string()],
                encrypted_codes: Some("encrypted".to_string()),
                codes_delivered_at: None,
                codes_delivery_id: None,
                codes_invalidated_at: None,
            },
        );
        service.write_state(&state).await?;

        service
            .add_binding(PrincipalBinding {
                space_uid,
                principal_id: Uuid::now_v7(),
                node_account_id: account_id,
                binding_method: BindingMethod::Invite,
            })
            .await?;
        let state = service.read_state().await?;
        assert!(state.recovery_reset_markers[&marker_id]
            .response_invalidated_at
            .is_some());
        assert!(state.backup_rotation_requests[&rotation_id]
            .codes_invalidated_at
            .is_some());
        Ok(())
    }

    #[tokio::test]
    async fn owner_reset_audit_keeps_approver_as_issuer_and_redacts_bearer_actor() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let event_id = Uuid::now_v7();
        let request_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let principal_id = Uuid::now_v7();
        let account_id = Uuid::now_v7();
        let issuer_principal_id = Uuid::now_v7();
        let issuer_account_id = Uuid::now_v7();
        let issuer_credential_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        queue_recovery_audit(
            &mut state,
            event_id,
            "recovery.owner_reset_completed",
            request_id,
            Some(Uuid::now_v7()),
            space_uid,
            principal_id,
            account_id,
            None,
            None,
            None,
            Some(issuer_principal_id),
            Some(issuer_account_id),
            Some(issuer_credential_id),
            serde_json::json!({"credential_generation": 1}),
        );
        let record = state
            .recovery_audit_outbox
            .get(&event_id)
            .expect("queued recovery audit");
        assert_eq!(record.actor_principal_id, None);
        assert_eq!(record.actor_account_id, None);
        assert_eq!(record.actor_credential_id, None);
        assert_eq!(record.issuer_principal_id, Some(issuer_principal_id));
        assert_eq!(record.issuer_account_id, Some(issuer_account_id));
        assert_eq!(record.event["actor_principal_id"], serde_json::Value::Null);
        assert_eq!(record.event["actor_account_id"], serde_json::Value::Null);
        assert_eq!(record.event["credential_id"], serde_json::Value::Null);
        assert_eq!(
            record.event["issuer_credential_id"],
            serde_json::json!(issuer_credential_id)
        );
        Ok(())
    }

    #[tokio::test]
    async fn owner_approval_rejects_a_target_generation_changed_after_issue() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let issuer_account_id = Uuid::now_v7();
        let target_account_id = Uuid::now_v7();
        let issuer_principal_id = Uuid::now_v7();
        let target_principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let mut state = service.read_state().await?;
        for (account_id, name) in [(issuer_account_id, "Issuer"), (target_account_id, "Target")] {
            state.accounts.insert(
                account_id,
                HumanAccount {
                    account_id,
                    display_name: name.to_string(),
                    status: AccountStatus::Active,
                    created_at: timestamp(Utc::now()),
                    node_roles: BTreeSet::new(),
                    credential_generation: 0,
                },
            );
        }
        state.bindings.extend([
            PrincipalBinding {
                space_uid,
                principal_id: issuer_principal_id,
                node_account_id: issuer_account_id,
                binding_method: BindingMethod::Setup,
            },
            PrincipalBinding {
                space_uid,
                principal_id: target_principal_id,
                node_account_id: target_account_id,
                binding_method: BindingMethod::Invite,
            },
        ]);
        service.write_state(&state).await?;
        let (_, token, _) = service
            .issue_owner_recovery_approval_unchecked(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_principal_id,
                issuer_account_id,
                None,
                None,
                None,
            )
            .await?;
        let mut state = service.read_state().await?;
        state
            .accounts
            .get_mut(&target_account_id)
            .expect("target account")
            .credential_generation = 1;
        service.write_state(&state).await?;
        let error = service
            .start_owner_recovery_registration(&token)
            .await
            .expect_err("an approval must not cross a credential generation change");
        assert!(error.to_string().contains("owner approval is stale"));
        Ok(())
    }

    #[tokio::test]
    async fn test_req_sec_013_node_recovery_fence_blocks_status_until_commit() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let issuer_account_id = Uuid::now_v7();
        let target_account_id = Uuid::now_v7();
        let issuer_principal_id = Uuid::now_v7();
        let target_principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let mut state = service.read_state().await?;
        for (account_id, name) in [(issuer_account_id, "Issuer"), (target_account_id, "Target")] {
            state.accounts.insert(
                account_id,
                HumanAccount {
                    account_id,
                    display_name: name.to_string(),
                    status: AccountStatus::Active,
                    created_at: timestamp(Utc::now()),
                    node_roles: BTreeSet::new(),
                    credential_generation: 0,
                },
            );
        }
        state.bindings.extend([
            PrincipalBinding {
                space_uid,
                principal_id: issuer_principal_id,
                node_account_id: issuer_account_id,
                binding_method: BindingMethod::Setup,
            },
            PrincipalBinding {
                space_uid,
                principal_id: target_principal_id,
                node_account_id: target_account_id,
                binding_method: BindingMethod::Invite,
            },
        ]);
        let invitation_id = Uuid::now_v7();
        state.invitations.insert(
            invitation_id,
            AccountInvitation {
                invitation_id,
                token_hash: token_hash("blocked-invitation"),
                display_name: "Blocked invite".to_string(),
                space_uid: Some(space_uid),
                role: Some("viewer".to_string()),
                expires_at: timestamp(Utc::now() + Duration::hours(1)),
                acceptance: None,
                created_by: issuer_account_id,
            },
        );
        service.write_state(&state).await?;
        let snapshot = RecoveryBindingSnapshot {
            request_id: Uuid::now_v7(),
            recovery_fence_id: Uuid::now_v7(),
            recovery_fence_expires_at: timestamp(Utc::now() + Duration::minutes(5)),
            space_authorization_revision: 1,
            issuer_space_lifecycle_epoch: 1,
            target_space_lifecycle_epoch: 1,
            issuer_node_lifecycle_epoch: 0,
            target_node_lifecycle_epoch: 0,
            issuer_generation: 0,
            target_generation: 0,
        };
        service
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&snapshot),
            )
            .await?;
        assert!(service.start_add_passkey(target_account_id).await.is_err());
        assert!(service
            .start_totp_enrollment(target_account_id)
            .await
            .is_err());
        assert!(service
            .start_recovery_registration(target_account_id, "unused", "000000")
            .await
            .is_err());
        assert!(service
            .complete_oidc_login(
                "https://issuer.example",
                "blocked-subject",
                "Blocked",
                None,
                Some(target_account_id),
                Some(0),
                None,
            )
            .await
            .is_err());
        assert!(service
            .issue_invitation(
                issuer_account_id,
                "Blocked invite",
                Some(space_uid),
                Some("viewer".to_string()),
            )
            .await
            .is_err());
        assert!(service
            .start_invitation_registration("blocked-invitation")
            .await
            .is_err());
        assert!(service
            .set_account_status(target_account_id, AccountStatus::Suspended)
            .await
            .is_err());
        service
            .complete_recovery_fence(snapshot.recovery_fence_id)
            .await?;
        service
            .set_account_status(target_account_id, AccountStatus::Suspended)
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn owner_reset_preserves_agents_and_invalidates_human_device_grants() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let agent_id = Uuid::now_v7();
        let device_id = Uuid::now_v7();
        let reset_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        let account = HumanAccount {
            account_id,
            display_name: "Target".to_string(),
            status: AccountStatus::Active,
            created_at: timestamp(Utc::now()),
            node_roles: BTreeSet::new(),
            credential_generation: 0,
        };
        state.accounts.insert(account_id, account.clone());
        let old_session_token = service
            .create_session(
                &state,
                account_id,
                Uuid::now_v7(),
                AssuranceLevel::PhishingResistant,
            )
            .await?;
        state.device_credentials.insert(
            device_id,
            DeviceCredential {
                credential_id: device_id,
                device_name: "cli".to_string(),
                public_key_jwk: serde_json::json!({"kty": "EC"}),
                account_id,
                credential_generation: 0,
                created_at: timestamp(Utc::now()),
                last_used_at: None,
                expires_at: None,
                revoked_at: None,
            },
        );
        state.agent_credentials.insert(
            agent_id,
            AgentCredential {
                credential_id: Uuid::now_v7(),
                agent_id,
                public_key_jwk: serde_json::json!({"kty": "EC"}),
                created_at: timestamp(Utc::now()),
                last_used_at: None,
                expires_at: None,
                revoked_at: None,
            },
        );
        service.write_state(&state).await?;

        let mut state = service.read_state().await?;
        let session_token = service
            .prepare_owner_reset(&mut state, &account, 1, Uuid::now_v7(), reset_id)
            .await?;
        assert!(!session_token.is_empty());
        assert_eq!(state.accounts[&account_id].credential_generation, 1);
        assert!(state.device_credentials[&device_id].revoked_at.is_some());
        assert!(state.agent_credentials[&agent_id].revoked_at.is_none());
        let winner_session_id = service.session_id_for_token(&session_token).await?;
        service
            .revoke_account_sessions_except(
                state.node_id,
                account_id,
                &timestamp(Utc::now()),
                Some(winner_session_id),
            )
            .await?;
        let sessions = service.list_sessions(account_id).await?;
        assert_eq!(sessions.len(), 2);
        assert!(sessions
            .iter()
            .any(|session| session["revoked_at"].is_string()));
        assert!(service
            .authenticate_session(&old_session_token)
            .await
            .is_err());
        Ok(())
    }

    #[tokio::test]
    async fn oidc_invitation_attempt_rejects_a_recovery_generation_change() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let account_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let invitation_id = Uuid::now_v7();
        let provider_id = Uuid::now_v7();
        let token = "oidc-invitation";
        let mut state = service.read_state().await?;
        state.accounts.insert(
            account_id,
            HumanAccount {
                account_id,
                display_name: "OIDC member".to_string(),
                status: AccountStatus::Active,
                created_at: timestamp(Utc::now()),
                node_roles: BTreeSet::new(),
                credential_generation: 0,
            },
        );
        state.bindings.push(PrincipalBinding {
            space_uid,
            principal_id: Uuid::now_v7(),
            node_account_id: account_id,
            binding_method: BindingMethod::Invite,
        });
        state.invitations.insert(
            invitation_id,
            AccountInvitation {
                invitation_id,
                token_hash: token_hash(token),
                display_name: "OIDC member".to_string(),
                space_uid: Some(space_uid),
                role: Some("viewer".to_string()),
                expires_at: timestamp(Utc::now() + Duration::hours(1)),
                acceptance: Some(InvitationAcceptance::Pending {
                    account_id,
                    principal_id: Uuid::now_v7(),
                    kind: InvitationAcceptanceKind::Oidc,
                    claimed_at: timestamp(Utc::now()),
                    credential_generation: 0,
                }),
                created_by: account_id,
            },
        );
        state.oidc_providers.insert(
            provider_id,
            OidcProvider {
                provider_id,
                issuer: "https://issuer.example".to_string(),
                client_id: "client".to_string(),
                client_secret: None,
                enabled: true,
                created_at: timestamp(Utc::now()),
            },
        );
        service.write_state(&state).await?;
        service
            .save_oidc_attempt(
                provider_id,
                "oidc-state",
                "nonce",
                "pkce",
                Some(token),
                None,
            )
            .await?;
        let mut state = service.read_state().await?;
        state
            .oidc_attempts
            .get_mut(&token_hash("oidc-state"))
            .expect("saved OIDC attempt")
            .invitation_account_generation = None;
        state
            .accounts
            .get_mut(&account_id)
            .unwrap()
            .credential_generation = 1;
        service.write_state(&state).await?;
        let error = service
            .consume_oidc_attempt("oidc-state")
            .await
            .expect_err("an OIDC invitation flow must not cross a reset");
        assert!(error
            .to_string()
            .contains("OIDC invitation login attempt is stale"));
        Ok(())
    }

    #[tokio::test]
    async fn pending_owner_reset_marker_is_not_reported_as_completed() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let challenge_id = Uuid::now_v7();
        let approval_id = Uuid::now_v7();
        let reset_id = Uuid::now_v7();
        let mut state = service.read_state().await?;
        state.recovery_challenge_tombstones.insert(
            challenge_id,
            RecoveryChallengeTombstone {
                challenge_id,
                approval_id,
                reset_id,
                reason: "account_reset".to_string(),
                created_at: timestamp(Utc::now()),
            },
        );
        state.recovery_reset_markers.insert(
            reset_id,
            RecoveryResetMarker {
                reset_id,
                challenge_id,
                approval_id,
                account_id: Uuid::now_v7(),
                generation_before: 0,
                generation_after: 1,
                session_id: Uuid::now_v7(),
                space_authorization_revision: 1,
                recovery_fence_id: Uuid::now_v7(),
                space_uid: Uuid::now_v7(),
                principal_id: Uuid::now_v7(),
                issuer_principal_id: Uuid::now_v7(),
                space_fence_status: default_space_fence_status(),
                committed_at: timestamp(Utc::now()),
                encrypted_response: None,
                response_delivered_at: None,
                response_delivery_id: None,
                response_invalidated_at: None,
                completion_proof_hash: None,
            },
        );
        service.write_state(&state).await?;

        let pending = service
            .owner_recovery_challenge_context(challenge_id)
            .await
            .expect_err("a pending Space fence must remain retryable");
        assert!(pending.to_string().contains("RECOVERY_FENCE_UNAVAILABLE"));

        let mut state = service.read_state().await?;
        state
            .recovery_reset_markers
            .get_mut(&reset_id)
            .expect("reset marker")
            .space_fence_status = "reconciled".to_string();
        service.write_state(&state).await?;
        let completed = service
            .owner_recovery_challenge_context(challenge_id)
            .await
            .expect_err("a reconciled reset must be terminal");
        assert!(completed
            .to_string()
            .contains("owner reset already completed"));
        Ok(())
    }

    #[tokio::test]
    async fn expired_node_recovery_fence_cannot_be_completed() -> Result<()> {
        let service = NodeIdentityService::new_for_tests("localhost", "http://localhost:8000")?;
        service.bootstrap_if_needed().await?;
        let issuer_account_id = Uuid::now_v7();
        let target_account_id = Uuid::now_v7();
        let issuer_principal_id = Uuid::now_v7();
        let target_principal_id = Uuid::now_v7();
        let space_uid = Uuid::now_v7();
        let snapshot = RecoveryBindingSnapshot {
            request_id: Uuid::now_v7(),
            recovery_fence_id: Uuid::now_v7(),
            recovery_fence_expires_at: timestamp(Utc::now() + Duration::minutes(5)),
            space_authorization_revision: 1,
            issuer_space_lifecycle_epoch: 1,
            target_space_lifecycle_epoch: 1,
            issuer_node_lifecycle_epoch: 0,
            target_node_lifecycle_epoch: 0,
            issuer_generation: 0,
            target_generation: 0,
        };
        let mut state = service.read_state().await?;
        for account_id in [issuer_account_id, target_account_id] {
            state.accounts.insert(
                account_id,
                HumanAccount {
                    account_id,
                    display_name: "Recovery test".to_string(),
                    status: AccountStatus::Active,
                    created_at: timestamp(Utc::now()),
                    node_roles: BTreeSet::new(),
                    credential_generation: 0,
                },
            );
        }
        state.bindings.extend([
            PrincipalBinding {
                space_uid,
                principal_id: issuer_principal_id,
                node_account_id: issuer_account_id,
                binding_method: BindingMethod::Setup,
            },
            PrincipalBinding {
                space_uid,
                principal_id: target_principal_id,
                node_account_id: target_account_id,
                binding_method: BindingMethod::Invite,
            },
        ]);
        service.write_state(&state).await?;
        service
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&snapshot),
            )
            .await?;
        let mut state = service.read_state().await?;
        state
            .node_recovery_fences
            .get_mut(&snapshot.recovery_fence_id)
            .unwrap()
            .expires_at = timestamp(Utc::now() - Duration::seconds(1));
        service.write_state(&state).await?;
        assert!(service
            .start_add_passkey(target_account_id)
            .await
            .expect_err("an expired but unreconciled fence must still block Node writes")
            .to_string()
            .contains("RECOVERY_FENCE_UNAVAILABLE"));
        assert!(service
            .complete_recovery_fence(snapshot.recovery_fence_id)
            .await
            .is_err());
        let mut state = service.read_state().await?;
        state.recovery_reset_markers.insert(
            Uuid::now_v7(),
            RecoveryResetMarker {
                reset_id: Uuid::now_v7(),
                challenge_id: Uuid::now_v7(),
                approval_id: Uuid::now_v7(),
                account_id: target_account_id,
                generation_before: 0,
                generation_after: 1,
                session_id: Uuid::now_v7(),
                space_authorization_revision: 1,
                recovery_fence_id: snapshot.recovery_fence_id,
                space_uid,
                principal_id: target_principal_id,
                issuer_principal_id,
                space_fence_status: default_space_fence_status(),
                committed_at: timestamp(Utc::now()),
                encrypted_response: None,
                response_delivered_at: None,
                response_delivery_id: None,
                response_invalidated_at: None,
                completion_proof_hash: None,
            },
        );
        service.write_state(&state).await?;
        service
            .abort_recovery_fence_after_space_abort(snapshot.recovery_fence_id)
            .await?;
        let state = service.read_state().await?;
        assert_eq!(
            state.node_recovery_fences[&snapshot.recovery_fence_id].status,
            "released"
        );
        assert!(state
            .recovery_reset_markers
            .values()
            .all(|marker| marker.space_fence_status == "reconciled"));
        let reopened = RecoveryBindingSnapshot {
            recovery_fence_expires_at: timestamp(Utc::now() + Duration::minutes(5)),
            ..snapshot.clone()
        };
        assert!(service
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&reopened),
            )
            .await
            .expect_err("a released Node fence must not be reopened by a stale challenge")
            .to_string()
            .contains("RECOVERY_FENCE_UNAVAILABLE"));
        let provisional = RecoveryBindingSnapshot {
            request_id: Uuid::now_v7(),
            recovery_fence_id: Uuid::now_v7(),
            recovery_fence_expires_at: timestamp(Utc::now() + Duration::minutes(5)),
            space_authorization_revision: 0,
            issuer_space_lifecycle_epoch: 0,
            target_space_lifecycle_epoch: 0,
            issuer_node_lifecycle_epoch: 0,
            target_node_lifecycle_epoch: 0,
            issuer_generation: 0,
            target_generation: 0,
        };
        service
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&provisional),
            )
            .await?;
        assert_eq!(
            service
                .recovery_fence_phase(provisional.recovery_fence_id)
                .await?
                .as_deref(),
            Some("provisional")
        );
        assert_eq!(
            service
                .recovery_fence_for_request(
                    provisional.request_id,
                    space_uid,
                    target_principal_id,
                    target_account_id,
                    issuer_account_id,
                )
                .await?,
            Some(provisional.clone())
        );
        let paired = RecoveryBindingSnapshot {
            space_authorization_revision: 1,
            ..provisional.clone()
        };
        service
            .acquire_recovery_fence(
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
                Some(&paired),
            )
            .await?;
        assert_eq!(
            service
                .recovery_fence_phase(provisional.recovery_fence_id)
                .await?
                .as_deref(),
            Some("paired")
        );
        let found_paired = service
            .recovery_fence_for_request(
                provisional.request_id,
                space_uid,
                target_principal_id,
                target_account_id,
                issuer_account_id,
            )
            .await?
            .expect("paired fence remains discoverable by its request key");
        assert_eq!(
            found_paired.recovery_fence_id,
            provisional.recovery_fence_id
        );
        Ok(())
    }

    #[test]
    fn committed_node_write_errors_are_distinguished_from_unknown_outcomes() {
        assert!(node_write_was_committed_with_ambiguous_response(&anyhow!(
            "node control write committed with an ambiguous response: timeout"
        )));
        assert!(!node_write_was_committed_with_ambiguous_response(&anyhow!(
            "node control write outcome unknown: timeout"
        )));
        assert!(!node_write_was_committed_with_ambiguous_response(&anyhow!(
            "node control write outcome unknown: timeout"
        )));
    }
}
