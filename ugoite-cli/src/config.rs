use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    if let Ok(p) = std::env::var("UGOITE_CLI_CONFIG_PATH") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(h) = std::env::var("UGOITE_CONFIG_HOME") {
        if !h.trim().is_empty() {
            return PathBuf::from(h).join("ugoite").join("cli-endpoints.json");
        }
    }
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.trim().is_empty() {
            return PathBuf::from(x).join("ugoite").join("cli-endpoints.json");
        }
    }
    dirs_home().join(".ugoite").join("cli-endpoints.json")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

pub fn load_config() -> EndpointConfig {
    let path = config_path();
    if !path.exists() {
        return EndpointConfig::default();
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return EndpointConfig::default(),
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save_config(config: &EndpointConfig) -> Result<PathBuf> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, text)?;
    Ok(path)
}

pub fn operator_for_path(path: &str) -> Result<opendal::Operator> {
    use opendal::services::Fs;
    let trimmed = path.trim_end_matches('/');
    let mut root = if trimmed.is_empty() { "/" } else { trimmed };
    if let Some(local_root) = root.strip_prefix("file://") {
        root = if local_root.is_empty() {
            "/"
        } else {
            local_root
        };
    } else if root.contains("://") {
        bail!("unsupported storage uri in core mode: {root}");
    }
    let builder = Fs::default().root(root);
    Ok(opendal::Operator::new(builder)?.finish())
}

pub fn space_ws_path(_root_path: &str, space_id: &str) -> String {
    format!("spaces/{}", space_id)
}

pub fn parse_space_path(space_path: &str) -> (String, String) {
    let text = space_path.trim_end_matches('/');
    if let Some(pos) = text.rfind("/spaces/") {
        let root = &text[..pos];
        let rest = &text[pos + 8..];
        let space_id = rest.split('/').next().unwrap_or(rest);
        (root.to_string(), space_id.to_string())
    } else {
        (
            "".to_string(),
            std::path::Path::new(text)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(text)
                .to_string(),
        )
    }
}

pub fn base_url(config: &EndpointConfig) -> Option<String> {
    match config.mode {
        EndpointMode::Backend => Some(config.backend_url.trim_end_matches('/').to_string()),
        EndpointMode::Api => Some(config.api_url.trim_end_matches('/').to_string()),
        EndpointMode::Core => None,
    }
}

pub fn print_json<T: serde::Serialize>(value: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).unwrap_or_default()
    );
}
