use crate::config::{
    clear_auth_session, load_auth_session, load_config, print_json, save_auth_session,
    validated_base_url, AuthSession, EndpointMode,
};
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::Utc;
use clap::{Args, Subcommand};
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
                "private_key_storage": if session.private_key_pkcs8.is_some() { "owner_only_file" } else { "os_keychain" },
            })).unwrap_or_else(|| json!({"paired": false}));
            print_json(&profile);
        }
        AuthSubCmd::Login {
            device_name,
            space_uid,
            actions,
        } => {
            let config = load_config();
            if config.mode == EndpointMode::Core {
                bail!("auth login requires backend or api mode");
            }
            let base = validated_base_url(&config)?
                .ok_or_else(|| anyhow!("remote endpoint is missing"))?;
            login(&base, &device_name, space_uid, actions).await?;
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
) -> Result<()> {
    let signing_key = SigningKey::random(&mut OsRng);
    let public_key_jwk = public_jwk(&signing_key);
    let response = reqwest::Client::new()
        .post(format!(
            "{}/oauth/device/authorization",
            base.trim_end_matches('/')
        ))
        .json(&json!({
            "device_name": device_name,
            "public_key_jwk": public_key_jwk,
            "space_uid": space_uid,
            "requested_actions": actions,
        }))
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
            .json(&json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code",
                "device_code": device_code,
                "client_assertion": assertion,
            }))
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

pub async fn active_session(base_url: &str) -> Result<Option<AuthSession>> {
    let Some(mut session) = load_auth_session() else {
        return Ok(None);
    };
    if session.base_url.trim_end_matches('/') != base_url.trim_end_matches('/') {
        bail!("saved CLI credential belongs to a different server; run `ugoite auth login`");
    }
    if session.expires_at > Utc::now().timestamp() + 30 {
        return Ok(Some(session));
    }
    let key = load_signing_key(&session)?;
    let token_url = format!("{}/oauth/token", base_url.trim_end_matches('/'));
    let assertion = client_assertion(&key, &session.public_key_jwk, &token_url)?;
    let response = reqwest::Client::new()
        .post(&token_url)
        .json(&json!({
            "grant_type": "refresh_token",
            "refresh_token": session.refresh_token,
            "client_assertion": assertion,
        }))
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
    use super::canonical_dpop_htu;

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
}
