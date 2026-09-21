use crate::config::{print_json, AuthSession};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use clap::{Args, Subcommand, ValueEnum};
use p256::{
    ecdsa::{signature::Signer, Signature, SigningKey},
    elliptic_curve::rand_core::OsRng,
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::IsTerminal;
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub const DEFAULT_DEVICE_ACTIONS: &str = "read,create,update";

/// Parse the `--space-uid` login scope as an immutable UUIDv7 Space UID.
///
/// Backend/api mode addresses a Knowledge authority by its immutable UUIDv7
/// Space UID only. Slugs, filesystem paths, and non-v7 UUIDs are rejected
/// here with a CLI usage error so a typo can never silently scope a
/// credential to another Space.
fn parse_space_uid_arg(value: &str) -> Result<Uuid, String> {
    let parsed = Uuid::parse_str(value.trim())
        .map_err(|_| "backend/api mode requires SPACE_UID (UUIDv7)".to_string())?;
    if parsed.get_version() != Some(uuid::Version::SortRand) {
        return Err("backend/api mode requires SPACE_UID (UUIDv7)".to_string());
    }
    Ok(parsed)
}

/// Renders the device-authorization prompt without leaking the one-time
/// secret into logs.
///
/// Interactive terminals address a human who must type the code now, so the
/// secret appears exactly once on stderr with explicit entry guidance and is
/// never emitted as a plain log line. Non-interactive output (CI, pipes)
/// omits the secret entirely and reports machine-readable ceremony state:
/// the verification URL plus the fact that a code is required.
///
/// When the server provides `verification_uri_complete` (which embeds the
/// one-time code), interactive terminals show that complete URI so the human
/// can open it directly; the code is still shown for manual entry. Machine
/// JSON emits `verification_uri_complete` (falling back to
/// `verification_uri`) and never the raw `user_code`.
pub fn device_authorization_prompt(
    user_code: &str,
    verification_uri: &str,
    verification_uri_complete: Option<&str>,
    stderr_is_terminal: bool,
) -> String {
    let complete = verification_uri_complete
        .filter(|uri| !uri.trim().is_empty())
        .unwrap_or(verification_uri);
    if stderr_is_terminal {
        format!(
            "Open {complete} on any signed-in device.\nEnter this one-time code now: {user_code}\n(The code is shown only here; it is never logged.)"
        )
    } else {
        serde_json::to_string(&json!({
            "code": "DEVICE_AUTHORIZATION_REQUIRED",
            "verification_uri": verification_uri,
            "verification_uri_complete": complete,
            "user_code_required": true,
        }))
        .expect("device ceremony state serializes")
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum AuthLoginTarget {
    /// The issuer-audience credential used by REST CLI operations.
    Rest,
    /// The protected-resource credential used by the CLI Konase MCP host.
    Mcp,
}

#[derive(Args)]
pub struct AuthCmd {
    #[command(subcommand)]
    pub sub: AuthSubCmd,
}

#[derive(Subcommand)]
pub enum AuthSubCmd {
    /// Show the paired device and short-lived token state.
    Profile {
        /// Named credential profile (canonical). Omit to use the current context's credential.
        #[arg(long, value_name = "CREDENTIAL")]
        credential: Option<String>,
    },
    /// Pair this terminal without requiring a browser on the terminal itself.
    Login {
        #[arg(long, default_value = "Ugoite CLI")]
        device_name: String,
        #[arg(
            long,
            value_parser = parse_space_uid_arg,
            value_name = "SPACE_UID",
            help = "Immutable Space UID (UUIDv7) to scope the new credential to. A local Space path or slug is never accepted here."
        )]
        space_uid: Option<Uuid>,
        #[arg(
            long,
            value_delimiter = ',',
            default_value = DEFAULT_DEVICE_ACTIONS
        )]
        actions: Vec<String>,
        /// Credential target. MCP discovers the protected resource metadata;
        /// its raw resource URL is not needed on the command line.
        #[arg(long = "for", value_enum, default_value_t = AuthLoginTarget::Rest)]
        target: AuthLoginTarget,
        /// Canonical connection to authenticate against (named profile mode).
        #[arg(long, value_name = "CONNECTION")]
        connection: Option<String>,
        /// Named credential profile to store (canonical). When omitted and
        /// the selected context uniquely determines one, it is used.
        /// Re-running login for an existing name replaces that profile.
        #[arg(long, value_name = "CREDENTIAL")]
        credential: Option<String>,
    },
    /// Revoke local access by deleting the local device credential.
    Logout {
        /// Named credential profile (canonical). Omit to use the current context's credential.
        #[arg(long, value_name = "CREDENTIAL")]
        credential: Option<String>,
    },
}

pub async fn run(
    cmd: AuthCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        AuthSubCmd::Profile { credential } => {
            let name = match credential.as_deref() {
                Some(name) => {
                    if name.trim().is_empty() {
                        bail!("--credential must not be empty; pass a credential name or omit --credential to use the current context's credential");
                    }
                    name.to_string()
                }
                None => current_context_credential(explicit_config, context_override)?,
            };
            print_named_profile(&name)?;
        }
        AuthSubCmd::Login {
            device_name,
            space_uid,
            actions,
            target,
            connection,
            credential,
        } => {
            login_named(
                device_name,
                space_uid,
                actions,
                target,
                connection.as_deref(),
                credential.as_deref(),
                explicit_config,
                context_override,
            )
            .await?;
        }
        AuthSubCmd::Logout { credential } => {
            let name = match credential.as_deref() {
                Some(name) => {
                    if name.trim().is_empty() {
                        bail!("--credential must not be empty; pass a credential name or omit --credential to use the current context's credential");
                    }
                    name.to_string()
                }
                None => current_context_credential(explicit_config, context_override)?,
            };
            logout_named(&name)?;
        }
    }
    Ok(())
}

/// Named-profile login (plan section 44): authenticate against a canonical
/// connection and store the credential under a profile name in the
/// user-global credential store. Secrets never enter TOML; contexts reference
/// the profile by name only.
#[allow(clippy::too_many_arguments)]
async fn login_named(
    device_name: String,
    space_uid: Option<Uuid>,
    actions: Vec<String>,
    target: AuthLoginTarget,
    explicit_connection: Option<&str>,
    explicit_credential: Option<&str>,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    use crate::cli_config::{load_cli_config, resolve_cli_context, ConnectionConfig};

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let files = load_cli_config(explicit_config, &cwd)?;
    if files.sources.is_empty() {
        bail!("auth login --credential requires a canonical CLI config; run `ugoite config init` first.");
    }
    // Connection: explicit --connection, else the override/current context's
    // connection (same rule as space create). Core connections cannot take
    // device credentials.
    let connection_name = if let Some(name) = explicit_connection {
        let name = name.trim();
        if !files.effective.connections.contains_key(name) {
            bail!("Connection {name:?} is not defined.");
        }
        name.to_string()
    } else if let Some(name) = context_override {
        let context = files
            .effective
            .contexts
            .get(name)
            .ok_or_else(|| anyhow!("Context {name:?} is not defined."))?;
        context.value.connection.clone()
    } else if let Some(current) = files.effective.current_context.as_ref() {
        let context = files
            .effective
            .contexts
            .get(&current.value)
            .ok_or_else(|| anyhow!("Current context {:?} is not defined.", current.value))?;
        context.value.connection.clone()
    } else {
        bail!("Cannot determine a connection for `auth login`: pass --connection <NAME>.");
    };
    let connection = files
        .effective
        .connections
        .get(&connection_name)
        .ok_or_else(|| anyhow!("Connection {connection_name:?} is not defined."))?;
    let base = match &connection.value {
        ConnectionConfig::Core { .. } => {
            bail!("auth login requires a backend or api connection, not core.");
        }
        ConnectionConfig::Backend { url } | ConnectionConfig::Api { url } => {
            let parsed = crate::cli_config::model::validate_remote_url(url, "Login endpoint")?;
            parsed.as_str().trim_end_matches('/').to_string()
        }
    };
    // Credential: explicit --credential, else the uniquely determined profile
    // from the selected context (plan 44: omittable when unambiguous).
    let credential_name = if let Some(name) = explicit_credential {
        let name = name.trim();
        if name.is_empty() {
            bail!("credential name must not be empty");
        }
        name.to_string()
    } else {
        let scope = context_override
            .and_then(|name| files.effective.contexts.get(name))
            .or_else(|| {
                files
                    .effective
                    .current_context
                    .as_ref()
                    .and_then(|current| files.effective.contexts.get(&current.value))
            });
        match scope.and_then(|context| context.value.credential.clone()) {
            Some(name) => name,
            None => {
                bail!("Cannot determine a credential for `auth login`: pass --credential <NAME>.")
            }
        }
    };
    if context_override.is_some() {
        let _ = resolve_cli_context(&files.effective, context_override)?;
    }
    let resource = match target {
        AuthLoginTarget::Rest => None,
        AuthLoginTarget::Mcp => Some(mcp_resource(&base).await?),
    };
    let session = perform_device_login(&base, &device_name, space_uid, actions, resource).await?;
    store_named_profile(&connection_name, &credential_name, &session)?;
    // Never print secrets: only the profile identity is reported.
    let stored_view = serde_json::json!({
        "credential_id": session.credential_id,
        "space_uid": session.space_uid,
    });
    print_json(&serde_json::json!({
        "paired": true,
        "connection": connection_name,
        "credential": credential_name,
        "credential_id": stored_view["credential_id"].clone(),
        "space_uid": stored_view["space_uid"].clone(),
    }));
    Ok(())
}

/// Resolve the credential for `auth profile` / `auth logout` when
/// `--credential` is omitted: the current (or `--context`-selected) context's
/// named credential. Fails actionably when the context carries none.
fn current_context_credential(
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<String> {
    use crate::cli_config::{load_cli_config, resolve_cli_context};

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let files = load_cli_config(explicit_config, &cwd)?;
    let resolved = resolve_cli_context(&files.effective, context_override)?;
    resolved.credential_name.ok_or_else(|| {
        anyhow!(
            "Cannot determine a credential: pass --credential <NAME> or select a context with a credential."
        )
    })
}

/// Show one named profile without ever printing secrets.
fn print_named_profile(name: &str) -> Result<()> {
    let store = crate::cli_config::credentials::load_credentials()?;
    let profile = store
        .credentials
        .get(name)
        .ok_or_else(|| anyhow!("Credential profile {name:?} is not paired. Run `ugoite auth login --credential {name}`."))?;
    print_json(&redacted_profile(name, profile));
    Ok(())
}

/// Remove one named profile, leaving all others intact.
fn logout_named(name: &str) -> Result<()> {
    use crate::cli_config::credentials::{load_credentials, write_credentials};

    let mut store = load_credentials()?;
    let removed = store.credentials.remove(name);
    let Some(profile) = removed else {
        bail!("Credential profile {name:?} is not paired.");
    };
    // Best-effort OS-keychain cleanup for the removed profile only.
    // A null private_key_pkcs8 counts as absent (same rule as the display
    // projection), so hand-edited nulls cannot orphan a keychain entry.
    if let Some(credential_id) = profile.get("credential_id").and_then(Value::as_str) {
        if profile
            .get("private_key_pkcs8")
            .is_none_or(|value| value.is_null())
        {
            let _ = keyring::Entry::new("ugoite-cli", credential_id)
                .and_then(|entry| entry.delete_credential());
        }
    }
    write_credentials(&store)?;
    println!("Credential profile {name:?} removed.");
    Ok(())
}

/// Project a stored profile to its non-secret display shape.
fn redacted_profile(name: &str, profile: &Value) -> Value {
    let paired = !profile.is_null();
    json!({
        "paired": paired,
        "credential": name,
        "connection": profile.get("connection").cloned().unwrap_or(Value::Null),
        "credential_id": profile.get("credential_id").cloned().unwrap_or(Value::Null),
        "device_name": profile.get("device_name").cloned().unwrap_or(Value::Null),
        "space_uid": profile.get("space_uid").cloned().unwrap_or(Value::Null),
        "access_token_expires_at": profile.get("expires_at").cloned().unwrap_or(Value::Null),
        "credential_target": if profile.get("resource").is_some_and(|value| !value.is_null()) { "mcp" } else { "rest" },
        "private_key_storage": if profile.get("private_key_pkcs8").is_some_and(|value| !value.is_null()) { "owner_only_file" } else { "os_keychain" },
    })
}

/// Persist a device session as an opaque named profile (secrets live only in
/// the user-global credential store, never in TOML).
fn store_named_profile(
    connection_name: &str,
    credential_name: &str,
    session: &AuthSession,
) -> Result<std::path::PathBuf> {
    use crate::cli_config::credentials::{load_credentials, write_credentials};

    let mut store = load_credentials()?;
    let mut profile = serde_json::to_value(session).context("serialize CLI credential profile")?;
    profile["connection"] = Value::String(connection_name.to_string());
    store
        .credentials
        .insert(credential_name.to_string(), profile);
    write_credentials(&store)
}

/// Device authorization flow returning the established session without
/// persisting it; the caller stores it as a named credential profile.
async fn perform_device_login(
    base: &str,
    device_name: &str,
    space_uid: Option<Uuid>,
    actions: Vec<String>,
    resource: Option<String>,
) -> Result<AuthSession> {
    // UUIDv7 admission lives in exactly one place: the Clap value parser
    // (`parse_space_uid_arg`). No post-parse recheck here; the parser-level
    // regression test (`login_space_uid_accepts_only_uuid_v7`) pins rejection.
    let signing_key = SigningKey::random(&mut OsRng);
    let public_key_jwk = public_jwk(&signing_key);
    let device_payload = oauth_payload(
        json!({
            "device_name": device_name,
            "public_key_jwk": public_key_jwk,
            "space_uid": space_uid,
            "requested_actions": actions,
        }),
        resource.as_deref(),
    );
    let response = reqwest::Client::new()
        .post(format!(
            "{}/oauth/device/authorization",
            base.trim_end_matches('/')
        ))
        .json(&device_payload)
        .send()
        .await
        .context("start device authorization")?;
    let status = response.status();
    let device: Value = response.json().await?;
    if !status.is_success() {
        bail!("device authorization failed: {device}");
    }
    let user_code = device["user_code"]
        .as_str()
        .ok_or_else(|| anyhow!("server omitted user_code"))?;
    let verification_uri = device["verification_uri"]
        .as_str()
        .ok_or_else(|| anyhow!("server omitted verification_uri"))?;
    let verification_uri_complete = device["verification_uri_complete"].as_str();
    eprintln!(
        "{}",
        device_authorization_prompt(
            user_code,
            verification_uri,
            verification_uri_complete,
            std::io::stderr().is_terminal()
        )
    );
    let device_code = device["device_code"]
        .as_str()
        .ok_or_else(|| anyhow!("server omitted device_code"))?;
    let interval = device["interval"].as_u64().unwrap_or(5).max(1);
    let expires = Utc::now().timestamp() + device["expires_in"].as_i64().unwrap_or(600);
    let token_url = format!("{}/oauth/token", base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let token = loop {
        if Utc::now().timestamp() >= expires {
            bail!("device authorization expired");
        }
        let assertion = client_assertion(&signing_key, &public_key_jwk, &token_url)?;
        let response = client
            .post(&token_url)
            .json(&oauth_payload(
                json!({
                    "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                    "device_code": device_code,
                    "client_assertion": assertion,
                }),
                resource.as_deref(),
            ))
            .send()
            .await
            .context("poll device authorization")?;
        let status = response.status();
        let value: Value = response.json().await?;
        if status.is_success() {
            break value;
        }
        if value.get("error").and_then(Value::as_str) != Some("authorization_pending") {
            bail!("device authorization failed: {value}");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
    };
    let credential_id: Uuid = token["credential_id"]
        .as_str()
        .ok_or_else(|| anyhow!("token response omitted credential_id"))?
        .parse()?;
    let granted_space_uid: Uuid = token["space_uid"]
        .as_str()
        .ok_or_else(|| anyhow!("token response omitted space_uid"))?
        .parse()?;
    if let Some(requested) = space_uid {
        if requested != granted_space_uid {
            bail!("approved Space differs from the requested Space UID; refusing to fall back to another Space");
        }
    }
    let private_key = URL_SAFE_NO_PAD.encode(signing_key.to_pkcs8_der()?.as_bytes());
    let stored_in_keychain = keyring::Entry::new("ugoite-cli", &credential_id.to_string())
        .and_then(|entry| entry.set_password(&private_key))
        .is_ok();
    let session = AuthSession {
        credential_id,
        device_name: device_name.to_string(),
        public_key_jwk,
        private_key_pkcs8: (!stored_in_keychain).then_some(private_key),
        access_token: token["access_token"]
            .as_str()
            .ok_or_else(|| anyhow!("token response omitted access_token"))?
            .to_string(),
        refresh_token: token["refresh_token"]
            .as_str()
            .ok_or_else(|| anyhow!("token response omitted refresh_token"))?
            .to_string(),
        expires_at: Utc::now().timestamp() + token["expires_in"].as_i64().unwrap_or(300),
        base_url: base.to_string(),
        resource,
        space_uid: granted_space_uid,
    };
    Ok(session)
}

pub fn load_signing_key(session: &AuthSession) -> Result<SigningKey> {
    let encoded = match &session.private_key_pkcs8 {
        Some(value) => value.clone(),
        None => keyring::Entry::new("ugoite-cli", &session.credential_id.to_string())?
            .get_password()
            .context("read CLI private key from OS keychain")?,
    };
    SigningKey::from_pkcs8_der(&URL_SAFE_NO_PAD.decode(encoded)?).context("decode CLI private key")
}

pub fn dpop_proof(session: &AuthSession, method: &str, uri: &str) -> Result<String> {
    let key = load_signing_key(session)?;
    signed_jwt(
        &key,
        json!({"alg":"ES256","typ":"dpop+jwt","jwk":session.public_key_jwk}),
        json!({
            "htm": method.to_uppercase(), "htu": canonical_dpop_htu(uri)?, "ath": URL_SAFE_NO_PAD.encode(Sha256::digest(session.access_token.as_bytes())),
            "iat": Utc::now().timestamp(), "jti": Uuid::now_v7().to_string(),
        }),
    )
}

/// Return the DPoP HTTP target URI without query or fragment components.
pub fn canonical_dpop_htu(uri: &str) -> Result<String> {
    let mut url = Url::parse(uri).with_context(|| format!("invalid DPoP request URL: {uri}"))?;
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.to_string())
}

pub struct McpTarget {
    pub resource: String,
    pub endpoint: String,
}

/// Discover the exact protected resource and HTTP endpoint advertised by the
/// configured server. API-mode frontends proxy the well-known request and MCP
/// endpoint under `/api`; the integrated static server exposes those two MCP
/// routes at the public root, so an `/api` base also tries that root fallback.
pub async fn mcp_target(base_url: &str) -> Result<McpTarget> {
    let base_url = base_url.trim_end_matches('/');
    let mut candidates = vec![base_url.to_string()];
    if let Some(root_url) = api_base_root(base_url) {
        if root_url != base_url {
            candidates.push(root_url);
        }
    }

    let client = reqwest::Client::new();
    let mut failures = Vec::new();
    for candidate in candidates {
        let metadata_url = format!("{candidate}/.well-known/oauth-protected-resource");
        let response = match client.get(&metadata_url).send().await {
            Ok(response) => response,
            Err(error) => {
                failures.push(format!("{metadata_url}: {error}"));
                continue;
            }
        };
        let status = response.status();
        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => {
                failures.push(format!("{metadata_url}: {error}"));
                continue;
            }
        };
        if !status.is_success() {
            failures.push(format!("{metadata_url}: HTTP {status} ({body})"));
            continue;
        }
        let metadata: Value = match serde_json::from_str(&body) {
            Ok(metadata) => metadata,
            Err(error) => {
                failures.push(format!("{metadata_url}: {error}"));
                continue;
            }
        };
        let Some(resource) = metadata["resource"]
            .as_str()
            .filter(|resource| !resource.trim().is_empty())
        else {
            failures.push(format!("{metadata_url}: metadata omitted resource"));
            continue;
        };
        return Ok(McpTarget {
            resource: resource.to_owned(),
            endpoint: format!("{candidate}/mcp"),
        });
    }

    bail!("MCP resource discovery failed: {}", failures.join("; "));
}

pub async fn mcp_resource(base_url: &str) -> Result<String> {
    Ok(mcp_target(base_url).await?.resource)
}

fn api_base_root(base_url: &str) -> Option<String> {
    let mut url = Url::parse(base_url).ok()?;
    let path = url.path().trim_end_matches('/');
    let root_path = path.strip_suffix("/api")?.to_string();
    url.set_path(if root_path.is_empty() {
        "/"
    } else {
        &root_path
    });
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string().trim_end_matches('/').to_string())
}

/// Refresh an in-memory named-credential session.
///
/// Sessions refresh with a 30s expiry skew and persist through the
/// user-global credential store. Always returns `Ok(Some(..))` on success:
/// the input session unchanged when it is still fresh, else the rotated
/// session (failures are errors, never a silent `None`). The caller persists
/// any rotated session back to its named profile.
pub async fn refresh_session(session: &AuthSession, base_url: &str) -> Result<Option<AuthSession>> {
    if session.expires_at > Utc::now().timestamp() + 30 {
        return Ok(Some(session.clone()));
    }
    let mut refreshed = session.clone();
    let key = load_signing_key(&refreshed)?;
    let token_url = format!("{}/oauth/token", base_url.trim_end_matches('/'));
    let assertion = client_assertion(&key, &refreshed.public_key_jwk, &token_url)?;
    let response = reqwest::Client::new()
        .post(&token_url)
        .json(&oauth_payload(
            json!({
                "grant_type": "refresh_token",
                "refresh_token": refreshed.refresh_token,
                "client_assertion": assertion,
            }),
            refreshed.resource.as_deref(),
        ))
        .send()
        .await
        .context("refresh CLI access token")?;
    let status = response.status();
    let payload: Value = response.json().await?;
    if !status.is_success() {
        bail!("CLI credential refresh failed: {payload}");
    }
    refreshed.access_token = payload["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("refresh response omitted access_token"))?
        .to_string();
    refreshed.refresh_token = payload["refresh_token"]
        .as_str()
        .ok_or_else(|| anyhow!("refresh response omitted refresh_token"))?
        .to_string();
    refreshed.expires_at = Utc::now().timestamp() + payload["expires_in"].as_i64().unwrap_or(300);
    Ok(Some(refreshed))
}

fn oauth_payload(mut payload: Value, resource: Option<&str>) -> Value {
    if let Some(resource) = resource {
        payload["resource"] = Value::String(resource.to_owned());
    }
    payload
}

fn client_assertion(key: &SigningKey, jwk: &Value, audience: &str) -> Result<String> {
    let now = Utc::now().timestamp();
    let x = jwk["x"].as_str().ok_or_else(|| anyhow!("JWK x missing"))?;
    let y = jwk["y"].as_str().ok_or_else(|| anyhow!("JWK y missing"))?;
    let client_id = URL_SAFE_NO_PAD.encode(Sha256::digest(
        format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#).as_bytes(),
    ));
    signed_jwt(
        key,
        json!({"alg":"ES256","typ":"JWT","jwk":jwk}),
        json!({
            "iss": client_id, "sub": client_id,
            "aud": audience, "iat": now, "exp": now + 60, "jti": Uuid::now_v7().to_string(),
        }),
    )
}

fn signed_jwt(key: &SigningKey, header: Value, claims: Value) -> Result<String> {
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?);
    let input = format!("{header}.{payload}");
    let signature: Signature = key.sign(input.as_bytes());
    Ok(format!(
        "{input}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

fn public_jwk(key: &SigningKey) -> Value {
    let point = key.verifying_key().to_encoded_point(false);
    json!({
        "kty": "EC", "crv": "P-256",
        "x": URL_SAFE_NO_PAD.encode(point.x().expect("uncompressed x")),
        "y": URL_SAFE_NO_PAD.encode(point.y().expect("uncompressed y")),
    })
}

#[cfg(test)]
async fn login(
    base: &str,
    device_name: &str,
    space_uid: Option<Uuid>,
    actions: Vec<String>,
    resource: Option<String>,
) -> Result<AuthSession> {
    perform_device_login(base, device_name, space_uid, actions, resource).await
}

#[cfg(test)]
mod tests {
    use super::{
        api_base_root, canonical_dpop_htu, device_authorization_prompt, login, mcp_resource,
        mcp_target, oauth_payload, parse_space_uid_arg, public_jwk, refresh_session,
    };
    use base64::Engine as _;
    use p256::pkcs8::EncodePrivateKey;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    fn read_request(stream: &mut TcpStream) -> String {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set request read timeout");
        let mut request = Vec::new();
        let mut content_length = 0_usize;
        let mut body_start = None;
        loop {
            let mut buffer = [0_u8; 1024];
            let read = stream.read(&mut buffer).expect("read request");
            assert!(read > 0, "request ended before its body was received");
            request.extend_from_slice(&buffer[..read]);
            if body_start.is_none() {
                if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let end = position + 4;
                    body_start = Some(end);
                    let headers = String::from_utf8_lossy(&request[..end]);
                    for line in headers.lines() {
                        let mut parts = line.splitn(2, ':');
                        if let (Some(name), Some(value)) = (parts.next(), parts.next()) {
                            if name.eq_ignore_ascii_case("content-length") {
                                content_length = value.trim().parse().expect("content length");
                            }
                        }
                    }
                }
            }
            if body_start.is_some_and(|start| request.len() >= start + content_length) {
                return String::from_utf8(request).expect("UTF-8 HTTP request");
            }
        }
    }

    fn spawn_http_server(
        responses: Vec<(&'static str, String)>,
    ) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("test server address");
        let (sender, receiver) = mpsc::channel();
        let handle = thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().expect("accept test request");
                sender
                    .send(read_request(&mut stream))
                    .expect("send captured request");
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .expect("write test response");
            }
        });
        (format!("http://{address}"), receiver, handle)
    }

    #[test]
    fn device_prompt_marks_the_secret_for_interactive_entry_only() {
        let interactive = device_authorization_prompt(
            "ABCD-EFGH",
            "https://node.example/device",
            Some("https://node.example/device?user_code=ABCD-EFGH"),
            true,
        );
        assert!(interactive.contains("ABCD-EFGH"));
        assert!(interactive.contains("https://node.example/device?user_code=ABCD-EFGH"));
        assert!(interactive.contains("one-time code"));
        assert!(interactive.contains("never logged"));

        // Without a complete URI the interactive prompt falls back to the
        // plain verification URI.
        let fallback =
            device_authorization_prompt("ABCD-EFGH", "https://node.example/device", None, true);
        assert!(fallback.contains("https://node.example/device"));

        let machine = device_authorization_prompt(
            "ABCD-EFGH",
            "https://node.example/device",
            Some("https://node.example/device?user_code=ABCD-EFGH"),
            false,
        );
        let state: serde_json::Value = serde_json::from_str(&machine).expect("machine JSON");
        assert_eq!(state["code"], "DEVICE_AUTHORIZATION_REQUIRED");
        assert_eq!(state["verification_uri"], "https://node.example/device");
        assert_eq!(
            state["verification_uri_complete"],
            "https://node.example/device?user_code=ABCD-EFGH"
        );
        assert_eq!(state["user_code_required"], true);
        // The bare one-time secret is never a JSON field; the complete URI
        // is the server-provided handoff and may embed it for direct open.
        assert!(state.get("user_code").is_none());

        // Machine JSON falls back to verification_uri when the server omits
        // the complete URI, and never leaks the code.
        let fallback_machine =
            device_authorization_prompt("ABCD-EFGH", "https://node.example/device", None, false);
        let fallback_state: serde_json::Value =
            serde_json::from_str(&fallback_machine).expect("machine JSON");
        assert_eq!(
            fallback_state["verification_uri_complete"],
            "https://node.example/device"
        );
        assert!(fallback_state.get("user_code").is_none());
    }

    #[test]
    fn login_space_uid_accepts_only_uuid_v7() {
        assert!(parse_space_uid_arg(&uuid::Uuid::now_v7().to_string()).is_ok());
        assert!(parse_space_uid_arg("019f1234-5678-7abc-8def-0123456789ab").is_ok());
        for rejected in [
            uuid::Uuid::nil().to_string(),
            "123e4567-e89b-42d3-a456-426614174000".to_string(),
            "team-notes".to_string(),
            "/root/spaces/team-notes".to_string(),
            String::new(),
        ] {
            let error = parse_space_uid_arg(&rejected).expect_err("non-v7 Space UID must fail");
            assert!(
                error.contains("UUIDv7"),
                "unexpected validation error for {rejected:?}: {error}"
            );
        }
    }

    #[test]
    fn dpop_htu_excludes_query_and_fragment() {
        assert_eq!(
            canonical_dpop_htu("https://node.example/spaces/demo?cursor=next#ignored").unwrap(),
            "https://node.example/spaces/demo"
        );
    }

    #[test]
    fn dpop_htu_preserves_path() {
        assert_eq!(
            canonical_dpop_htu("https://node.example:8443/api/").unwrap(),
            "https://node.example:8443/api/"
        );
    }

    #[test]
    fn rest_oauth_payload_omits_resource() {
        let payload = oauth_payload(json!({"grant_type": "refresh_token"}), None);
        assert_eq!(payload, json!({"grant_type": "refresh_token"}));
    }

    #[test]
    fn mcp_oauth_payload_carries_resource() {
        let payload = oauth_payload(
            json!({"grant_type": "refresh_token"}),
            Some("https://ugoite.example/mcp"),
        );
        assert_eq!(payload["resource"], "https://ugoite.example/mcp");
    }

    #[test]
    fn api_base_root_strips_only_the_api_suffix() {
        assert_eq!(
            api_base_root("https://ugoite.example/api"),
            Some("https://ugoite.example".to_string())
        );
        assert_eq!(
            api_base_root("https://ugoite.example/console/api/"),
            Some("https://ugoite.example/console".to_string())
        );
        assert_eq!(api_base_root("https://ugoite.example/console"), None);
    }

    #[test]
    fn mcp_login_carries_resource_through_discovery_device_and_exchange() {
        let resource = "http://ugoite.example/mcp";
        let credential_id = uuid::Uuid::now_v7();
        let space_uid = uuid::Uuid::now_v7();
        let (base_url, requests, server) = spawn_http_server(vec![
            ("200 OK", json!({"resource": resource}).to_string()),
            (
                "201 Created",
                json!({
                    "device_code": "device-code",
                    "user_code": "ABCD-EFGH",
                    "verification_uri": "http://127.0.0.1/device",
                    "expires_in": 600,
                    "interval": 1
                })
                .to_string(),
            ),
            (
                "200 OK",
                json!({
                    "credential_id": credential_id,
                    "access_token": "mcp-access-token",
                    "refresh_token": "mcp-refresh-token",
                    "expires_in": 300,
                    "space_uid": space_uid
                })
                .to_string(),
            ),
        ]);
        let discovered = tokio::runtime::Runtime::new()
            .expect("create test runtime")
            .block_on(mcp_resource(&base_url))
            .expect("discover MCP resource");
        assert_eq!(discovered, resource);
        let session = tokio::runtime::Runtime::new()
            .expect("create test runtime")
            .block_on(login(
                &base_url,
                "test-device",
                Some(space_uid),
                vec!["read".to_string()],
                Some(discovered),
            ))
            .expect("complete MCP login");
        server.join().expect("join test server");
        let requests = requests.into_iter().collect::<Vec<_>>();
        assert_eq!(requests.len(), 3);
        assert!(requests[0].starts_with("GET /.well-known/oauth-protected-resource HTTP/1.1"));
        for request in &requests[1..] {
            let body = request.split_once("\r\n\r\n").expect("request body").1;
            let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
            assert_eq!(body["resource"], resource);
        }
        assert_eq!(session.resource.as_deref(), Some(resource));
        if session.private_key_pkcs8.is_none() {
            let _ = keyring::Entry::new("ugoite-cli", &session.credential_id.to_string())
                .and_then(|entry| entry.delete_credential());
        }
    }

    #[test]
    fn mcp_target_falls_back_to_integrated_server_root_for_api_base() {
        let resource = "http://ugoite.example/mcp";
        let (base_url, requests, server) = spawn_http_server(vec![
            (
                "404 Not Found",
                json!({"detail": "API route not found"}).to_string(),
            ),
            ("200 OK", json!({"resource": resource}).to_string()),
        ]);
        let target = tokio::runtime::Runtime::new()
            .expect("create test runtime")
            .block_on(mcp_target(&format!("{base_url}/api")))
            .expect("discover MCP target");
        server.join().expect("join test server");
        let requests = requests.into_iter().collect::<Vec<_>>();
        assert!(requests[0].starts_with("GET /api/.well-known/oauth-protected-resource HTTP/1.1"));
        assert!(requests[1].starts_with("GET /.well-known/oauth-protected-resource HTTP/1.1"));
        assert_eq!(target.resource, resource);
        assert_eq!(target.endpoint, format!("{base_url}/mcp"));
    }

    #[test]
    fn mcp_refresh_carries_the_saved_resource() {
        let signing_key = super::SigningKey::random(&mut super::OsRng);
        let public_key_jwk = public_jwk(&signing_key);
        let private_key = super::URL_SAFE_NO_PAD.encode(
            signing_key
                .to_pkcs8_der()
                .expect("encode private key")
                .as_bytes(),
        );
        let resource = "https://ugoite.example/mcp";
        let (base_url, requests, server) = spawn_http_server(vec![(
            "200 OK",
            json!({
                "access_token": "refreshed-mcp-access-token",
                "refresh_token": "rotated-mcp-refresh-token",
                "expires_in": 300
            })
            .to_string(),
        )]);
        let expired = super::AuthSession {
            credential_id: uuid::Uuid::now_v7(),
            device_name: "mcp-device".to_string(),
            public_key_jwk,
            private_key_pkcs8: Some(private_key),
            access_token: "expired-mcp-access-token".to_string(),
            refresh_token: "mcp-refresh-token".to_string(),
            expires_at: 0,
            base_url: base_url.clone(),
            resource: Some(resource.to_string()),
            space_uid: uuid::Uuid::now_v7(),
        };
        let session = tokio::runtime::Runtime::new()
            .expect("create test runtime")
            .block_on(refresh_session(&expired, &base_url))
            .expect("refresh MCP session")
            .expect("refreshed MCP session");
        server.join().expect("join test server");
        let request = requests.into_iter().next().expect("refresh request");
        let body = request.split_once("\r\n\r\n").expect("request body").1;
        let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
        assert_eq!(body["resource"], resource);
        assert_eq!(session.access_token, "refreshed-mcp-access-token");
        assert_eq!(session.resource.as_deref(), Some(resource));
    }
}

#[cfg(test)]
mod profile_tests {
    use super::redacted_profile;
    use crate::config::AuthSession;

    fn sample_session() -> AuthSession {
        AuthSession {
            credential_id: uuid::Uuid::now_v7(),
            device_name: "test-device".to_string(),
            public_key_jwk: serde_json::json!({"kty": "EC"}),
            private_key_pkcs8: Some("inline-private-key-material".to_string()),
            access_token: "secret-access-token".to_string(),
            refresh_token: "secret-refresh-token".to_string(),
            expires_at: 123,
            base_url: "https://ugoite.example.com".to_string(),
            resource: None,
            space_uid: uuid::Uuid::now_v7(),
        }
    }

    #[test]
    fn redacted_profile_never_contains_secrets() {
        let profile = serde_json::to_value(sample_session()).unwrap();
        let shown = redacted_profile("alice-work", &profile);
        let text = serde_json::to_string(&shown).unwrap();
        assert!(!text.contains("secret-access-token"));
        assert!(!text.contains("secret-refresh-token"));
        assert!(!text.contains("inline-private-key-material"));
        assert_eq!(shown["credential"], "alice-work");
        assert_eq!(shown["paired"], true);
    }

    #[test]
    fn named_profile_payload_carries_connection_without_toml() {
        // The stored profile is an opaque JSON value (persisted to the
        // user-global credential store, never to TOML): it must carry the
        // connection name alongside the session fields, while its display
        // projection still redacts every secret.
        let mut profile = serde_json::to_value(sample_session()).unwrap();
        profile["connection"] = serde_json::Value::String("work".to_string());
        assert_eq!(profile["connection"], "work");
        assert_eq!(profile["base_url"], "https://ugoite.example.com");
        let shown = redacted_profile("alice-work", &profile);
        let text = serde_json::to_string(&shown).unwrap();
        assert!(!text.contains("secret-access-token"));
        assert!(!text.contains("secret-refresh-token"));
    }
}
