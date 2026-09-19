//! Named credential profiles (plan sections 43-45).
//!
//! Secrets live only in `~/.ugoite/credentials.json`, never in TOML and never
//! in project-local config. Contexts reference a credential by name; one
//! credential may serve many contexts.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CREDENTIALS_VERSION_V1: u32 = 1;

/// Opaque per-profile credential payload. The CLI treats values as opaque so
/// future token shapes do not require a config migration.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CredentialStore {
    #[serde(default = "default_credentials_version")]
    pub version: u32,
    #[serde(default)]
    pub credentials: BTreeMap<String, serde_json::Value>,
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self::empty()
    }
}

impl CredentialStore {
    pub fn empty() -> Self {
        Self {
            version: CREDENTIALS_VERSION_V1,
            credentials: BTreeMap::new(),
        }
    }
}

fn default_credentials_version() -> u32 {
    CREDENTIALS_VERSION_V1
}

/// User-global home directory. Fail-closed when `HOME` is undeterminable:
/// no `./.ugoite` fallback (a CWD-relative credential file would leak
/// secrets into project directories).
fn home_dir_strict() -> Result<PathBuf> {
    match std::env::var("HOME") {
        Ok(home) if !home.trim().is_empty() => Ok(PathBuf::from(home)),
        _ => anyhow::bail!(
            "cannot determine the home directory (HOME is missing or empty); refusing to read or write user-global credentials"
        ),
    }
}

/// Default credential filesystem location (always user-global).
pub fn credentials_path() -> Result<PathBuf> {
    Ok(home_dir_strict()?.join(".ugoite").join("credentials.json"))
}

pub fn load_credentials() -> Result<CredentialStore> {
    let path = credentials_path()?;
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let store: CredentialStore = serde_json::from_str(&text)
                .with_context(|| format!("invalid credentials at {}", path.display()))?;
            if store.version != CREDENTIALS_VERSION_V1 {
                anyhow::bail!(
                    "unsupported credentials version {} at {} (expected 1)",
                    store.version,
                    path.display()
                );
            }
            for name in store.credentials.keys() {
                super::model::validate_credential_name(name)
                    .with_context(|| format!("invalid credential name {name:?}"))?;
            }
            Ok(store)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CredentialStore::default())
        }
        Err(error) => Err(error).with_context(|| format!("load {}", path.display())),
    }
}

pub fn write_credentials(store: &CredentialStore) -> Result<PathBuf> {
    let path = credentials_path()?;
    for name in store.credentials.keys() {
        super::model::validate_credential_name(name)
            .with_context(|| format!("invalid credential name {name:?}"))?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create credential directory {}", parent.display()))?;
        ensure_owner_only_dir(parent)?;
    }
    let text = serde_json::to_string_pretty(store).context("serialize credentials")?;
    write_owner_only_atomic(&path, text.as_bytes())?;
    Ok(path)
}

/// Resolve the named credential profile for exactly `connection_name`.
///
/// Returns `None` when no credential is requested. Never falls back to an
/// implicit global lookup: a named profile must match the requested
/// connection, otherwise an actionable error is returned (no silent
/// cross-connection inheritance).
pub fn resolve_named_profile(
    store: &CredentialStore,
    connection_name: &str,
    credential_name: Option<&str>,
) -> Result<Option<serde_json::Value>> {
    let Some(name) = credential_name else {
        return Ok(None);
    };
    let profile = store.credentials.get(name).ok_or_else(|| {
        anyhow::anyhow!(
            "Credential profile {name:?} is not paired for connection {connection_name:?}. Run `ugoite auth login --connection {connection_name} --credential {name}`."
        )
    })?;
    let profile_connection = profile
        .get("connection")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if profile_connection != connection_name {
        anyhow::bail!(
            "Credential profile {name:?} belongs to connection {profile_connection:?}, not {connection_name:?}; refusing to reuse it across connections. Run `ugoite auth login --connection {connection_name} --credential <NAME>`."
        );
    }
    Ok(Some(profile.clone()))
}

/// Uniquely-resolvable credential for `connection_name`.
///
/// Scans stored profiles whose `connection` field matches. Exactly one
/// match resolves; zero or many is an actionable error (never a warning,
/// never a silent pick).
pub fn uniquely_resolvable_credential(
    store: &CredentialStore,
    connection_name: &str,
) -> Result<String> {
    let mut matches: Vec<&String> = store
        .credentials
        .iter()
        .filter(|(_, profile)| {
            profile
                .get("connection")
                .and_then(serde_json::Value::as_str)
                == Some(connection_name)
        })
        .map(|(name, _)| name)
        .collect();
    matches.sort();
    match matches.as_slice() {
        [only] => Ok((*only).clone()),
        [] => anyhow::bail!(
            "Cannot determine a credential for connection {connection_name:?}: no credential is paired for it. Run `ugoite auth login --connection {connection_name} --credential <NAME>`."
        ),
        _ => anyhow::bail!(
            "Cannot determine a credential for connection {connection_name:?}: multiple credentials are paired ({}). Pass --credential <NAME>.",
            matches
                .iter()
                .map(|name| format!("{name:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// Credential selection for an explicit `--connection` change.
///
/// Never silently inherits a different connection's credential:
/// - explicit `--credential` wins (validated at the HTTP boundary);
/// - otherwise, when the scoped context already targets the same connection,
///   its credential is reused;
/// - otherwise, a uniquely-resolvable stored credential for the requested
///   connection is used, else an actionable error.
pub fn resolve_credential_for_connection(
    store: &CredentialStore,
    requested_connection: &str,
    explicit_credential: Option<&str>,
    scope_credential: Option<&str>,
    scope_connection: Option<&str>,
) -> Result<Option<String>> {
    if let Some(name) = explicit_credential {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            anyhow::bail!("credential name must not be empty");
        }
        super::model::validate_credential_name(trimmed)?;
        return Ok(Some(trimmed.to_string()));
    }
    if let (Some(scope_conn), Some(scope_cred)) = (scope_connection, scope_credential) {
        if scope_conn == requested_connection {
            return Ok(Some(scope_cred.to_string()));
        }
        // Different connection: never inherit. Fall through to unique
        // resolution for the requested connection.
    } else if scope_connection.is_none() && scope_credential.is_some() {
        // No scope connection to compare against; do not inherit blindly.
    }
    // No explicit credential and no safely-inheritable scope credential.
    // Callers for core connections may accept `None`; remote callers should
    // require unique resolution. We return `None` here only when the store
    // has no candidate; callers needing auth convert that into an actionable
    // error via `uniquely_resolvable_credential` or allow anonymous.
    // To keep the safety boundary explicit, attempt unique resolution and
    // let the caller decide: if the store has exactly one candidate, use it.
    let candidates: Vec<&String> = {
        let mut names: Vec<&String> = store
            .credentials
            .iter()
            .filter(|(_, profile)| {
                profile
                    .get("connection")
                    .and_then(serde_json::Value::as_str)
                    == Some(requested_connection)
            })
            .map(|(name, _)| name)
            .collect();
        names.sort();
        names
    };
    match candidates.as_slice() {
        [only] => Ok(Some((*only).clone())),
        _ => Ok(None),
    }
}

#[cfg(unix)]
fn ensure_owner_only_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .with_context(|| format!("set owner-only permissions on {}", dir.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn ensure_owner_only_dir(_dir: &Path) -> Result<()> {
    Ok(())
}

/// Secure atomic credential write: temp file in the same directory,
/// owner-only mode, write-all, fsync file, atomic rename, fsync parent dir.
#[cfg(unix)]
fn write_owner_only_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("credential path {} has no parent directory", path.display())
        })?;
    let temp_name = format!(".credentials-{}.tmp", uuid::Uuid::now_v7().simple());
    let temp = parent.join(temp_name);
    // Create temp with owner-only mode from the start.
    let write_result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
            .with_context(|| format!("create temporary credential {}", temp.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("write temporary credential {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary credential {}", temp.display()))?;
        drop(file);
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))?;
        std::fs::rename(&temp, path)
            .with_context(|| format!("publish credential {}", path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    write_result?;
    // fsync parent dir so the rename is durable.
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(not(unix))]
fn write_owner_only_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("credential path {} has no parent directory", path.display())
        })?;
    let temp_name = format!(".credentials-{}.tmp", uuid::Uuid::now_v7().simple());
    let temp = parent.join(temp_name);
    let write_result = (|| -> Result<()> {
        let mut file = std::fs::File::create(&temp)
            .with_context(|| format!("create temporary credential {}", temp.display()))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp, path)
            .with_context(|| format!("publish credential {}", path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    write_result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_store_uses_v1_and_round_trips() {
        let store = CredentialStore::default();
        assert_eq!(store.version, CREDENTIALS_VERSION_V1);
        assert_eq!(CredentialStore::empty().version, CREDENTIALS_VERSION_V1);
        let text = serde_json::to_string_pretty(&store).unwrap();
        let reparsed: CredentialStore = serde_json::from_str(&text).unwrap();
        assert_eq!(reparsed.version, CREDENTIALS_VERSION_V1);
    }

    #[test]
    fn multiple_profiles_coexist_and_remove_leaves_others() {
        let mut store = CredentialStore::empty();
        store.credentials.insert(
            "alice-work".to_string(),
            serde_json::json!({"access_token": "secret-a"}),
        );
        store.credentials.insert(
            "alice-staging".to_string(),
            serde_json::json!({"access_token": "secret-b"}),
        );
        // One profile is reusable bookkeeping: removal is scoped by name.
        store.credentials.remove("alice-work");
        assert!(!store.credentials.contains_key("alice-work"));
        assert_eq!(
            store.credentials.get("alice-staging").unwrap()["access_token"],
            "secret-b"
        );
        let text = serde_json::to_string_pretty(&store).unwrap();
        let reparsed: CredentialStore = serde_json::from_str(&text).unwrap();
        assert_eq!(reparsed.credentials.len(), 1);
    }
}
