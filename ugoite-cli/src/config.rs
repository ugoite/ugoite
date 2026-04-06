use anyhow::{anyhow, bail, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::io::IsTerminal;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EndpointMode {
    #[default]
    Core,
    Backend,
    Api,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EndpointConfig {
    #[serde(default)]
    pub mode: EndpointMode,
    #[serde(default = "default_backend_url")]
    pub backend_url: String,
    #[serde(default = "default_api_url")]
    pub api_url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bearer_token: Option<String>,
}

fn default_backend_url() -> String {
    "http://localhost:8000".to_string()
}

fn default_api_url() -> String {
    "http://localhost:3000/api".to_string()
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            mode: EndpointMode::Core,
            backend_url: default_backend_url(),
            api_url: default_api_url(),
        }
    }
}

pub fn config_path() -> PathBuf {
    if let Some(path) = non_empty_env_path("UGOITE_CLI_CONFIG_PATH") {
        return path;
    }
    if let Some(config_home) = non_empty_env_path("UGOITE_CONFIG_HOME") {
        return config_home.join("ugoite").join("cli-endpoints.json");
    }
    if let Some(xdg_config_home) = non_empty_env_path("XDG_CONFIG_HOME") {
        return xdg_config_home.join("ugoite").join("cli-endpoints.json");
    }
    dirs_home().join(".ugoite").join("cli-endpoints.json")
}

pub fn auth_session_path() -> PathBuf {
    config_path()
        .parent()
        .unwrap_or(Path::new("."))
        .join("cli-auth.json")
}

fn non_empty_env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key).ok().and_then(|value| {
        if value.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    })
}

fn dirs_home() -> PathBuf {
    match std::env::var("HOME") {
        Ok(home) => PathBuf::from(home),
        Err(_) => PathBuf::from("."),
    }
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

pub fn load_config() -> EndpointConfig {
    let path = config_path();
    if !path.exists() {
        return EndpointConfig::default();
    }
    let read_text = std::fs::read_to_string(&path);
    let text = match read_text {
        Ok(text) => text,
        Err(_) => return EndpointConfig::default(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn load_auth_session() -> AuthSession {
    let path = auth_session_path();
    if !path.exists() {
        return AuthSession::default();
    }
    let read_text = std::fs::read_to_string(&path);
    let text = match read_text {
        Ok(text) => text,
        Err(_) => return AuthSession::default(),
    };
    let mut session: AuthSession = serde_json::from_str(&text).unwrap_or_default();
    session.bearer_token = session.bearer_token.and_then(non_empty_string);
    session
}

pub fn save_config(config: &EndpointConfig) -> Result<PathBuf> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text =
        serde_json::to_string_pretty(config).expect("EndpointConfig serialization is infallible");
    std::fs::write(&path, text)?;
    Ok(path)
}

pub fn save_auth_session(session: &AuthSession) -> Result<PathBuf> {
    let path = auth_session_path();
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let normalized = AuthSession {
        bearer_token: session.bearer_token.clone().and_then(non_empty_string),
    };
    let text =
        serde_json::to_string_pretty(&normalized).expect("AuthSession serialization is infallible");
    std::fs::write(&path, text)?;
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

pub fn effective_bearer_token() -> Option<String> {
    non_empty_env_value("UGOITE_AUTH_BEARER_TOKEN").or_else(|| load_auth_session().bearer_token)
}

pub fn effective_api_key() -> Option<String> {
    non_empty_env_value("UGOITE_AUTH_API_KEY")
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
    let builder = Fs::default().root(root);
    opendal::Operator::new(builder)
        .map(|operator| operator.finish())
        .map_err(Into::into)
}

pub fn space_ws_path(_root_path: &str, space_id: &str) -> String {
    format!("spaces/{}", space_id)
}

fn explicit_core_space_path(space_path: &str) -> Option<(String, String)> {
    let text = space_path.trim_end_matches('/');
    let pos = text.rfind("/spaces/")?;
    let root = if pos == 0 { "/" } else { &text[..pos] };
    let rest = &text[pos + 8..];
    let space_id = rest.split('/').next().unwrap_or(rest);
    if root.is_empty() || space_id.is_empty() {
        return None;
    }
    Some((root.to_string(), space_id.to_string()))
}

pub fn parse_space_path(space_path: &str) -> (String, String) {
    if let Some(explicit) = explicit_core_space_path(space_path) {
        return explicit;
    }
    let text = space_path.trim_end_matches('/');
    (
        "".to_string(),
        std::path::Path::new(text)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(text)
            .to_string(),
    )
}

pub fn resolve_space_reference(
    config: &EndpointConfig,
    space_path: &str,
    command_name: &str,
) -> Result<(String, String)> {
    let parsed = parse_space_path(space_path);
    if validated_base_url(config)?.is_some() {
        return Ok(parsed);
    }
    explicit_core_space_path(space_path).ok_or_else(|| {
        anyhow!(
            "{command_name} requires SPACE_ID_OR_PATH as /path/to/root/spaces/<id> in core mode"
        )
    })
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

struct SelectedServerEndpoint<'a> {
    label: &'static str,
    url: &'a str,
}

fn selected_server_endpoint(config: &EndpointConfig) -> Option<SelectedServerEndpoint<'_>> {
    match config.mode {
        EndpointMode::Backend => Some(SelectedServerEndpoint {
            label: "Backend endpoint",
            url: &config.backend_url,
        }),
        EndpointMode::Api => Some(SelectedServerEndpoint {
            label: "API endpoint",
            url: &config.api_url,
        }),
        EndpointMode::Core => None,
    }
}

pub fn base_url(config: &EndpointConfig) -> Option<String> {
    selected_server_endpoint(config).map(|endpoint| endpoint.url.trim_end_matches('/').to_string())
}

fn is_loopback_host(host: &str) -> bool {
    let normalized = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    normalized.eq_ignore_ascii_case("localhost")
        || normalized
            .parse::<IpAddr>()
            .map(|address| address.is_loopback())
            .unwrap_or(false)
}

pub fn validate_server_endpoint_url(url: &str, label: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|error| anyhow!("{label} URL {url:?} is invalid: {error}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            if parsed.host_str().is_some_and(is_loopback_host) {
                return Ok(());
            }
            bail!(
                "{label} URL {url} uses cleartext http:// for a non-loopback host. Use https:// for remote endpoints, or use a loopback http:// URL for local development."
            )
        }
        scheme => bail!("{label} URL {url} must use http:// or https://, not {scheme}://."),
    }
}

pub fn endpoint_transport_warning(url: &str, label: &str) -> Option<String> {
    validate_server_endpoint_url(url, label).err().map(|error| {
        format!(
            "{error} Server-backed commands will refuse this endpoint until you switch to https:// or a loopback http:// URL."
        )
    })
}

pub fn validated_base_url(config: &EndpointConfig) -> Result<Option<String>> {
    let Some(endpoint) = selected_server_endpoint(config) else {
        return Ok(None);
    };
    let base = endpoint.url.trim_end_matches('/').to_string();
    validate_server_endpoint_url(&base, endpoint.label)?;
    Ok(Some(base))
}

pub fn print_json<T: serde::Serialize>(value: &T) {
    let pretty_json = serde_json::to_string_pretty(value);
    let rendered = pretty_json.unwrap_or_default();
    println!("{rendered}");
}

/// Output format for CLI commands.
#[derive(ValueEnum, Clone, Debug, Default, PartialEq)]
pub enum Format {
    /// Pretty-printed JSON (default when piped)
    #[default]
    Json,
    /// Human-readable table (default when stdout is a TTY)
    Table,
    /// Key: value lines for single objects
    Plain,
}

/// Return the effective format: use explicit override, or auto-detect TTY.
pub fn effective_format(explicit: Option<Format>) -> Format {
    effective_format_for_stdout(explicit, std::io::stdout().is_terminal())
}

#[doc(hidden)]
pub fn effective_format_for_stdout(explicit: Option<Format>, stdout_is_terminal: bool) -> Format {
    if let Some(f) = explicit {
        return f;
    }
    if stdout_is_terminal {
        Format::Table
    } else {
        Format::Json
    }
}

/// Print a list of string IDs as a single-column table.
pub fn print_list_table(header: &str, items: &[impl std::fmt::Display]) {
    let col_width = items
        .iter()
        .map(|s| s.to_string().len())
        .max()
        .unwrap_or(0)
        .max(header.len());
    println!("{:<col_width$}", header, col_width = col_width);
    println!("{}", "-".repeat(col_width));
    for item in items {
        println!("{item}");
    }
}

/// Print a list of JSON objects as a table, selecting the given columns.
/// Columns is a slice of `(header, json_key)` pairs.
pub fn print_json_table(rows: &[serde_json::Value], columns: &[(&str, &str)]) {
    let mut widths: Vec<usize> = columns.iter().map(|(h, _)| h.len()).collect();
    let cell_matrix: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            columns
                .iter()
                .enumerate()
                .map(|(i, (_, key))| {
                    let cell = match &row[key] {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    widths[i] = widths[i].max(cell.len());
                    cell
                })
                .collect()
        })
        .collect();
    let header: Vec<String> = columns
        .iter()
        .enumerate()
        .map(|(i, (h, _))| format!("{:<width$}", h, width = widths[i]))
        .collect();
    println!("{}", header.join("  "));
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    println!("{}", sep.join("  "));
    for row_cells in &cell_matrix {
        let row: Vec<String> = row_cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<width$}", c, width = widths[i]))
            .collect();
        println!("{}", row.join("  "));
    }
}
