//! Central CLI execution-context resolver (plan section 14).
//!
//! `resolve_cli_context` is the only place that maps
//! `current_context/--context → context → connection → credential → Space`.
//! Space-bound commands must call this instead of implementing their own
//! resolution. No guessing: only explicit `--context`, then
//! `current_context`, else an actionable error.

use anyhow::{bail, Result};
use std::path::PathBuf;

use super::merge::EffectiveConfig;
use super::model::{validate_config_name, validate_remote_url, ConnectionConfig};

/// Resolved transport for one named connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedConnection {
    Core { root: PathBuf },
    Backend { url: url::Url },
    Api { url: url::Url },
}

/// Fully resolved CLI execution context feeding existing Ugoite operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCliContext {
    pub context_name: String,
    pub connection_name: String,
    pub connection: ResolvedConnection,
    pub space_uid: uuid::Uuid,
    pub credential_name: Option<String>,
}

impl ResolvedConnection {
    pub fn transport_label(&self) -> &'static str {
        match self {
            ResolvedConnection::Core { .. } => "core",
            ResolvedConnection::Backend { .. } => "backend",
            ResolvedConnection::Api { .. } => "api",
        }
    }
}

/// Resolve the execution target. Never falls back to first Space/connection,
/// recently used Space, or directory contents.
///
/// Empty `--context` (`""` or whitespace-only) is a usage error (exit 2),
/// never a fallback to `current_context`: unspecified (`None`) and
/// explicit-empty are distinguished.
pub fn resolve_cli_context(
    effective: &EffectiveConfig,
    explicit_context: Option<&str>,
) -> Result<ResolvedCliContext> {
    if let Some(name) = explicit_context {
        if name.trim().is_empty() {
            return Err(crate::output::UsageError(
                "--context must not be empty; pass a context name or omit --context to use the current context".to_string(),
            )
            .into());
        }
        // Validate explicit name shape early so typos fail with an actionable
        // message rather than a generic "not defined".
        let trimmed = name.trim();
        if let Err(error) = validate_config_name(trimmed, "context") {
            return Err(crate::output::UsageError(format!(
                "invalid --context {name:?}: {error:#}"
            ))
            .into());
        }
    }
    let context_name = match explicit_context {
        Some(name) => name.trim().to_owned(),
        None => effective.current_context.as_ref().map(|value| value.value.clone()).ok_or_else(|| {
            anyhow::anyhow!(
                "No Ugoite context is selected.\nCreate a Space:\n  ugoite space create <NAME>\nSelect an existing context:\n  ugoite context use <NAME>\nOr run once with:\n  ugoite --context <NAME> entry list"
            )
        })?,
    };
    let context = effective.contexts.get(&context_name).ok_or_else(|| {
        if explicit_context.is_some() {
            anyhow::anyhow!("Context {context_name:?} is not defined.")
        } else {
            anyhow::anyhow!(
                "Current context {context_name:?} is not defined. Select another context with `ugoite context use <NAME>`."
            )
        }
    })?;
    let connection_name = context.value.connection.clone();
    let connection = effective.connections.get(&connection_name).ok_or_else(|| {
        anyhow::anyhow!(
            "Context {context_name:?} references connection {connection_name:?}, but that connection is not defined."
        )
    })?;
    let resolved = match &connection.value {
        ConnectionConfig::Core { root } => {
            if root.trim().is_empty() || root.contains('\0') {
                bail!("Connection {connection_name:?} has an invalid core root");
            }
            ResolvedConnection::Core {
                root: PathBuf::from(root),
            }
        }
        ConnectionConfig::Backend { url } => ResolvedConnection::Backend {
            url: validate_remote_url(url, "Backend endpoint")?,
        },
        ConnectionConfig::Api { url } => ResolvedConnection::Api {
            url: validate_remote_url(url, "API endpoint")?,
        },
    };
    Ok(ResolvedCliContext {
        context_name,
        connection_name,
        connection: resolved,
        space_uid: context.value.space_uid,
        credential_name: context.value.credential.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_config::merge::{merge_loaded_configs, LoadedConfigFile};
    use crate::cli_config::model::ConfigFile;
    use std::collections::BTreeMap;

    fn effective_from_toml(text: &str) -> EffectiveConfig {
        let config = ConfigFile::parse_toml(text, "test").unwrap();
        merge_loaded_configs(
            vec![LoadedConfigFile {
                path: PathBuf::from("test.toml"),
                config,
            }],
            PathBuf::from("test.toml"),
        )
        .unwrap()
    }

    #[test]
    fn explicit_context_overrides_current() {
        let effective = effective_from_toml(
            r#"
version = 1
current_context = "personal"
[connections.local]
type = "core"
root = "/tmp/root"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
[contexts.other]
connection = "local"
space_uid = "019f2222-2222-7abc-8def-222222222222"
"#,
        );
        let resolved = resolve_cli_context(&effective, Some("other")).unwrap();
        assert_eq!(resolved.context_name, "other");
        assert_eq!(
            resolved.space_uid.to_string(),
            "019f2222-2222-7abc-8def-222222222222"
        );
    }

    #[test]
    fn missing_current_context_errors_without_guessing() {
        let effective = effective_from_toml(
            r#"
version = 1
[connections.local]
type = "core"
root = "/tmp/root"
"#,
        );
        let error = resolve_cli_context(&effective, None).unwrap_err();
        assert!(error.to_string().contains("No Ugoite context is selected"));
    }

    #[test]
    fn missing_connection_reports_config_sources() {
        let mut effective = effective_from_toml(
            r#"
version = 1
current_context = "personal"
[connections.local]
type = "core"
root = "/tmp/root"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
"#,
        );
        effective.connections = BTreeMap::new();
        // Bypass merge validation to test resolver-level reporting.
        let error = resolve_cli_context(&effective, None).unwrap_err();
        assert!(error.to_string().contains("references connection"));
    }
}
