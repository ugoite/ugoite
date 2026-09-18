//! Config source discovery.
//!
//! Priority (plan sections 16-20):
//! 1. `--config <PATH>` — single-file explicit override, no merging.
//! 2. `<CWD>/.ugoite/config.toml` — project-local, CWD only (no parent walk).
//! 3. `UGOITE_CONFIG` — platform separator-joined list (`:` / `;`).
//! 4. `~/.ugoite/config.toml` — canonical user-global path.
//!
//! Write target (plan section 22):
//! 1. `--config` path
//! 2. project-local path if it already exists
//! 3. first `UGOITE_CONFIG` entry
//! 4. global path

use std::path::{Path, PathBuf};

/// Canonical user-global config path: `~/.ugoite/config.toml`.
pub fn canonical_global_config_path() -> PathBuf {
    home_dir().join(".ugoite").join("config.toml")
}

/// Project-local config path for exactly `cwd` (no parent traversal).
pub fn project_local_config_path(cwd: &Path) -> PathBuf {
    cwd.join(".ugoite").join("config.toml")
}

fn home_dir() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) if !home.trim().is_empty() => PathBuf::from(home),
        _ => PathBuf::from("."),
    }
}

/// Split `UGOITE_CONFIG` on the platform path separator, dropping empties.
pub fn split_env_config_list(value: &str) -> Vec<PathBuf> {
    std::env::split_paths(value)
        .filter(|path| !path.as_os_str().is_empty())
        .collect()
}

/// Pure source-stack builder for tests and the environment reader below.
///
/// `project_exists` / `global_exists` gate inclusion so missing files do not
/// create phantom sources. `env_entries` are already split.
pub fn build_source_stack(
    explicit_config: Option<&Path>,
    project_path: &Path,
    project_exists: bool,
    env_entries: &[PathBuf],
    global_path: &Path,
    global_exists: bool,
) -> Vec<PathBuf> {
    if let Some(explicit) = explicit_config {
        return vec![explicit.to_path_buf()];
    }
    if project_exists {
        let mut stack = vec![project_path.to_path_buf()];
        if !env_entries.is_empty() {
            stack.extend(env_entries.iter().cloned());
        } else if global_exists {
            stack.push(global_path.to_path_buf());
        }
        return stack;
    }
    if !env_entries.is_empty() {
        return env_entries.to_vec();
    }
    if global_exists {
        return vec![global_path.to_path_buf()];
    }
    Vec::new()
}

/// Resolve the effective source list from the live environment.
pub fn source_stack_from_environment(explicit_config: Option<&Path>, cwd: &Path) -> Vec<PathBuf> {
    let project_path = project_local_config_path(cwd);
    let project_exists = project_path.is_file();
    let env_entries = std::env::var("UGOITE_CONFIG")
        .ok()
        .map(|value| split_env_config_list(&value))
        .unwrap_or_default();
    let global_path = canonical_global_config_path();
    let global_exists = global_path.is_file();
    build_source_stack(
        explicit_config,
        &project_path,
        project_exists,
        &env_entries,
        &global_path,
        global_exists,
    )
}

/// Pure write-target resolution.
pub fn resolve_write_target(
    explicit_config: Option<&Path>,
    project_path: &Path,
    project_exists: bool,
    env_entries: &[PathBuf],
    global_path: &Path,
) -> PathBuf {
    if let Some(explicit) = explicit_config {
        return explicit.to_path_buf();
    }
    if project_exists {
        return project_path.to_path_buf();
    }
    if let Some(first) = env_entries.first() {
        return first.clone();
    }
    global_path.to_path_buf()
}

/// Live-environment write target.
pub fn live_write_target(explicit_config: Option<&Path>, cwd: &Path) -> PathBuf {
    let project_path = project_local_config_path(cwd);
    let env_entries = std::env::var("UGOITE_CONFIG")
        .ok()
        .map(|value| split_env_config_list(&value))
        .unwrap_or_default();
    resolve_write_target(
        explicit_config,
        &project_path,
        project_path.is_file(),
        &env_entries,
        &canonical_global_config_path(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(values: &[&str]) -> Vec<PathBuf> {
        values.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn explicit_config_wins_alone() {
        let stack = build_source_stack(
            Some(Path::new("/tmp/test.toml")),
            Path::new("/repo/.ugoite/config.toml"),
            true,
            &paths(&["/env/a.toml"]),
            Path::new("/home/u/.ugoite/config.toml"),
            true,
        );
        assert_eq!(stack, paths(&["/tmp/test.toml"]));
    }

    #[test]
    fn project_plus_env() {
        let stack = build_source_stack(
            None,
            Path::new("/repo/.ugoite/config.toml"),
            true,
            &paths(&["/env/a.toml", "/env/b.toml"]),
            Path::new("/home/u/.ugoite/config.toml"),
            true,
        );
        assert_eq!(
            stack,
            paths(&["/repo/.ugoite/config.toml", "/env/a.toml", "/env/b.toml"])
        );
    }

    #[test]
    fn project_plus_global_without_env() {
        let stack = build_source_stack(
            None,
            Path::new("/repo/.ugoite/config.toml"),
            true,
            &[],
            Path::new("/home/u/.ugoite/config.toml"),
            true,
        );
        assert_eq!(
            stack,
            paths(&["/repo/.ugoite/config.toml", "/home/u/.ugoite/config.toml"])
        );
    }

    #[test]
    fn env_only_without_project() {
        let stack = build_source_stack(
            None,
            Path::new("/repo/.ugoite/config.toml"),
            false,
            &paths(&["/env/a.toml"]),
            Path::new("/home/u/.ugoite/config.toml"),
            true,
        );
        assert_eq!(stack, paths(&["/env/a.toml"]));
    }

    #[test]
    fn write_target_prefers_project() {
        let target = resolve_write_target(
            None,
            Path::new("/repo/.ugoite/config.toml"),
            true,
            &paths(&["/env/a.toml"]),
            Path::new("/home/u/.ugoite/config.toml"),
        );
        assert_eq!(target, PathBuf::from("/repo/.ugoite/config.toml"));
    }

    #[test]
    fn write_target_falls_back_to_env_then_global() {
        let target = resolve_write_target(
            None,
            Path::new("/repo/.ugoite/config.toml"),
            false,
            &paths(&["/env/a.toml"]),
            Path::new("/home/u/.ugoite/config.toml"),
        );
        assert_eq!(target, PathBuf::from("/env/a.toml"));
        let target = resolve_write_target(
            None,
            Path::new("/repo/.ugoite/config.toml"),
            false,
            &[],
            Path::new("/home/u/.ugoite/config.toml"),
        );
        assert_eq!(target, PathBuf::from("/home/u/.ugoite/config.toml"));
    }
}
