//! Effective config: deterministic merge across the source stack.
//!
//! The stack is ordered high → low priority. For `connections.<name>`,
//! `contexts.<name>`, and `current_context` the first definition wins —
//! object-level, never field-level (plan section 21). A broken source
//! anywhere in the stack fails closed instead of being silently ignored.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::model::ConfigFile;
use super::model::{ConnectionConfig, ContextConfig};

/// A merged value plus the file it came from (for `config current` output).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveValue<T> {
    pub value: T,
    pub source: PathBuf,
}

/// One parsed file with its path.
#[derive(Debug, Clone)]
pub struct LoadedConfigFile {
    pub path: PathBuf,
    pub config: ConfigFile,
}

/// Merged view over the whole source stack.
#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    pub current_context: Option<EffectiveValue<String>>,
    pub connections: BTreeMap<String, EffectiveValue<ConnectionConfig>>,
    pub contexts: BTreeMap<String, EffectiveValue<ContextConfig>>,
    /// All files that participated (high → low priority).
    pub sources: Vec<PathBuf>,
    /// Deterministic mutation target (plan section 22).
    pub write_target: PathBuf,
}

impl EffectiveConfig {
    pub fn context_names(&self) -> Vec<&str> {
        self.contexts.keys().map(String::as_str).collect()
    }

    pub fn connection_names(&self) -> Vec<&str> {
        self.connections.keys().map(String::as_str).collect()
    }
}

/// Load + merge an explicit ordered file list. Missing files are skipped so
/// a not-yet-created global config is not an error; present-but-broken files
/// fail closed.
///
/// NOTE: `--config <PATH>` must NOT use this function: it is a single-file
/// explicit override and a missing file is a usage error. Use
/// [`load_explicit_config_file`] + [`merge_loaded_configs`] for that path.
pub fn load_effective_config(
    ordered_paths: &[PathBuf],
    write_target: PathBuf,
) -> Result<EffectiveConfig> {
    let mut loaded = Vec::new();
    for path in ordered_paths {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                bail!(
                    "cannot load CLI configuration at {}: {error}",
                    path.display()
                )
            }
        };
        let config = ConfigFile::parse_toml(&text, &path.display().to_string())
            .with_context(|| format!("invalid CLI configuration at {}", path.display()))?;
        loaded.push(LoadedConfigFile {
            path: path.clone(),
            config,
        });
    }
    merge_loaded_configs(loaded, write_target)
}

/// Load a single explicit `--config` file. Unlike the source stack, a missing
/// explicit file is a fail-closed error (e.g. typo'd path) rather than an
/// empty effective config.
pub fn load_explicit_config_file(path: &std::path::Path) -> Result<LoadedConfigFile> {
    let text = std::fs::read_to_string(path).with_context(|| {
        format!(
            "cannot load explicit CLI configuration at {}",
            path.display()
        )
    })?;
    let config = ConfigFile::parse_toml(&text, &path.display().to_string())
        .with_context(|| format!("invalid explicit CLI configuration at {}", path.display()))?;
    Ok(LoadedConfigFile {
        path: path.to_path_buf(),
        config,
    })
}

/// Deterministic first-wins merge over already-parsed files.
pub fn merge_loaded_configs(
    loaded: Vec<LoadedConfigFile>,
    write_target: PathBuf,
) -> Result<EffectiveConfig> {
    let mut effective = EffectiveConfig {
        current_context: None,
        connections: BTreeMap::new(),
        contexts: BTreeMap::new(),
        sources: loaded.iter().map(|file| file.path.clone()).collect(),
        write_target,
    };
    for file in &loaded {
        if effective.current_context.is_none() {
            if let Some(current) = file.config.current_context.clone() {
                effective.current_context = Some(EffectiveValue {
                    value: current,
                    source: file.path.clone(),
                });
            }
        }
        for (name, connection) in &file.config.connections {
            effective
                .connections
                .entry(name.clone())
                .or_insert_with(|| EffectiveValue {
                    value: connection.clone(),
                    source: file.path.clone(),
                });
        }
        for (name, context) in &file.config.contexts {
            effective
                .contexts
                .entry(name.clone())
                .or_insert_with(|| EffectiveValue {
                    value: context.clone(),
                    source: file.path.clone(),
                });
        }
    }
    validate_effective(&effective)?;
    Ok(effective)
}

fn validate_effective(effective: &EffectiveConfig) -> Result<()> {
    for (name, context) in &effective.contexts {
        if !effective
            .connections
            .contains_key(&context.value.connection)
        {
            bail!(
                "Context {name:?} references connection {:?}, but that connection is not defined",
                context.value.connection
            );
        }
    }
    if let Some(current) = &effective.current_context {
        if !effective.contexts.contains_key(&current.value) {
            bail!(
                "current_context {:?} does not match any defined context",
                current.value
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn file(
        path: &str,
        current: Option<&str>,
        connections: Vec<(&str, ConnectionConfig)>,
        contexts: Vec<(&str, ContextConfig)>,
    ) -> LoadedConfigFile {
        LoadedConfigFile {
            path: PathBuf::from(path),
            config: ConfigFile {
                version: 1,
                current_context: current.map(str::to_owned),
                connections: connections
                    .into_iter()
                    .map(|(name, value)| (name.to_owned(), value))
                    .collect::<BTreeMap<_, _>>(),
                contexts: contexts
                    .into_iter()
                    .map(|(name, value)| (name.to_owned(), value))
                    .collect::<BTreeMap<_, _>>(),
            },
        }
    }

    fn core(root: &str) -> ConnectionConfig {
        ConnectionConfig::Core {
            root: root.to_owned(),
        }
    }

    fn backend(url: &str) -> ConnectionConfig {
        ConnectionConfig::Backend {
            url: url.to_owned(),
        }
    }

    fn context(connection: &str, uid: &str) -> ContextConfig {
        ContextConfig {
            connection: connection.to_owned(),
            space_uid: uuid::Uuid::parse_str(uid).unwrap(),
            credential: None,
        }
    }

    #[test]
    fn first_object_wins_without_field_merge() {
        let uid_a = "019f1111-1111-7abc-8def-111111111111";
        let uid_b = "019f2222-2222-7abc-8def-222222222222";
        let effective = merge_loaded_configs(
            vec![
                file(
                    "a.toml",
                    Some("personal"),
                    vec![
                        ("local", core("/tmp/root")),
                        ("work", backend("https://a.example.com")),
                    ],
                    vec![("personal", context("local", uid_a))],
                ),
                file(
                    "b.toml",
                    Some("other"),
                    vec![
                        ("local", core("/tmp/root")),
                        ("work", backend("https://b.example.com")),
                    ],
                    vec![("other", context("local", uid_b))],
                ),
            ],
            PathBuf::from("a.toml"),
        )
        .unwrap();
        assert_eq!(
            effective.connections.get("work").unwrap().value,
            backend("https://a.example.com")
        );
        assert_eq!(
            effective.current_context.as_ref().unwrap().value,
            "personal"
        );
    }

    #[test]
    fn invalid_reference_fails_closed() {
        let result = merge_loaded_configs(
            vec![file(
                "a.toml",
                Some("missing"),
                vec![("local", core("/tmp/root"))],
                vec![],
            )],
            PathBuf::from("a.toml"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn context_missing_connection_fails() {
        let result = merge_loaded_configs(
            vec![file(
                "a.toml",
                None,
                vec![],
                vec![(
                    "personal",
                    context("local", "019f1111-1111-7abc-8def-111111111111"),
                )],
            )],
            PathBuf::from("a.toml"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn three_file_priority_is_deterministic() {
        let effective = merge_loaded_configs(
            vec![
                file(
                    "project.toml",
                    None,
                    vec![],
                    vec![(
                        "demo",
                        context("local", "019f1111-1111-7abc-8def-111111111111"),
                    )],
                ),
                file(
                    "global.toml",
                    None,
                    vec![("local", core("/global"))],
                    vec![],
                ),
                file(
                    "company.toml",
                    None,
                    vec![("local", core("/company"))],
                    vec![],
                ),
            ],
            PathBuf::from("project.toml"),
        )
        .unwrap();
        // contexts.demo comes from project; connections.local resolves to the
        // first file that defines it (global), never a field mix.
        assert_eq!(
            effective.contexts.get("demo").unwrap().source,
            PathBuf::from("project.toml")
        );
        assert_eq!(
            effective.connections.get("local").unwrap().source,
            PathBuf::from("global.toml")
        );
    }

    #[test]
    fn explicit_missing_file_is_an_error() {
        let missing = PathBuf::from("/definitely/missing/ugoite-config.toml");
        assert!(super::load_explicit_config_file(&missing).is_err());
    }

    #[test]
    fn default_config_version_is_v1() {
        assert_eq!(ConfigFile::default().version, 1);
        assert_eq!(ConfigFile::empty().version, 1);
    }
}
