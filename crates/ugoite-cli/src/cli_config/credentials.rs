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

/// Default credential filesystem location (always user-global).
pub fn credentials_path() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) if !home.trim().is_empty() => PathBuf::from(home),
        _ => PathBuf::from("."),
    }
    .join(".ugoite")
    .join("credentials.json")
}

pub fn load_credentials() -> Result<CredentialStore> {
    let path = credentials_path();
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
            Ok(store)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(CredentialStore::default())
        }
        Err(error) => Err(error).with_context(|| format!("load {}", path.display())),
    }
}

pub fn write_credentials(store: &CredentialStore) -> Result<PathBuf> {
    let path = credentials_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(store).context("serialize credentials")?;
    write_owner_only(&path, text.as_bytes())?;
    Ok(path)
}

#[cfg(unix)]
fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
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
