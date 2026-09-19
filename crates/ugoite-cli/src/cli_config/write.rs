//! Atomic config writes + context-name helpers.
//!
//! Mutations follow: load stack → build effective → determine write target →
//! materialize full object → validate merged view → atomic write (plan 48).
//! Lower-priority files are never edited in place; the full effective object
//! is materialized into the active write target (plan 23).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::merge::EffectiveConfig;
use super::model::{serialize_config, ConfigFile};

/// Write `config` atomically via temp-file + rename in the same directory.
///
/// Single canonical write-boundary implementation: temp file in the same
/// directory, write-all, fsync file, atomic rename, fsync parent dir. Never
/// chmods the target directory (project-local `.ugoite/` keeps its existing
/// permissions; `~/.ugoite` ownership is managed by the credential path).
pub fn write_config_file_atomic(path: &Path, config: &ConfigFile) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create config directory {}", parent.display()))?;
        }
    }
    let text = serialize_config(config)?;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    let temp = match parent {
        Some(dir) => tempfile_named_in(dir)?,
        None => tempfile_named_in(Path::new("."))?,
    };
    let write_result = (|| -> Result<()> {
        // O_EXCL: uuid temp names never collide; a hit is a fail-closed error.
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .with_context(|| format!("write temporary config {}", temp.display()))?;
        file.write_all(text.as_bytes())
            .with_context(|| format!("write temporary config {}", temp.display()))?;
        file.sync_all()
            .with_context(|| format!("sync temporary config {}", temp.display()))?;
        drop(file);
        std::fs::rename(&temp, path)
            .with_context(|| format!("publish config {}", path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    write_result?;
    // fsync parent dir so the rename is durable; best-effort on non-unix.
    let parent_dir = parent.unwrap_or(Path::new("."));
    if let Ok(dir) = std::fs::File::open(parent_dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

fn tempfile_named_in(dir: &Path) -> Result<PathBuf> {
    let name = format!(".ugoite-config-{}.tmp", uuid::Uuid::now_v7().simple());
    Ok(dir.join(name))
}

/// Normalize a user-supplied core root to an absolute path for storage.
pub fn normalize_core_root_to_absolute(input: &str, cwd: &Path) -> String {
    let path = Path::new(input.trim());
    if path.is_absolute() {
        return path.to_string_lossy().into_owned();
    }
    cwd.join(path).to_string_lossy().into_owned()
}

/// Deterministic collision-free context name (plan sections 32-33):
/// `<space>`, `<connection>-<space>`, `<connection>-2-<space>`, ...
/// Collision is checked against the whole effective namespace.
/// An empty slug never yields an empty context name: it falls back to the
/// connection name (fail-closed against `validate_config_name`).
pub fn unique_context_name(
    effective: &EffectiveConfig,
    connection_name: &str,
    space_slug: &str,
) -> String {
    let trimmed = space_slug.trim();
    let slug = if trimmed.is_empty() {
        connection_name.trim()
    } else {
        trimmed
    };
    let slug = if slug.is_empty() { "context" } else { slug };
    if !effective.contexts.contains_key(slug) {
        return slug.to_owned();
    }
    let prefixed = format!("{connection_name}-{slug}");
    if !effective.contexts.contains_key(&prefixed) {
        return prefixed;
    }
    let mut counter = 2_u32;
    loop {
        let candidate = format!("{connection_name}-{counter}-{slug}");
        if !effective.contexts.contains_key(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli_config::merge::merge_loaded_configs;
    use crate::cli_config::merge::LoadedConfigFile;
    use crate::cli_config::model::{ConnectionConfig, ContextConfig};
    use std::collections::BTreeMap;

    #[test]
    fn atomic_write_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let config = ConfigFile {
            version: 1,
            current_context: Some("personal".to_owned()),
            connections: BTreeMap::from([(
                "local".to_owned(),
                ConnectionConfig::Core {
                    root: "/tmp/root".to_owned(),
                },
            )]),
            contexts: BTreeMap::from([(
                "personal".to_owned(),
                ContextConfig {
                    connection: "local".to_owned(),
                    space_uid: uuid::Uuid::now_v7(),
                    credential: None,
                },
            )]),
        };
        write_config_file_atomic(&path, &config).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let reparsed = ConfigFile::parse_toml(&text, "roundtrip").unwrap();
        assert_eq!(reparsed, config);
        // No temp files leaked next to the target.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn collision_names_use_connection_prefix() {
        let uid = "019f1111-1111-7abc-8def-111111111111";
        let context = |connection: &str| ContextConfig {
            connection: connection.to_owned(),
            space_uid: uuid::Uuid::parse_str(uid).unwrap(),
            credential: None,
        };
        let effective = merge_loaded_configs(
            vec![LoadedConfigFile {
                path: PathBuf::from("a.toml"),
                config: ConfigFile {
                    version: 1,
                    current_context: None,
                    connections: BTreeMap::from([(
                        "local".to_owned(),
                        ConnectionConfig::Core {
                            root: "/tmp/root".to_owned(),
                        },
                    )]),
                    contexts: BTreeMap::from([
                        ("demo".to_owned(), context("local")),
                        ("local-demo".to_owned(), context("local")),
                        ("local-2-demo".to_owned(), context("local")),
                    ]),
                },
            }],
            PathBuf::from("a.toml"),
        )
        .unwrap();
        assert_eq!(
            unique_context_name(&effective, "local", "demo"),
            "local-3-demo"
        );
    }

    #[test]
    fn relative_root_normalizes_to_absolute() {
        let cwd = Path::new("/Users/alice/research");
        assert_eq!(
            normalize_core_root_to_absolute("data", cwd),
            "/Users/alice/research/data"
        );
        assert_eq!(
            normalize_core_root_to_absolute("/abs/root", cwd),
            "/abs/root"
        );
    }
}
