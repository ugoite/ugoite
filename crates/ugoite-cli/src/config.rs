use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

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

pub fn operator_for_path(path: &str) -> Result<opendal::Operator> {
    let trimmed = path.trim_end_matches('/');
    let root = if let Some(local_root) = path.strip_prefix("file://") {
        let local_root = local_root.trim_end_matches('/');
        if local_root.is_empty() {
            "/"
        } else {
            local_root
        }
    } else if path.contains("://") {
        bail!("unsupported storage uri on a local core connection: {path}");
    } else if trimmed.is_empty() {
        "/"
    } else {
        trimmed
    };
    if root.contains('\0') {
        bail!("unsupported local path contains null byte: {path:?}");
    }
    // Keep local-path admission here; Storage owns same-filesystem atomic
    // write directory setup and the special root `/` handling.
    ugoite_storage::operator_from_uri(&format!("fs://{root}"))
}

pub fn validate_server_endpoint_url(url: &str, label: &str) -> Result<()> {
    crate::cli_config::model::validate_remote_url(url, label).map(|_| ())
}
