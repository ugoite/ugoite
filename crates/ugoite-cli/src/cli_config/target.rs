//! Context-first resolution for Space-bound commands (plan sections 52-54).
//!
//! Every Space-bound command resolves its Space through the selected context.
//! Domain semantics never branch per connection type; local and remote keep
//! the same command meaning and differ only in transport and trust boundary.
//!
//! Context path rule: an immutable Space UID resolves to exactly one local
//! directory (`<root>/spaces/<SPACE_UID>`) and the shared domain-owned Space
//! compatibility classifier decides compatibility. Legacy slug-named
//! directories and implicit path/slug discovery are never consulted.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use super::merge::EffectiveConfig;
use super::resolve::resolve_cli_context;

/// Resolved Space target for one command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceTarget {
    Core {
        root: String,
        space_id: String,
    },
    Remote {
        base: String,
        space_uid: String,
        /// Named connection identity used for credential scoping.
        connection: String,
        /// Optional named credential profile identity (never a secret).
        credential: Option<String>,
    },
}

impl SpaceTarget {
    /// Connection identity for context-first remotes.
    pub fn connection_name(&self) -> Option<&str> {
        match self {
            SpaceTarget::Core { .. } => None,
            SpaceTarget::Remote { connection, .. } => Some(connection.as_str()),
        }
    }

    /// Credential profile identity for context-first remotes.
    pub fn credential_name(&self) -> Option<&str> {
        match self {
            SpaceTarget::Core { .. } => None,
            SpaceTarget::Remote { credential, .. } => credential.as_deref(),
        }
    }

    /// Base URL for remotes.
    pub fn base_url(&self) -> Option<&str> {
        match self {
            SpaceTarget::Core { .. } => None,
            SpaceTarget::Remote { base, .. } => Some(base.as_str()),
        }
    }
}

pub fn resolve_command_target(
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
    command_name: &str,
) -> Result<SpaceTarget> {
    resolve_command_target_with_overrides(
        explicit_config,
        context_override,
        None,
        None,
        command_name,
    )
}

/// Override-aware resolution (PR-03 connection safety).
///
/// `connection_override` (`--connection`) and `credential_override`
/// (`--credential`) are explicit per-invocation selections. Empty strings are
/// usage errors (exit 2), never fallbacks. A different connection never
/// silently inherits the scoped context's credential: it uses the explicit
/// credential or the uniquely-resolvable credential for that connection,
/// else an actionable error.
pub fn resolve_command_target_with_overrides(
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
    connection_override: Option<&str>,
    credential_override: Option<&str>,
    command_name: &str,
) -> Result<SpaceTarget> {
    let _ = command_name;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let files = super::runtime::load_cli_config(explicit_config, &cwd)?;
    resolve_context_target_with_overrides(
        &files.effective,
        context_override,
        connection_override,
        credential_override,
    )
}

/// Resolve entirely from the effective config (no legacy involved).
pub fn resolve_context_target(
    effective: &EffectiveConfig,
    context_override: Option<&str>,
) -> Result<SpaceTarget> {
    resolve_context_target_with_overrides(effective, context_override, None, None)
}

/// Override-aware context resolution with connection safety.
pub fn resolve_context_target_with_overrides(
    effective: &EffectiveConfig,
    context_override: Option<&str>,
    connection_override: Option<&str>,
    credential_override: Option<&str>,
) -> Result<SpaceTarget> {
    // Fail-closed on explicit-empty overrides (distinguish unspecified).
    if let Some(name) = connection_override {
        if name.trim().is_empty() {
            return Err(crate::output::UsageError(
                "--connection must not be empty; pass a connection name or omit --connection"
                    .to_string(),
            )
            .into());
        }
    }
    if let Some(name) = credential_override {
        if name.trim().is_empty() {
            return Err(crate::output::UsageError(
                "--credential must not be empty; pass a credential name or omit --credential"
                    .to_string(),
            )
            .into());
        }
    }
    let resolved = resolve_cli_context(effective, context_override)?;
    // Determine effective connection + credential with safety.
    let (connection_name, credential_name) = if let Some(explicit_conn) = connection_override {
        let requested = explicit_conn.trim().to_string();
        super::model::validate_config_name(&requested, "connection").map_err(|error| {
            crate::output::UsageError(format!("invalid --connection {explicit_conn:?}: {error:#}"))
        })?;
        if !effective.connections.contains_key(&requested) {
            bail!("Connection {requested:?} is not defined.");
        }
        let credential = if let Some(explicit_cred) = credential_override {
            let trimmed = explicit_cred.trim().to_string();
            super::model::validate_credential_name(&trimmed).map_err(|error| {
                crate::output::UsageError(format!(
                    "invalid --credential {explicit_cred:?}: {error:#}"
                ))
            })?;
            Some(trimmed)
        } else if resolved.connection_name == requested {
            // Same connection: safely inherit the scoped credential.
            resolved.credential_name.clone()
        } else {
            // Different connection: never inherit. Use the uniquely-resolvable
            // credential for the requested connection, else an actionable
            // error (not a warning).
            match unique_context_credential_for_connection(effective, &requested) {
                Some(name) => Some(name),
                None => {
                    // Fall back to the credential store's unique candidate so
                    // paired-but-unreferenced profiles also resolve; otherwise
                    // error actionably.
                    match super::credentials::load_credentials()
                        .ok()
                        .and_then(|store| {
                            super::credentials::resolve_credential_for_connection(
                                &store,
                                &requested,
                                None,
                                resolved.credential_name.as_deref(),
                                Some(resolved.connection_name.as_str()),
                            )
                            .ok()
                            .flatten()
                        }) {
                        Some(name) => Some(name),
                        None => bail!(
                            "Cannot determine a credential for connection {requested:?}: the current context {:?} targets connection {:?}. Pass --credential <NAME> for connection {requested:?}.",
                            resolved.context_name,
                            resolved.connection_name
                        ),
                    }
                }
            }
        };
        (requested, credential)
    } else if let Some(explicit_cred) = credential_override {
        let trimmed = explicit_cred.trim().to_string();
        super::model::validate_credential_name(&trimmed).map_err(|error| {
            crate::output::UsageError(format!("invalid --credential {explicit_cred:?}: {error:#}"))
        })?;
        (resolved.connection_name.clone(), Some(trimmed))
    } else {
        (
            resolved.connection_name.clone(),
            resolved.credential_name.clone(),
        )
    };
    // Re-resolve the (possibly overridden) connection transport.
    let connection = effective.connections.get(&connection_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Context {:?} references connection {connection_name:?}, but that connection is not defined.",
            resolved.context_name
        )
    })?;
    match &connection.value {
        super::model::ConnectionConfig::Core { root } => {
            if root.trim().is_empty() || root.contains('\0') {
                bail!("Connection {connection_name:?} has an invalid core root");
            }
            // Core needs no credential; drop any inherited profile so local
            // operations never carry remote identity.
            let root_path = PathBuf::from(root);
            let space_id = validate_core_space(&root_path, &resolved.space_uid).with_context(|| {
                format!(
                    "Context {:?} references Space {}, but that Space could not be found under connection {:?}.\nRoot:\n  {}",
                    resolved.context_name,
                    resolved.space_uid,
                    connection_name,
                    root_path.display(),
                )
            })?;
            Ok(SpaceTarget::Core {
                root: root_path.to_string_lossy().into_owned(),
                space_id,
            })
        }
        super::model::ConnectionConfig::Backend { url }
        | super::model::ConnectionConfig::Api { url } => {
            let parsed = super::model::validate_remote_url(url, "Remote endpoint")?;
            Ok(SpaceTarget::Remote {
                base: parsed.as_str().trim_end_matches('/').to_string(),
                space_uid: resolved.space_uid.to_string(),
                connection: connection_name,
                credential: credential_name,
            })
        }
    }
}

/// Distinct credential referenced by contexts targeting `connection_name`.
/// `Some` only when exactly one distinct profile is referenced (unique).
fn unique_context_credential_for_connection(
    effective: &EffectiveConfig,
    connection_name: &str,
) -> Option<String> {
    use std::collections::BTreeSet;
    let mut names = BTreeSet::new();
    for context in effective.contexts.values() {
        if context.value.connection == connection_name {
            if let Some(credential) = &context.value.credential {
                names.insert(credential.clone());
            }
        }
    }
    if names.len() == 1 {
        names.into_iter().next()
    } else {
        None
    }
}

/// Validate `root/spaces/<uid>` against the shared domain-owned Space
/// compatibility classifier. No CLI-local compatibility semantics are
/// introduced here; failures close without guessing another Space.
fn validate_core_space(root: &Path, space_uid: &uuid::Uuid) -> Result<String> {
    let dir = root.join("spaces").join(space_uid.to_string());
    let raw = std::fs::read(dir.join("meta.json"))
        .map_err(|_| anyhow::anyhow!("Space directory {} is missing", dir.display()))?;
    let metadata: serde_json::Value = serde_json::from_slice(&raw)
        .map_err(|_| anyhow::anyhow!("Space metadata at {} is invalid", dir.display()))?;
    ugoite_domain::space::classify_space_version(&metadata).map_err(|error| {
        anyhow::anyhow!(
            "Space at {} is incompatible (detected {:?})",
            dir.display(),
            error.detected()
        )
    })?;
    let stored = metadata
        .get("space_uid")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok())
        .ok_or_else(|| anyhow::anyhow!("Space metadata is missing its immutable space_uid"))?;
    if stored != *space_uid {
        bail!("Space directory and metadata space_uid disagree");
    }
    if space_uid.get_version() != Some(uuid::Version::SortRand) {
        bail!("Context space_uid must be a UUIDv7");
    }
    // The directory name is the authority key; the stored UID must agree.
    Ok(space_uid.to_string())
}

/// Legacy-shaped triple for mechanical migration of existing handlers:
/// `(root, space_id, Option<base>)`. New code should match on
/// [`SpaceTarget`] directly.
pub fn resolve_command_triple(
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
    command_name: &str,
) -> Result<(String, String, Option<String>)> {
    match resolve_command_target(explicit_config, context_override, command_name)? {
        SpaceTarget::Remote {
            base, space_uid, ..
        } => Ok((String::new(), space_uid, Some(base))),
        SpaceTarget::Core { root, space_id } => Ok((root, space_id, None)),
    }
}

/// Override-aware triple for callers that already accept `--connection` /
/// `--credential` but still need the legacy shape.
pub fn resolve_command_triple_with_overrides(
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
    connection_override: Option<&str>,
    credential_override: Option<&str>,
    command_name: &str,
) -> Result<(String, String, Option<String>)> {
    match resolve_command_target_with_overrides(
        explicit_config,
        context_override,
        connection_override,
        credential_override,
        command_name,
    )? {
        SpaceTarget::Remote {
            base, space_uid, ..
        } => Ok((String::new(), space_uid, Some(base))),
        SpaceTarget::Core { root, space_id } => Ok((root, space_id, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_validation_rejects_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let uid = uuid::Uuid::now_v7();
        assert!(validate_core_space(dir.path(), &uid).is_err());
    }
}
