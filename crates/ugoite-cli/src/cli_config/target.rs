//! Context-first resolution for Space-bound commands (plan sections 52-54).
//!
//! Migrated commands resolve their Space through the selected context by
//! default and keep the explicit positional as a v0.1.x compatibility path:
//! - `entry get ENTRY_ID` → selected context (new)
//! - `entry get SPACE ENTRY_ID` → legacy explicit Space (compat, unchanged)
//!
//! The compatibility parsing lives in this CLI adapter layer only; domain
//! semantics never branch per connection type. Local and remote keep the
//! same command meaning — only transport and trust boundary differ.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use super::merge::EffectiveConfig;
use super::resolve::{resolve_cli_context, ResolvedConnection};

/// Resolved Space target for one command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceTarget {
    Core { root: String, space_id: String },
    Remote { base: String, space_uid: String },
}

/// Resolve the Space for a Space-bound command.
///
/// - `legacy_space = Some(..)` → v0.1.x compatibility path: the exact legacy
///   resolution (single global endpoint mode) with unchanged error shapes.
/// - `legacy_space = None` → selected context (`--context` override, else
///   `current_context`; never guessed).
pub fn resolve_command_target(
    legacy_space: Option<&str>,
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
    command_name: &str,
) -> Result<SpaceTarget> {
    if let Some(space) = legacy_space {
        return resolve_legacy_target(space, command_name);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let files = super::runtime::load_cli_config(explicit_config, &cwd)?;
    resolve_context_target(&files.effective, context_override)
}

/// Resolve entirely from the effective config (no legacy involved).
pub fn resolve_context_target(
    effective: &EffectiveConfig,
    context_override: Option<&str>,
) -> Result<SpaceTarget> {
    let resolved = resolve_cli_context(effective, context_override)?;
    match resolved.connection {
        ResolvedConnection::Core { root } => {
            let space_id = validate_core_space(&root, &resolved.space_uid).with_context(|| {
                format!(
                    "Context {:?} references Space {}, but that Space could not be found under connection {:?}.\nRoot:\n  {}",
                    resolved.context_name,
                    resolved.space_uid,
                    resolved.connection_name,
                    root.display(),
                )
            })?;
            Ok(SpaceTarget::Core {
                root: root.to_string_lossy().into_owned(),
                space_id,
            })
        }
        ResolvedConnection::Backend { url } | ResolvedConnection::Api { url } => {
            Ok(SpaceTarget::Remote {
                base: url.as_str().trim_end_matches('/').to_string(),
                space_uid: resolved.space_uid.to_string(),
            })
        }
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
    // (Legacy slug-named directories are reachable only through the explicit
    // legacy positional path, never through a context UID.)
    Ok(space_uid.to_string())
}

/// v0.1.x compatibility path: byte-for-byte the legacy resolution behavior.
fn resolve_legacy_target(space: &str, command_name: &str) -> Result<SpaceTarget> {
    let config = crate::config::load_config()?;
    let base = crate::config::validated_base_url(&config)?;
    if base.is_some() {
        let space_uid = crate::config::parse_space_uid(space)
            .with_context(|| format!("{command_name} requires SPACE_UID in backend/api mode"))?;
        return Ok(SpaceTarget::Remote {
            base: base.unwrap_or_default(),
            space_uid,
        });
    }
    let (root, space_id) = crate::config::resolve_space_reference(&config, space, command_name)?;
    Ok(SpaceTarget::Core { root, space_id })
}

/// Split transitional positionals (plan section 54): one value is the bare
/// ID against the selected context; two values are the legacy explicit
/// `(SPACE, ID)` pair. Anything else is a usage error.
pub fn split_space_and_id<'a>(
    values: &'a [String],
    id_label: &str,
    command_name: &str,
) -> Result<(Option<&'a str>, &'a str)> {
    match values {
        [id] => Ok((None, id.as_str())),
        [space, id] => Ok((Some(space.as_str()), id.as_str())),
        _ => bail!("{command_name} requires {id_label} (and optional legacy SPACE first)"),
    }
}

/// Three-positional variant: `(ID, REVISION)` against the context, or legacy
/// `(SPACE, ID, REVISION)`.
pub fn split_space_id_and_revision<'a>(
    values: &'a [String],
    command_name: &str,
) -> Result<(Option<&'a str>, &'a str, &'a str)> {
    match values {
        [id, revision] => Ok((None, id.as_str(), revision.as_str())),
        [space, id, revision] => Ok((Some(space.as_str()), id.as_str(), revision.as_str())),
        _ => {
            bail!("{command_name} requires ENTRY_ID REVISION_ID (and optional legacy SPACE first)")
        }
    }
}

/// Legacy-shaped triple for mechanical migration of existing handlers:
/// `(root, space_id, Option<base>)`. New code should match on
/// [`SpaceTarget`] directly.
pub fn resolve_command_triple(
    legacy_space: Option<&str>,
    explicit_config: Option<&Path>,
    context_override: Option<&str>,
    command_name: &str,
) -> Result<(String, String, Option<String>)> {
    match resolve_command_target(
        legacy_space,
        explicit_config,
        context_override,
        command_name,
    )? {
        SpaceTarget::Remote { base, space_uid } => Ok((String::new(), space_uid, Some(base))),
        SpaceTarget::Core { root, space_id } => Ok((root, space_id, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_two_positionals_prefers_context_for_bare_id() {
        let one = vec!["meeting-001".to_string()];
        assert_eq!(
            split_space_and_id(&one, "ENTRY_ID", "entry get").unwrap(),
            (None, "meeting-001")
        );
        let two = vec!["myspace".to_string(), "meeting-001".to_string()];
        assert_eq!(
            split_space_and_id(&two, "ENTRY_ID", "entry get").unwrap(),
            (Some("myspace"), "meeting-001")
        );
        let none: Vec<String> = vec![];
        assert!(split_space_and_id(&none, "ENTRY_ID", "entry get").is_err());
    }

    #[test]
    fn split_restore_positionals() {
        let two = vec!["e1".to_string(), "r1".to_string()];
        assert_eq!(
            split_space_id_and_revision(&two, "entry restore").unwrap(),
            (None, "e1", "r1")
        );
        let three = vec!["s".to_string(), "e1".to_string(), "r1".to_string()];
        assert_eq!(
            split_space_id_and_revision(&three, "entry restore").unwrap(),
            (Some("s"), "e1", "r1")
        );
    }

    #[test]
    fn core_validation_rejects_missing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let uid = uuid::Uuid::now_v7();
        assert!(validate_core_space(dir.path(), &uid).is_err());
    }
}
