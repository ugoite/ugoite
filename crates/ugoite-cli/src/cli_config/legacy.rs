//! Legacy (v0.1.x) configuration reader (plan sections 49-51).
//!
//! v0.1.x keeps read compatibility for the legacy endpoint file at the
//! loader boundary: it is normalized to the canonical internal model
//! (`ConfigFile`) for `config migrate` and never propagates legacy shapes
//! into command layers. New writes are canonical TOML only, and the legacy
//! reader is removed in v0.2.
//!
//! The legacy file carries a single global endpoint mode plus backend/API
//! URLs. It knows no connections, contexts, or Space UIDs, so migration
//! produces connections only — contexts are created afterwards by
//! `space create` (automatic) or `context add`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::model::{ConfigFile, ConnectionConfig};

/// Legacy endpoint mode as stored in `cli-endpoints.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyMode {
    Core,
    Backend,
    Api,
}

/// Normalized legacy endpoint configuration.
#[derive(Debug, Clone)]
pub struct LegacyEndpoint {
    pub mode: LegacyMode,
    pub backend_url: String,
    pub api_url: String,
}

/// Legacy file location (same resolution as the legacy loader).
pub fn legacy_config_path() -> PathBuf {
    crate::config::config_path()
}

/// Read the legacy file. `Ok(None)` means absent (nothing to migrate);
/// a present-but-broken file fails closed.
pub fn read_legacy_config() -> Result<Option<LegacyEndpoint>> {
    read_legacy_config_at(&legacy_config_path())
}

fn read_legacy_config_at(path: &Path) -> Result<Option<LegacyEndpoint>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            anyhow::bail!(
                "cannot load legacy CLI configuration at {}: {error}",
                path.display()
            )
        }
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .with_context(|| format!("invalid legacy CLI configuration at {}", path.display()))?;
    if !value.is_object() {
        anyhow::bail!(
            "invalid legacy CLI configuration at {}: expected a JSON object",
            path.display()
        );
    }
    let mode = match value
        .get("mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("core")
    {
        "core" => LegacyMode::Core,
        "backend" => LegacyMode::Backend,
        "api" => LegacyMode::Api,
        other => anyhow::bail!(
            "invalid legacy CLI configuration at {}: unknown mode {other:?}",
            path.display()
        ),
    };
    let backend_url = value
        .get("backend_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("http://localhost:8000")
        .to_string();
    let api_url = value
        .get("api_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("http://localhost:3000/api")
        .to_string();
    Ok(Some(LegacyEndpoint {
        mode,
        backend_url,
        api_url,
    }))
}

/// Normalize a legacy endpoint into a canonical config file.
///
/// `local_root` seeds the `local` core connection only when the legacy mode
/// is core; backend/api connections always carry the stored URLs (validated
/// with the shared endpoint rule). No contexts or current context are
/// invented — Space registration stays with `space create` / `context add`.
pub fn normalize_legacy_to_config_file(
    legacy: &LegacyEndpoint,
    local_root: &Path,
    source_label: &str,
) -> Result<ConfigFile> {
    let mut config = ConfigFile::empty();
    if legacy.mode == LegacyMode::Core {
        config.connections.insert(
            "local".to_string(),
            ConnectionConfig::Core {
                root: super::write::normalize_core_root_to_absolute(
                    &local_root.to_string_lossy(),
                    local_root,
                ),
            },
        );
    }
    config.connections.insert(
        "backend".to_string(),
        ConnectionConfig::Backend {
            url: legacy.backend_url.clone(),
        },
    );
    config.connections.insert(
        "api".to_string(),
        ConnectionConfig::Api {
            url: legacy.api_url.clone(),
        },
    );
    config.validate(source_label)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_legacy_file_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("cli-endpoints.json");
        assert!(read_legacy_config_at(&missing).unwrap().is_none());
    }

    #[test]
    fn broken_legacy_file_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cli-endpoints.json");
        std::fs::write(&path, "{invalid").unwrap();
        assert!(read_legacy_config_at(&path).is_err());
    }

    #[test]
    fn core_legacy_normalizes_with_local_connection() {
        let legacy = LegacyEndpoint {
            mode: LegacyMode::Core,
            backend_url: "http://localhost:8000".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        };
        let config =
            normalize_legacy_to_config_file(&legacy, Path::new("/tmp/work"), "test").unwrap();
        assert!(config.connections.contains_key("local"));
        assert!(config.connections.contains_key("backend"));
        assert!(config.connections.contains_key("api"));
        assert!(config.contexts.is_empty());
        assert!(config.current_context.is_none());
    }

    #[test]
    fn backend_legacy_skips_local_connection() {
        let legacy = LegacyEndpoint {
            mode: LegacyMode::Backend,
            backend_url: "https://ugoite.example.com".to_string(),
            api_url: "https://ugoite.example.com/api".to_string(),
        };
        let config =
            normalize_legacy_to_config_file(&legacy, Path::new("/tmp/work"), "test").unwrap();
        assert!(!config.connections.contains_key("local"));
    }

    #[test]
    fn cleartext_legacy_url_fails_validation() {
        let legacy = LegacyEndpoint {
            mode: LegacyMode::Backend,
            backend_url: "http://ugoite.example.com".to_string(),
            api_url: "http://localhost:3000/api".to_string(),
        };
        assert!(normalize_legacy_to_config_file(&legacy, Path::new("/tmp/work"), "test").is_err());
    }
}
