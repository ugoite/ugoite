//! Canonical TOML model for CLI configuration v1.
//!
//! Disk format (TOML):
//! ```toml
//! version = 1
//! current_context = "personal"
//! [connections.local]
//! type = "core"
//! root = "/absolute/path/to/workspace"
//! [contexts.personal]
//! connection = "local"
//! space_uid = "019f1111-1111-7abc-8def-111111111111"
//! ```
//!
//! Config never carries Space authority: only immutable `space_uid` plus
//! connection/credential names are stored. Display names, slugs, forms,
//! entries, and settings are always read from the Space itself.

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::IpAddr;

pub const CONFIG_VERSION_V1: u32 = 1;

/// Named connection definition.
///
/// `core` / `backend` / `api` are transport/topology attributes of a named
/// connection, never a global CLI mode.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum ConnectionConfig {
    Core { root: String },
    Backend { url: String },
    Api { url: String },
}

/// Named execution target: connection + immutable Space UID + optional
/// named credential profile reference. Secrets never live here.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    pub connection: String,
    pub space_uid: uuid::Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

/// On-disk canonical config file.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    #[serde(default = "default_config_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_context: Option<String>,
    #[serde(default)]
    pub connections: BTreeMap<String, ConnectionConfig>,
    #[serde(default)]
    pub contexts: BTreeMap<String, ContextConfig>,
}

fn default_config_version() -> u32 {
    CONFIG_VERSION_V1
}

impl Default for ConfigFile {
    fn default() -> Self {
        Self::empty()
    }
}

impl ConfigFile {
    pub fn empty() -> Self {
        Self {
            version: CONFIG_VERSION_V1,
            current_context: None,
            connections: BTreeMap::new(),
            contexts: BTreeMap::new(),
        }
    }

    /// Parse one TOML source. Duplicate keys inside one file are rejected by
    /// the TOML parser itself; we surface that as a fail-closed error.
    pub fn parse_toml(text: &str, source_label: &str) -> Result<Self> {
        let parsed: ConfigFile =
            toml::from_str(text).with_context(|| format!("invalid TOML in {source_label}"))?;
        parsed.validate(source_label)?;
        Ok(parsed)
    }

    /// Fail-closed per-file validation (cross-file refs are checked on the
    /// effective config instead).
    pub fn validate(&self, source_label: &str) -> Result<()> {
        if self.version != CONFIG_VERSION_V1 {
            bail!(
                "unsupported config version {} in {source_label} (expected 1)",
                self.version
            );
        }
        for (name, connection) in &self.connections {
            validate_connection_name(name)
                .with_context(|| format!("invalid connection name {name:?} in {source_label}"))?;
            validate_connection(connection)
                .with_context(|| format!("invalid connection {name:?} in {source_label}"))?;
        }
        for (name, context) in &self.contexts {
            validate_context_name(name)
                .with_context(|| format!("invalid context name {name:?} in {source_label}"))?;
            validate_context_fields(context)
                .with_context(|| format!("invalid context {name:?} in {source_label}"))?;
            // Cross-file references (context -> connection from a
            // lower-priority source) are legal; the effective config
            // enforces that the merged view resolves.
        }
        if let Some(current) = &self.current_context {
            validate_config_name(current, "context").with_context(|| {
                format!("invalid current_context {current:?} in {source_label}")
            })?;
            // Cross-source current_context targets are legal; the effective
            // config enforces resolvability.
        }
        Ok(())
    }
}

fn validate_connection_name(name: &str) -> Result<()> {
    validate_config_name(name, "connection")
}

fn validate_context_name(name: &str) -> Result<()> {
    validate_config_name(name, "context")
}

/// Shared name rule for connection/context/credential identities.
///
/// Rejects empty, leading/trailing whitespace, ".", "..", "/", "\\", NUL;
/// allows Unicode and `.-_` otherwise. Names are single path-segment
/// identities, never paths.
pub fn validate_config_name(name: &str, kind: &str) -> Result<()> {
    if name.is_empty() || name.trim().is_empty() {
        bail!("{kind} name must be a non-empty single path segment");
    }
    if name != name.trim() {
        bail!("{kind} name must not have leading or trailing whitespace");
    }
    if name == "." || name == ".." {
        bail!("{kind} name must not be {name:?}");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        bail!("{kind} name must be a non-empty single path segment");
    }
    Ok(())
}

/// Credential profile identities share the connection/context rule.
pub fn validate_credential_name(name: &str) -> Result<()> {
    validate_config_name(name, "credential")
}

pub fn validate_connection(connection: &ConnectionConfig) -> Result<()> {
    match connection {
        ConnectionConfig::Core { root } => {
            if root.trim().is_empty() || root.contains('\0') {
                bail!("core connection requires a non-empty root path");
            }
        }
        ConnectionConfig::Backend { url } => {
            validate_remote_url(url, "Backend endpoint")?;
        }
        ConnectionConfig::Api { url } => {
            validate_remote_url(url, "API endpoint")?;
        }
    }
    Ok(())
}

fn validate_context_fields(context: &ContextConfig) -> Result<()> {
    if context.connection.trim().is_empty() {
        bail!("context connection must not be empty");
    }
    validate_config_name(&context.connection, "connection")
        .with_context(|| format!("invalid context connection {:?}", context.connection))?;
    if context.space_uid.get_version() != Some(uuid::Version::SortRand) {
        bail!("context space_uid must be a UUIDv7");
    }
    if let Some(credential) = &context.credential {
        validate_credential_name(credential)
            .with_context(|| format!("invalid context credential {credential:?}"))?;
    }
    Ok(())
}

/// Shared endpoint rule: `https://` is always OK,
/// `http://` only for loopback development hosts.
///
/// Fail-closed base-endpoint rules: non-empty host, no embedded userinfo, no
/// fragment, and no query string (a base endpoint is an origin + path, never
/// `?`/`#` material). Rejects e.g. `https:///path`.
pub fn validate_remote_url(url: &str, label: &str) -> Result<url::Url> {
    let parsed =
        url::Url::parse(url).map_err(|error| anyhow!("{label} URL {url:?} is invalid: {error}"))?;
    let host = parsed.host_str().unwrap_or_default();
    if host.trim().is_empty() {
        bail!("{label} URL {url:?} must have a non-empty host");
    }
    // Fail-closed empty-authority check: `https:///path` parses with host
    // "path" under WHATWG rules, but the literal text has no authority at
    // all. Require the parsed host to appear literally after `scheme://`
    // (case-insensitive); this also rejects any hidden userinfo prefix.
    // Bracket-aware: IPv6 literals appear bracketed in text (`[::1]`) while
    // some parsers expose the host bare (`::1`), so accept either form and
    // never require the port to be part of the match.
    let lowered = url.to_ascii_lowercase();
    let host_lower = host.to_ascii_lowercase();
    let bracketed_lower = if host_lower.starts_with('[') || !host_lower.contains(':') {
        host_lower.clone()
    } else {
        format!("[{host_lower}]")
    };
    let expected_plain = format!("://{host_lower}");
    let expected_bracketed = format!("://{bracketed_lower}");
    if !lowered.contains(&expected_plain) && !lowered.contains(&expected_bracketed) {
        bail!("{label} URL {url:?} must include a host authority (for example https://host/path), not an empty authority");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("{label} URL {url:?} must not embed userinfo");
    }
    if parsed.fragment().is_some() {
        bail!("{label} URL {url:?} must not contain a fragment");
    }
    if parsed.query().is_some() {
        bail!("{label} URL {url:?} must not contain a query string");
    }
    match parsed.scheme() {
        "https" => Ok(parsed),
        "http" => {
            let host = parsed.host_str().unwrap_or_default();
            if is_loopback_host(host) {
                Ok(parsed)
            } else {
                bail!(
                    "{label} URL {url} uses cleartext http:// for a non-loopback host. Use https:// for remote endpoints, or use a loopback http:// URL for local development."
                )
            }
        }
        scheme => bail!("{label} URL {url} must use http:// or https://, not {scheme}://."),
    }
}

fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
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

/// Serialize a config file to canonical TOML.
pub fn serialize_config(config: &ConfigFile) -> Result<String> {
    toml::to_string_pretty(config).context("serialize canonical CLI config as TOML")
}

// Write-boundary root normalization lives in exactly one place:
// `super::write::normalize_core_root_to_absolute`. This module owns
// validation only, never a second normalization authority.

#[cfg(test)]
mod tests {
    use super::*;

    fn v7(id: &str) -> uuid::Uuid {
        uuid::Uuid::parse_str(id).unwrap()
    }

    #[test]
    fn parses_canonical_example() {
        let text = r#"
version = 1
current_context = "personal"
[connections.local]
type = "core"
root = "/Users/alice/knowledge"
[contexts.personal]
connection = "local"
space_uid = "019f1111-1111-7abc-8def-111111111111"
"#;
        let parsed = ConfigFile::parse_toml(text, "test").unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.current_context.as_deref(), Some("personal"));
        assert!(matches!(
            parsed.connections.get("local"),
            Some(ConnectionConfig::Core { .. })
        ));
        assert_eq!(
            parsed.contexts.get("personal").unwrap().space_uid,
            v7("019f1111-1111-7abc-8def-111111111111")
        );
    }

    #[test]
    fn rejects_unsupported_version() {
        let parsed = ConfigFile::parse_toml("version = 2\n", "test");
        assert!(parsed.is_err());
    }

    #[test]
    fn rejects_invalid_toml() {
        assert!(ConfigFile::parse_toml("version = [", "test").is_err());
    }

    #[test]
    fn rejects_duplicate_object_within_one_file() {
        let text = "[connections.work]\ntype = \"backend\"\nurl = \"https://a.example.com\"\n[connections.work]\ntype = \"backend\"\nurl = \"https://b.example.com\"\nversion = 1\n";
        assert!(ConfigFile::parse_toml(text, "test").is_err());
    }

    #[test]
    fn rejects_non_v7_space_uid() {
        let text = r#"
version = 1
[connections.local]
type = "core"
root = "/tmp/root"
[contexts.personal]
connection = "local"
space_uid = "123e4567-e89b-42d3-a456-426614174000"
"#;
        assert!(ConfigFile::parse_toml(text, "test").is_err());
    }

    #[test]
    fn rejects_cleartext_remote_url() {
        let text = r#"
version = 1
[connections.work]
type = "backend"
url = "http://ugoite.example.com"
"#;
        assert!(ConfigFile::parse_toml(text, "test").is_err());
    }

    #[test]
    fn accepts_loopback_http() {
        let text = r#"
version = 1
[connections.work]
type = "backend"
url = "http://localhost:8000"
"#;
        assert!(ConfigFile::parse_toml(text, "test").is_ok());
    }

    #[test]
    fn accepts_ipv6_and_ipv4_loopback_http_with_and_without_port() {
        for url in [
            "http://[::1]/",
            "http://[::1]:8000/",
            "http://[::1]:8000",
            "http://127.0.0.1/",
            "http://127.0.0.1:8000/",
            "http://127.0.0.1:8000",
        ] {
            assert!(
                validate_remote_url(url, "Remote endpoint").is_ok(),
                "loopback URL must pass: {url}"
            );
        }
    }

    #[test]
    fn rejects_empty_authority_and_hidden_userinfo() {
        for url in [
            "https:///path",
            "https://user:pass@example.com",
            "https://example.com/path#frag",
            "https://example.com/path?query=1",
        ] {
            assert!(
                validate_remote_url(url, "Remote endpoint").is_err(),
                "malformed URL must fail: {url}"
            );
        }
    }
}
