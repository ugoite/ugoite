use crate::config::{
    clear_auth_session, load_auth_session, load_config, print_json, save_auth_session,
    validated_base_url, AuthSession, EndpointMode,
};
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
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub const DEFAULT_DEVICE_ACTIONS: &str = "read,create,update";

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
    Profile,
    /// Pair this terminal without requiring a browser on the terminal itself.
    Login {
        #[arg(long, default_value = "Ugoite CLI")]
        device_name: String,
        #[arg(long)]
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
    },
    /// Revoke local access by deleting the local device credential.
    Logout,
}

pub async fn run(cmd: AuthCmd) -> Result<()> {
    match cmd.sub {
        AuthSubCmd::Profile => {
            let profile = load_auth_session().map(|session| json!({
                "paired": true,
                "credential_id": session.credential_id,
                "device_name": session.device_name,
                "space_uid": session.space_uid,
                "access_token_expires_at": session.expires_at,
                "credential_target": if session.resource.is_some() { "mcp" } else { "rest" },
                "private_key_storage": if session.private_key_pkcs8.is_some() { "owner_only_file" } else { "os_keychain" },
            })).unwrap_or_else(|| json!({"paired": false}));
            print_json(&profile);
        }
        AuthSubCmd::Login {
            device_name,
            space_uid,
            actions,
            target,
        } => {
            let config = load_config();
            if config.mode == EndpointMode::Core {
                bail!("auth login requires backend or api mode");
            }
            let base = validated_base_url(&config)?
                .ok_or_else(|| anyhow!("remote endpoint is missing"))?;
            let resource = match target {
                AuthLoginTarget::Rest => None,
                AuthLoginTarget::Mcp => Some(mcp_resource(&base).await?),
            };
            login(&base, &device_name, space_uid, actions, resource).await?;
        }
        AuthSubCmd::Logout => {
            if let Some(session) = load_auth_session() {
                if session.private_key_pkcs8.is_none() {
                    let _ = keyring::Entry::new("ugoite-cli", &session.credential_id.to_string())
                        .and_then(|entry| entry.delete_credential());
                }
            }
            clear_auth_session()?;
            println!("Local CLI credential removed.");
        }
    }
    Ok(())
}

async fn login(
    base: &str,
    device_name: &str,
    space_uid: Option<Uuid>,
    actions: Vec<String>,
    resource: Option<String>,
) -> Result<()> {
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
    eprintln!("Open {verification_uri} on any signed-in device and approve code {user_code}.");
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
        space_uid: token["space_uid"]
            .as_str()
            .ok_or_else(|| anyhow!("token response omitted space_uid"))?
            .parse()?,
    };
    let path = save_auth_session(&session)?;
    println!(
        "Paired device {} for Space {}. Credential metadata saved to {}.",
        session.credential_id,
        session.space_uid,
        path.display()
    );
    Ok(())
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

pub async fn active_session(base_url: &str) -> Result<Option<AuthSession>> {
    active_session_for(base_url, None).await
}

pub async fn active_session_for(
    base_url: &str,
    resource: Option<&str>,
) -> Result<Option<AuthSession>> {
    let Some(mut session) = load_auth_session() else {
        return Ok(None);
    };
    if session.base_url.trim_end_matches('/') != base_url.trim_end_matches('/') {
        bail!("saved CLI credential belongs to a different server; run `ugoite auth login`");
    }
    if session.resource.as_deref() != resource {
        let login_command = if resource.is_some() {
            "ugoite auth login --for mcp"
        } else {
            "ugoite auth login"
        };
        bail!("saved CLI credential targets a different protected resource; run `{login_command}`");
    }
    if session.expires_at > Utc::now().timestamp() + 30 {
        return Ok(Some(session));
    }
    let key = load_signing_key(&session)?;
    let token_url = format!("{}/oauth/token", base_url.trim_end_matches('/'));
    let assertion = client_assertion(&key, &session.public_key_jwk, &token_url)?;
    let response = reqwest::Client::new()
        .post(&token_url)
        .json(&oauth_payload(
            json!({
                "grant_type": "refresh_token",
                "refresh_token": session.refresh_token,
                "client_assertion": assertion,
            }),
            resource,
        ))
        .send()
        .await
        .context("refresh CLI access token")?;
    let status = response.status();
    let payload: Value = response.json().await?;
    if !status.is_success() {
        bail!("CLI credential refresh failed: {payload}");
    }
    session.access_token = payload["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("refresh response omitted access_token"))?
        .to_string();
    session.refresh_token = payload["refresh_token"]
        .as_str()
        .ok_or_else(|| anyhow!("refresh response omitted refresh_token"))?
        .to_string();
    session.expires_at = Utc::now().timestamp() + payload["expires_in"].as_i64().unwrap_or(300);
    save_auth_session(&session)?;
    Ok(Some(session))
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
mod tests {
    use super::{
        active_session_for, api_base_root, canonical_dpop_htu, load_auth_session, login,
        mcp_resource, mcp_target, oauth_payload, public_jwk, save_auth_session,
    };
    use base64::Engine as _;
    use p256::pkcs8::EncodePrivateKey;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("cli-config.json");
        let previous_path = std::env::var_os("UGOITE_CLI_CONFIG_PATH");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);

        let resource = "http://ugoite.example/mcp";
        let credential_id = uuid::Uuid::now_v7();
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
                    "space_uid": uuid::Uuid::nil()
                })
                .to_string(),
            ),
        ]);
        let discovered = tokio::runtime::Runtime::new()
            .expect("create test runtime")
            .block_on(mcp_resource(&base_url))
            .expect("discover MCP resource");
        assert_eq!(discovered, resource);
        tokio::runtime::Runtime::new()
            .expect("create test runtime")
            .block_on(login(
                &base_url,
                "test-device",
                Some(uuid::Uuid::nil()),
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
        assert_eq!(
            load_auth_session()
                .expect("saved MCP session")
                .resource
                .as_deref(),
            Some(resource)
        );
        if let Some(session) = load_auth_session() {
            if session.private_key_pkcs8.is_none() {
                let _ = keyring::Entry::new("ugoite-cli", &session.credential_id.to_string())
                    .and_then(|entry| entry.delete_credential());
            }
        }

        if let Some(path) = previous_path {
            std::env::set_var("UGOITE_CLI_CONFIG_PATH", path);
        } else {
            std::env::remove_var("UGOITE_CLI_CONFIG_PATH");
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
        let _guard = env_lock().lock().expect("env lock");
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("cli-config.json");
        let previous_path = std::env::var_os("UGOITE_CLI_CONFIG_PATH");
        std::env::set_var("UGOITE_CLI_CONFIG_PATH", &config_path);

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
        save_auth_session(&super::AuthSession {
            credential_id: uuid::Uuid::nil(),
            device_name: "mcp-device".to_string(),
            public_key_jwk,
            private_key_pkcs8: Some(private_key),
            access_token: "expired-mcp-access-token".to_string(),
            refresh_token: "mcp-refresh-token".to_string(),
            expires_at: 0,
            base_url: base_url.clone(),
            resource: Some(resource.to_string()),
            space_uid: uuid::Uuid::nil(),
        })
        .expect("save expired MCP session");
        let session = tokio::runtime::Runtime::new()
            .expect("create test runtime")
            .block_on(active_session_for(&base_url, Some(resource)))
            .expect("refresh MCP session")
            .expect("saved MCP session");
        server.join().expect("join test server");
        let request = requests.into_iter().next().expect("refresh request");
        let body = request.split_once("\r\n\r\n").expect("request body").1;
        let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
        assert_eq!(body["resource"], resource);
        assert_eq!(session.access_token, "refreshed-mcp-access-token");
        assert_eq!(session.resource.as_deref(), Some(resource));

        if let Some(path) = previous_path {
            std::env::set_var("UGOITE_CLI_CONFIG_PATH", path);
        } else {
            std::env::remove_var("UGOITE_CLI_CONFIG_PATH");
        }
    }
}
