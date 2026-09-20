use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthSession {
    pub credential_id: uuid::Uuid,
    pub device_name: String,
    pub public_key_jwk: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_pkcs8: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub base_url: String,
    pub resource: Option<String>,
    pub space_uid: uuid::Uuid,
}

pub fn auth_session_path() -> PathBuf {
    crate::cli_config::canonical_global_config_path()
        .parent()
        .unwrap_or(Path::new("."))
        .join("cli-credentials.json")
}

fn non_empty_string(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn non_empty_env_value(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(non_empty_string)
}

pub fn load_auth_session() -> Option<AuthSession> {
    let path = auth_session_path();
    if !path.exists() {
        return None;
    }
    let read_text = std::fs::read_to_string(&path);
    let text = match read_text {
        Ok(text) => text,
        Err(_) => return None,
    };
    serde_json::from_str(&text).ok()
}

pub fn save_auth_session(session: &AuthSession) -> Result<PathBuf> {
    let path = auth_session_path();
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let text =
        serde_json::to_string_pretty(session).expect("AuthSession serialization is infallible");
    write_auth_session_text(&path, &text)?;
    set_owner_only_permissions(&path)?;
    Ok(path)
}

pub fn clear_auth_session() -> Result<bool> {
    let path = auth_session_path();
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
fn write_auth_session_text(path: &Path, text: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_auth_session_text(path: &Path, text: &str) -> Result<()> {
    std::fs::write(path, text)?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn operator_for_path(path: &str) -> Result<opendal::Operator> {
    use opendal::services::Fs;
    let trimmed = path.trim_end_matches('/');
    let root = if let Some(local_root) = path.strip_prefix("file://") {
        let local_root = local_root.trim_end_matches('/');
        if local_root.is_empty() {
            "/"
        } else {
            local_root
        }
    } else if path.contains("://") {
        bail!("unsupported storage uri in core mode: {path}");
    } else if trimmed.is_empty() {
        "/"
    } else {
        trimmed
    };
    if root.contains('\0') {
        bail!("unsupported local path contains null byte: {path:?}");
    }
    // Core-mode background jobs may update a status document while the CLI
    // reads it. Keep OpenDAL's filesystem replacement writes on a proven
    // same-filesystem directory, including when a root-backed operator is
    // opened before `/spaces` exists.
    let atomic_write_dir = local_atomic_write_dir(root)?;
    let mut builder = Fs::default().root(root);
    // Do not silently fall back to OpenDAL's truncating write path. Space
    // metadata/settings replacement relies on this helper for crash-safe JSON
    // publication, so an unavailable same-filesystem directory is a
    // configuration error rather than a weaker storage mode.
    std::fs::create_dir_all(&atomic_write_dir).with_context(|| {
        format!(
            "create same-filesystem atomic write directory {}",
            atomic_write_dir.display()
        )
    })?;
    set_owner_only_directory(&atomic_write_dir)?;
    builder = builder.atomic_write_dir(atomic_write_dir.to_string_lossy().as_ref());
    Ok(opendal::Operator::new(builder)?)
}

fn local_atomic_write_dir(root: &str) -> Result<PathBuf> {
    // Atomic writes target Space objects below root/spaces. If the process is
    // pointed at the filesystem root before that directory exists, use a
    // verified same-filesystem temporary directory; once a spaces directory
    // exists, never cross that boundary.
    if root == "/" {
        let spaces = Path::new(root).join("spaces");
        if spaces.exists() {
            return Ok(spaces.join(".ugoite-atomic-writes"));
        }
        let temp = std::env::temp_dir();
        if same_filesystem(Path::new(root), &temp) {
            return Ok(temp.join(format!(".ugoite-atomic-writes-{}", std::process::id())));
        }
        bail!("cannot configure same-filesystem atomic writes for local root /");
    }
    Ok(Path::new(root).join("spaces").join(".ugoite-atomic-writes"))
}

#[cfg(unix)]
fn same_filesystem(first: &Path, second: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    std::fs::metadata(first)
        .and_then(|first| std::fs::metadata(second).map(|second| first.dev() == second.dev()))
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn same_filesystem(_first: &Path, _second: &Path) -> bool {
    true
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_owner_only_directory(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn normalize_space_root(root_path: &str) -> String {
    let trimmed = if root_path == "/" {
        "/"
    } else {
        root_path.trim_end_matches('/')
    };
    if let Some(parent) = trimmed.strip_suffix("/spaces") {
        if parent.is_empty() {
            return "/".to_string();
        }
        return parent.to_string();
    }
    trimmed.to_string()
}

pub fn validate_server_endpoint_url(url: &str, label: &str) -> Result<()> {
    crate::cli_config::model::validate_remote_url(url, label).map(|_| ())
}

/// Centralized output contract lives in `crate::output` (E0). These
/// Re-exports keep existing command imports working while the presentation
/// implementation lives under `crate::output`.
pub use crate::output::{
    effective_format, effective_format_for_stdout, emit_success, print_json, print_json_table,
    print_list_table, project_error, CliError, ExitCode, Format, MutationReceipt,
};
