//! Portable identity and authorization types stored in Node or Space state.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Active,
    Suspended,
    Revoked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HumanAccount {
    pub account_id: Uuid,
    pub display_name: String,
    pub status: AccountStatus,
    pub created_at: String,
    #[serde(default)]
    pub node_roles: BTreeSet<NodeRole>,
    /// Monotonic credential epoch. A reset advances it so serialized and
    /// remotely stored human credentials can be rejected without rewriting
    /// every credential object first.
    #[serde(default)]
    pub credential_generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeRole {
    NodeAdmin,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationMethodKind {
    Passkey,
    Oidc,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthenticationMethod {
    pub method_id: Uuid,
    pub account_id: Uuid,
    pub kind: AuthenticationMethodKind,
    /// OIDC identities use the canonical `issuer\nsubject` tuple, never email.
    pub external_subject: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WebAuthnCredential {
    pub credential_id: String,
    pub account_id: Uuid,
    /// Serialized credential material produced by the WebAuthn verifier.
    pub public_key: String,
    pub sign_count: u32,
    #[serde(default)]
    pub transports: Vec<String>,
    pub backup_eligible: bool,
    pub backup_state: bool,
    pub rp_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Agent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalState {
    Invited,
    Active,
    Suspended,
    Revoked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpacePrincipal {
    pub principal_id: Uuid,
    pub kind: PrincipalKind,
    pub display_name: String,
    pub state: PrincipalState,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceRole {
    Owner,
    Editor,
    Viewer,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Membership {
    pub principal_id: Uuid,
    pub role: SpaceRole,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingMethod {
    /// The current setup binding also decodes the pre-v1 owner-rebind value.
    /// Serialization always emits the canonical current value.
    #[serde(alias = "migration")]
    Setup,
    Invite,
    Oidc,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrincipalBinding {
    pub space_uid: Uuid,
    pub principal_id: Uuid,
    pub node_account_id: Uuid,
    pub binding_method: BindingMethod,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticatedSubject {
    HumanAccount { account_id: Uuid },
    SpacePrincipal { principal_id: Uuid },
    AgentPrincipal { agent_id: Uuid },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Human { account_id: Uuid },
    CliDevice { credential_id: Uuid },
    Agent { agent_id: Uuid },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestAuthenticationMethod {
    Passkey,
    Oidc,
    DeviceProof,
    AgentAssertion,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssuranceLevel {
    PhishingResistant,
    Federated,
    Possession,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CredentialConstraints {
    pub issuer: Option<String>,
    pub node_id: Option<Uuid>,
    pub audience: Option<String>,
    pub space_id: Option<Uuid>,
    #[serde(default)]
    pub actions: BTreeSet<Action>,
    pub expires_at: Option<String>,
    pub confirmation_key_thumbprint: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RequestIdentity {
    pub subject: AuthenticatedSubject,
    pub actor: Actor,
    pub credential_id: Uuid,
    pub authentication_method: RequestAuthenticationMethod,
    pub assurance: AssuranceLevel,
    pub constraints: CredentialConstraints,
    pub session_id: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Read,
    Create,
    Update,
    Delete,
    Share,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Grant {
    pub principal_id: Uuid,
    pub actions: BTreeSet<Action>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AccessPolicy {
    pub policy_id: Uuid,
    #[serde(default = "default_inherit_space_role")]
    pub inherit_space_role: bool,
    #[serde(default)]
    pub grants: Vec<Grant>,
}

const fn default_inherit_space_role() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AgentPrincipal {
    pub agent_id: Uuid,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    pub sponsor_principal_id: Uuid,
    pub owner_principal_ids: BTreeSet<Uuid>,
    #[serde(default)]
    pub mode: AgentMode,
    pub status: PrincipalState,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub last_used_at: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    #[default]
    Autonomous,
    Delegated,
    Both,
}

impl AgentMode {
    pub fn allows_autonomous(&self) -> bool {
        matches!(self, Self::Autonomous | Self::Both)
    }

    pub fn allows_delegated(&self) -> bool {
        matches!(self, Self::Delegated | Self::Both)
    }
}

impl AgentPrincipal {
    pub fn validate(&self) -> Result<()> {
        if self.owner_principal_ids.is_empty() {
            bail!("agent must have at least one human owner");
        }
        if !self
            .owner_principal_ids
            .contains(&self.sponsor_principal_id)
        {
            bail!("agent sponsor must be one of its owners");
        }
        Ok(())
    }
}

pub fn oidc_external_subject(issuer: &str, subject: &str) -> Result<String> {
    let issuer = issuer.trim().trim_end_matches('/');
    if issuer.is_empty() || subject.is_empty() || issuer.contains('\n') || subject.contains('\n') {
        bail!("OIDC issuer and subject must be non-empty single-line values");
    }
    Ok(format!("{issuer}\n{subject}"))
}

pub fn role_actions(role: &SpaceRole) -> BTreeSet<Action> {
    match role {
        SpaceRole::Owner => vec![
            Action::Read,
            Action::Create,
            Action::Update,
            Action::Delete,
            Action::Share,
        ],
        SpaceRole::Editor => vec![Action::Read, Action::Create, Action::Update],
        SpaceRole::Viewer => vec![Action::Read],
    }
    .into_iter()
    .collect()
}

pub fn evaluate_policy(
    inherited: &BTreeSet<Action>,
    principal_id: Uuid,
    policy: Option<&AccessPolicy>,
) -> BTreeSet<Action> {
    let mut effective = if policy.is_some_and(|policy| !policy.inherit_space_role) {
        BTreeSet::new()
    } else {
        inherited.clone()
    };
    if let Some(policy) = policy {
        for grant in policy
            .grants
            .iter()
            .filter(|g| g.principal_id == principal_id)
        {
            effective.extend(grant.actions.iter().cloned());
        }
    }
    effective
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oidc_identity_is_issuer_subject_not_email() {
        assert_eq!(
            oidc_external_subject("https://id.example/", "user-42").unwrap(),
            "https://id.example\nuser-42"
        );
    }

    #[test]
    fn oidc_subject_is_not_normalized_into_another_identity() {
        assert_ne!(
            oidc_external_subject("https://id.example", "user-42").unwrap(),
            oidc_external_subject("https://id.example", " user-42").unwrap(),
        );
    }

    #[test]
    fn explicit_grant_extends_space_role() {
        let principal_id = Uuid::from_u128(1);
        let inherited = role_actions(&SpaceRole::Owner);
        let policy = AccessPolicy {
            policy_id: Uuid::from_u128(2),
            inherit_space_role: true,
            grants: vec![Grant {
                principal_id,
                actions: [Action::Share].into_iter().collect(),
            }],
        };
        let effective = evaluate_policy(&inherited, principal_id, Some(&policy));
        assert!(effective.contains(&Action::Read));
        assert!(effective.contains(&Action::Delete));
        assert!(effective.contains(&Action::Update));
        assert!(effective.contains(&Action::Share));
    }

    #[test]
    fn old_owner_rebind_binding_decodes_to_current_setup_binding() {
        let binding: PrincipalBinding = serde_json::from_value(serde_json::json!({
            "space_uid": Uuid::from_u128(1),
            "principal_id": Uuid::from_u128(2),
            "node_account_id": Uuid::from_u128(3),
            "binding_method": "migration"
        }))
        .unwrap();

        assert_eq!(binding.binding_method, BindingMethod::Setup);
        assert_eq!(
            serde_json::to_value(binding).unwrap()["binding_method"],
            "setup"
        );
    }
}
