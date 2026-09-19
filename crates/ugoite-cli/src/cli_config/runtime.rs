//! Runtime helpers wiring the canonical config into CLI commands.
//!
//! All mutations follow: load stack → build effective → determine write
//! target → materialize full object into the write target → validate merged
//! view → atomic write. Lower-priority files are never edited in place.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::discover::{
    canonical_global_config_path, live_write_target, project_local_config_path,
    source_stack_from_environment,
};
use super::merge::{
    load_effective_config, load_explicit_config_file, merge_loaded_configs, EffectiveConfig,
    LoadedConfigFile,
};
use super::model::ConfigFile;

/// Resolved file set for one CLI invocation.
pub struct CliConfigFiles {
    pub effective: EffectiveConfig,
    pub sources: Vec<PathBuf>,
    pub write_target: PathBuf,
}

/// Load the effective config for a CLI invocation.
///
/// - `explicit_config` is `--config <PATH>` (single-file override).
/// - Otherwise the stack is built from CWD project-local, `UGOITE_CONFIG`,
///   and the user-global file.
pub fn load_cli_config(explicit_config: Option<&Path>, cwd: &Path) -> Result<CliConfigFiles> {
    if let Some(explicit) = explicit_config {
        let loaded = load_explicit_config_file(explicit)?;
        let write_target = explicit.to_path_buf();
        let effective = merge_loaded_configs(vec![loaded.clone()], write_target.clone())?;
        return Ok(CliConfigFiles {
            effective,
            sources: vec![loaded.path],
            write_target,
        });
    }
    let sources = source_stack_from_environment(None, cwd);
    let write_target = live_write_target(None, cwd);
    let effective = load_effective_config(&sources, write_target.clone())?;
    Ok(CliConfigFiles {
        effective,
        sources,
        write_target,
    })
}

/// Read the active write-target file, or an empty v1 file when absent.
pub fn read_write_target_file(path: &Path) -> Result<ConfigFile> {
    match std::fs::read_to_string(path) {
        Ok(text) => ConfigFile::parse_toml(&text, &path.display().to_string())
            .with_context(|| format!("invalid CLI configuration at {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::empty()),
        Err(error) => Err(error)
            .with_context(|| format!("cannot load CLI configuration at {}", path.display())),
    }
}

/// Apply `mutate` to the write-target file, validate the resulting merged
/// view, and publish atomically. Returns the write-target path.
pub fn mutate_write_target(
    files: &CliConfigFiles,
    mutate: impl FnOnce(&mut ConfigFile),
) -> Result<PathBuf> {
    use super::write::write_config_file_atomic;

    let mut target = read_write_target_file(&files.write_target)?;
    // A missing file has no version yet; a present file was already validated.
    if target.version == 0 {
        target = ConfigFile::empty();
    }
    mutate(&mut target);
    target.validate(&files.write_target.display().to_string())?;

    // Re-merge with the mutated write target taking highest priority among
    // stack files to validate the resulting effective configuration.
    let mut reloaded: Vec<LoadedConfigFile> = Vec::new();
    for source in &files.sources {
        if source == &files.write_target {
            reloaded.push(LoadedConfigFile {
                path: source.clone(),
                config: target.clone(),
            });
        } else {
            let text = match std::fs::read_to_string(source) {
                Ok(text) => text,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    anyhow::bail!(
                        "cannot load CLI configuration at {}: {error}",
                        source.display()
                    )
                }
            };
            let config = ConfigFile::parse_toml(&text, &source.display().to_string())?;
            reloaded.push(LoadedConfigFile {
                path: source.clone(),
                config,
            });
        }
    }
    if !files.sources.contains(&files.write_target) {
        reloaded.insert(
            0,
            LoadedConfigFile {
                path: files.write_target.clone(),
                config: target.clone(),
            },
        );
    }
    let _validated =
        merge_loaded_configs(reloaded, files.write_target.clone()).with_context(|| {
            format!(
                "resulting CLI configuration is invalid (write target {})",
                files.write_target.display()
            )
        })?;

    write_config_file_atomic(&files.write_target, &target)?;
    Ok(files.write_target.clone())
}

/// Canonical global path helper for `config init` default messaging.
pub fn global_config_path() -> PathBuf {
    canonical_global_config_path()
}

/// Project-local path helper for `config init --local`.
pub fn local_config_path(cwd: &Path) -> PathBuf {
    project_local_config_path(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_config::model::ConnectionConfig;

    #[test]
    fn empty_write_target_starts_from_v1() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("config.toml");
        let parsed = read_write_target_file(&target).unwrap();
        assert_eq!(parsed.version, 1);
    }

    #[test]
    fn mutate_materializes_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        let global = dir.path().join("config.toml");
        let files = CliConfigFiles {
            effective: EffectiveConfig {
                current_context: None,
                connections: Default::default(),
                contexts: Default::default(),
                sources: vec![],
                write_target: global.clone(),
            },
            sources: vec![],
            write_target: global.clone(),
        };
        let cwd = dir.path();
        mutate_write_target(&files, |config| {
            config.connections.insert(
                "local".to_owned(),
                ConnectionConfig::Core {
                    root: cwd.to_string_lossy().into_owned(),
                },
            );
        })
        .unwrap();
        let text = std::fs::read_to_string(&global).unwrap();
        assert!(text.contains("[connections.local]"));
    }
}
