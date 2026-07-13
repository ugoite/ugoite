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
    display_name: String,
    state: PasskeyRegistration,
    purpose: RegistrationPurpose,
    expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum RegistrationPurpose {
    Setup,
    Invitation { invitation_id: Uuid },
    AddCredential,
    Recovery,
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
    pub created_at: String,
    pub last_seen_at: String,
    pub expires_at: String,
    pub authenticated_at: String,
    pub revoked_at: Option<String>,
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
pub struct AccountInvitation {
    pub invitation_id: Uuid,
    pub token_hash: String,
    pub display_name: String,
    pub space_uid: Option<Uuid>,
    pub role: Option<String>,
    pub expires_at: String,
    pub used_at: Option<String>,
    #[serde(default)]
    pub accepted_account_id: Option<Uuid>,
    #[serde(default)]
    pub accepted_principal_id: Option<Uuid>,
    pub created_by: Uuid,
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
struct PendingTotpEnrollment {
    encrypted_secret: String,
    expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeviceCredential {
    pub credential_id: Uuid,
    pub device_name: String,
    pub public_key_jwk: serde_json::Value,
    pub account_id: Uuid,
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
    pub expires_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RefreshCredential {
    pub refresh_hash: String,
    pub credential_id: Uuid,
    pub account_id: Uuid,
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

#[derive(Debug)]
pub struct RecoveryRegistrationFinish {
    pub account: HumanAccount,
    pub session_id: String,
    pub recovery_codes: Vec<String>,
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
        if input.action.trim().is_empty() || input.target_type.trim().is_empty() {
            bail!("node audit action and target type are required");
        }
        if !input.safe_metadata.is_object() {
            bail!("node audit safe metadata must be an object");
        }
        let state = self.read_state().await?;
        let event = NodeAuditEvent {
            event_id: Uuid::now_v7(),
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
        };
        self.state_store
            .create_if_absent(
                &format!("nodes/{}/audit/{}.json", state.node_id, event.event_id),
                serde_json::to_vec(&event)?,
            )
            .await?;
        Ok(event)
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
            authentication_methods: BTreeMap::new(),
            passkeys: BTreeMap::new(),
            registration_challenges: BTreeMap::new(),
            authentication_challenges: BTreeMap::new(),
            invitations: BTreeMap::new(),
            recovery: BTreeMap::new(),
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
                display_name,
                state: registration,
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
    ) -> Result<RegistrationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let invitation = state
            .invitations
            .values()
            .find(|invitation| invitation.token_hash == token_hash(invitation_token))
            .cloned()
            .ok_or_else(|| anyhow!("invitation is invalid"))?;
        validate_expiry(&invitation.expires_at, "invitation")?;
        if invitation.used_at.is_some() {
            bail!("invitation was already used");
        }
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
                display_name: invitation.display_name,
                state: registration,
                purpose: RegistrationPurpose::Invitation {
                    invitation_id: invitation.invitation_id,
                },
                expires_at: timestamp(Utc::now() + Duration::minutes(CHALLENGE_LIFETIME_MINUTES)),
            },
        );
        self.write_state(&state).await?;
        Ok(RegistrationStart {
            challenge_id,
            public_key,
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
            .remove(&challenge_id)
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
        if invitation.token_hash != token_hash(invitation_token) || invitation.used_at.is_some() {
            bail!("invitation is invalid or used");
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
        invitation.used_at = Some(timestamp(Utc::now()));
        invitation.accepted_account_id = Some(account.account_id);
        invitation.accepted_principal_id = Some(Uuid::now_v7());
        let invitation = invitation.clone();
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
        let account = state
            .accounts
            .get(&account_id)
            .filter(|account| matches!(account.status, AccountStatus::Active))
            .cloned()
            .ok_or_else(|| anyhow!("account is not active"))?;
        let excludes = state
            .passkeys
            .values()
            .filter(|credential| credential.account_id == account_id)
            .map(|credential| credential.passkey.cred_id().clone())
            .collect::<Vec<_>>();
        let (mut public_key, registration) = self.webauthn.start_passkey_registration(
            account_id,
            &account_id.to_string(),
            &account.display_name,
            Some(excludes),
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
                display_name: account.display_name,
                state: registration,
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
        validate_expiry(&challenge.expires_at, "registration challenge")?;
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &challenge.state)?;
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        if state.passkeys.contains_key(&credential_id) {
            bail!("credential is already registered");
        }
        let now = timestamp(Utc::now());
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

    pub async fn revoke_passkey(&self, account_id: Uuid, credential_id: &str) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
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
        state.passkeys.remove(credential_id);
        self.write_state(&state).await
    }

    pub async fn start_totp_enrollment(&self, account_id: Uuid) -> Result<serde_json::Value> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
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
            },
        );
        let label = format!("Ugoite:{}", account.account_id);
        let uri = format!(
            "otpauth://totp/{label}?secret={encoded}&issuer=Ugoite&algorithm=SHA256&digits=6&period=30"
        );
        self.write_state(&state).await?;
        Ok(serde_json::json!({"secret": encoded, "otpauth_uri": uri}))
    }

    pub async fn finish_totp_enrollment(&self, account_id: Uuid, code: &str) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        let pending = state
            .pending_totp_enrollments
            .get(&account_id)
            .cloned()
            .ok_or_else(|| anyhow!("TOTP enrollment is not pending"))?;
        validate_expiry(&pending.expires_at, "TOTP enrollment")?;
        let secret = decrypt_recovery_secret(&self.encryption_key, &pending.encrypted_secret)?;
        if !verify_totp(&secret, code, Utc::now().timestamp())? {
            bail!("invalid TOTP code");
        }
        state.pending_totp_enrollments.remove(&account_id);
        let recovery = state
            .recovery
            .get_mut(&account_id)
            .ok_or_else(|| anyhow!("recovery record not found"))?;
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
        self.write_state(&state).await
    }

    pub async fn start_recovery_registration(
        &self,
        account_id: Uuid,
        recovery_code: &str,
        totp_code: &str,
    ) -> Result<RegistrationStart> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
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

        let excludes = state
            .passkeys
            .values()
            .filter(|credential| credential.account_id == account_id)
            .map(|credential| credential.passkey.cred_id().clone())
            .collect::<Vec<_>>();
        let (mut public_key, registration) = self.webauthn.start_passkey_registration(
            account_id,
            &account_id.to_string(),
            &account.display_name,
            Some(excludes),
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
                display_name: account.display_name,
                state: registration,
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
        let passkey = self
            .webauthn
            .finish_passkey_registration(credential, &challenge.state)
            .context("verify recovery Passkey registration")?;
        let credential_id = URL_SAFE_NO_PAD.encode(passkey.cred_id());
        if state.passkeys.contains_key(&credential_id) {
            bail!("credential is already registered");
        }
        let now = timestamp(Utc::now());
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
        })
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
        let state = self.read_state().await?;
        let key = session_key(state.node_id, &token_hash(session_token));
        let Some(record) = self.state_store.get(&key).await? else {
            return Ok(());
        };
        let mut session: BrowserSession = serde_json::from_slice(&record.value)?;
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
        let state = self.read_state().await?;
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
        let token = random_token(32)?;
        let invitation_id = Uuid::now_v7();
        let invitation = AccountInvitation {
            invitation_id,
            token_hash: token_hash(&token),
            display_name: normalized_display_name(display_name)?,
            space_uid,
            role,
            expires_at: timestamp(Utc::now() + Duration::hours(INVITATION_LIFETIME_HOURS)),
            used_at: None,
            accepted_account_id: None,
            accepted_principal_id: None,
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
        let invitation = state
            .invitations
            .get_mut(&invitation_id)
            .ok_or_else(|| anyhow!("invitation is invalid"))?;
        validate_expiry(&invitation.expires_at, "invitation")?;
        if invitation.used_at.is_some() {
            if invitation.accepted_account_id == Some(account_id)
                && invitation.accepted_principal_id.is_some()
            {
                return Ok((account, invitation.clone()));
            }
            bail!("invitation is invalid");
        }
        invitation.used_at = Some(timestamp(Utc::now()));
        invitation.accepted_account_id = Some(account_id);
        invitation.accepted_principal_id = Some(Uuid::now_v7());
        let invitation = invitation.clone();
        self.write_state(&state).await?;
        Ok((account, invitation))
    }

    pub async fn add_binding(&self, binding: PrincipalBinding) -> Result<()> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
        if state.bindings.iter().any(|candidate| candidate == &binding) {
            return Ok(());
        }
        if state.bindings.iter().any(|candidate| {
            candidate.space_uid == binding.space_uid
                && (candidate.principal_id == binding.principal_id
                    || candidate.node_account_id == binding.node_account_id)
        }) {
            bail!("principal or account is already bound in this space");
        }
        state.bindings.push(binding);
        self.write_state(&state).await
    }

    pub async fn principal_for_account(&self, space_uid: Uuid, account_id: Uuid) -> Result<Uuid> {
        let state = self.read_state().await?;
        state
            .bindings
            .iter()
            .find(|binding| binding.space_uid == space_uid && binding.node_account_id == account_id)
            .map(|binding| binding.principal_id)
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
        request.used_at = Some(timestamp(Utc::now()));
        let credential_id = Uuid::now_v7();
        let credential = DeviceCredential {
            credential_id,
            device_name: request.device_name.clone(),
            public_key_jwk: request.public_key_jwk.clone(),
            account_id,
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
        state
            .device_credentials
            .get(&credential_id)
            .filter(|credential| credential.revoked_at.is_none())
            .cloned()
            .ok_or_else(|| anyhow!("device credential is missing or revoked"))
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
                .filter(|account| matches!(account.status, AccountStatus::Active))
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
            if device.revoked_at.is_some() {
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
        state.oidc_attempts.insert(
            token_hash(state_token),
            OidcLoginAttempt {
                state_hash: token_hash(state_token),
                provider_id,
                nonce: nonce.to_string(),
                pkce_verifier: pkce_verifier.to_string(),
                invitation_hash: invitation_token.map(token_hash),
                link_account_id,
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
    ) -> Result<(HumanAccount, String, Option<AccountInvitation>)> {
        let _guard = self.state_lock.lock().await;
        let mut state = self.read_state().await?;
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
            (account, None)
        } else if let Some(account_id) = existing_account {
            let account = state
                .accounts
                .get(&account_id)
                .filter(|account| matches!(account.status, AccountStatus::Active))
                .cloned()
                .ok_or_else(|| anyhow!("OIDC account is not active"))?;
            let invitation = if let Some(invitation_hash) = invitation_hash {
                let invitation = state
                    .invitations
                    .values_mut()
                    .find(|invitation| invitation.token_hash == invitation_hash)
                    .ok_or_else(|| anyhow!("invitation is invalid"))?;
                validate_expiry(&invitation.expires_at, "invitation")?;
                if invitation.used_at.is_some()
                    && invitation.accepted_account_id != Some(account_id)
                {
                    bail!("invitation is invalid");
                }
                if invitation.used_at.is_none() {
                    invitation.used_at = Some(timestamp(Utc::now()));
                    invitation.accepted_account_id = Some(account_id);
                    invitation.accepted_principal_id = Some(Uuid::now_v7());
                }
                Some(invitation.clone())
            } else {
                None
            };
            (account, invitation)
        } else {
            let invitation_hash =
                invitation_hash.ok_or_else(|| anyhow!("new OIDC users require an invitation"))?;
            let invitation = state
                .invitations
                .values_mut()
                .find(|invitation| invitation.token_hash == invitation_hash)
                .ok_or_else(|| anyhow!("invitation is invalid"))?;
            validate_expiry(&invitation.expires_at, "invitation")?;
            if invitation.used_at.is_some() {
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
            };
            invitation.used_at = Some(timestamp(Utc::now()));
            invitation.accepted_account_id = Some(account.account_id);
            invitation.accepted_principal_id = Some(Uuid::now_v7());
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
        state
            .bindings
            .iter()
            .find(|binding| binding.space_uid == space_uid && binding.principal_id == principal_id)
            .map(|binding| binding.node_account_id)
            .ok_or_else(|| anyhow!("principal has no Node account binding"))
    }

    async fn create_session(
        &self,
        state: &NodeState,
        account_id: Uuid,
        credential_id: Uuid,
        assurance: AssuranceLevel,
    ) -> Result<String> {
        let session_token = random_token(32)?;
        let now = Utc::now();
        let now_text = timestamp(now);
        let hash = token_hash(&session_token);
        let session = BrowserSession {
            session_id: Uuid::now_v7(),
            session_hash: hash.clone(),
            credential_id,
            assurance,
            account_id,
            created_at: now_text.clone(),
            last_seen_at: now_text.clone(),
            expires_at: timestamp(now + Duration::days(SESSION_ABSOLUTE_DAYS)),
            authenticated_at: now_text,
            revoked_at: None,
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
        let prefix = format!("nodes/{node_id}/sessions");
        for key in self.state_store.list_prefix(&prefix).await? {
            let Some(record) = self.state_store.get(&key).await? else {
                continue;
            };
            let mut session: BrowserSession = serde_json::from_slice(&record.value)?;
            if session.account_id != account_id || session.revoked_at.is_some() {
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
            self.state_store
                .compare_and_swap(&state_key, version, bytes)
                .await?;
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
        let (_, retry) = service
            .accept_invitation_for_account(&token, account_id)
            .await?;
        assert_eq!(first.accepted_principal_id, retry.accepted_principal_id);
        assert!(first.used_at.is_some());
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
                },
            );
        }
        service.write_state(&state).await?;
        let issuer = "https://identity.example";
        let subject = "stable-subject";
        let (account, _, _) = service
            .complete_oidc_login(issuer, subject, "ignored", None, Some(account_id))
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
            )
            .await?;
        assert_eq!(
            accepted.map(|value| value.invitation_id),
            Some(invitation.invitation_id)
        );
        assert!(service
            .complete_oidc_login(issuer, subject, "ignored", None, Some(other_account_id))
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
}
