//! OAuth access-token and proof-of-possession primitives.

use crate::node_identity::token_hash;
use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AccessTokenClaims {
    pub iss: String,
    pub node_id: Uuid,
    pub sub: Uuid,
    pub principal_type: String,
    #[serde(default)]
    pub actor_principal_id: Option<Uuid>,
    pub aud: String,
    pub space_uid: Uuid,
    pub granted_actions: BTreeSet<String>,
    pub actor_chain: Vec<Uuid>,
    pub exp: i64,
    pub iat: i64,
    pub jti: Uuid,
    pub credential_id: Uuid,
    pub cnf: Confirmation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Confirmation {
    pub jkt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProofClaims {
    pub iss: Option<String>,
    pub sub: Option<String>,
    pub htm: Option<String>,
    pub htu: Option<String>,
    pub ath: Option<String>,
    pub aud: Option<String>,
    pub iat: i64,
    pub exp: Option<i64>,
    pub jti: String,
}

pub fn verify_client_assertion(
    assertion: &str,
    registered_jwk: &Value,
    audience: &str,
) -> Result<ProofClaims> {
    let proof = verify_p256_jwt(assertion, registered_jwk)?;
    if proof.aud.as_deref() != Some(audience) {
        bail!("client assertion audience mismatch");
    }
    if proof.iss.as_deref().is_none_or(str::is_empty) || proof.iss != proof.sub {
        bail!("client assertion requires matching non-empty iss and sub");
    }
    validate_proof_time(&proof)?;
    Ok(proof)
}

pub fn verify_dpop_proof(
    proof_jwt: &str,
    registered_jwk: &Value,
    method: &str,
    uri: &str,
    access_token: &str,
) -> Result<ProofClaims> {
    let proof = verify_p256_jwt(proof_jwt, registered_jwk)?;
    validate_proof_time(&proof)?;
    if proof.htm.as_deref().map(str::to_uppercase).as_deref() != Some(&method.to_uppercase()) {
        bail!("DPoP htm mismatch");
    }
    if proof.htu.as_deref() != Some(uri) {
        bail!("DPoP htu mismatch");
    }
    let expected_ath = URL_SAFE_NO_PAD.encode(Sha256::digest(access_token.as_bytes()));
    if proof.ath.as_deref() != Some(expected_ath.as_str()) {
        bail!("DPoP ath mismatch");
    }
    Ok(proof)
}

pub fn jwk_thumbprint(jwk: &Value) -> Result<String> {
    let x = jwk
        .get("x")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("JWK x is required"))?;
    let y = jwk
        .get("y")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("JWK y is required"))?;
    if jwk.get("kty").and_then(Value::as_str) != Some("EC")
        || jwk.get("crv").and_then(Value::as_str) != Some("P-256")
    {
        bail!("only EC P-256 proof keys are supported");
    }
    let canonical = format!(r#"{{"crv":"P-256","kty":"EC","x":"{x}","y":"{y}"}}"#);
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(canonical.as_bytes())))
}

fn verify_p256_jwt(jwt: &str, registered_jwk: &Value) -> Result<ProofClaims> {
    let (header_segment, payload_segment, signature_segment) = jwt_parts(jwt)?;
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header_segment)?)?;
    if header.get("alg").and_then(Value::as_str) != Some("ES256") {
        bail!("proof JWT must use ES256");
    }
    if let Some(presented) = header.get("jwk") {
        if jwk_thumbprint(presented)? != jwk_thumbprint(registered_jwk)? {
            bail!("proof key does not match registered credential");
        }
    }
    let x = URL_SAFE_NO_PAD.decode(
        registered_jwk["x"]
            .as_str()
            .ok_or_else(|| anyhow!("JWK x missing"))?,
    )?;
    let y = URL_SAFE_NO_PAD.decode(
        registered_jwk["y"]
            .as_str()
            .ok_or_else(|| anyhow!("JWK y missing"))?,
    )?;
    if x.len() != 32 || y.len() != 32 {
        bail!("invalid P-256 coordinates");
    }
    let mut encoded = Vec::with_capacity(65);
    encoded.push(4);
    encoded.extend_from_slice(&x);
    encoded.extend_from_slice(&y);
    let key = VerifyingKey::from_sec1_bytes(&encoded).context("invalid P-256 public key")?;
    let signature_bytes = URL_SAFE_NO_PAD.decode(signature_segment)?;
    let signature = Signature::from_slice(&signature_bytes).context("invalid ES256 signature")?;
    key.verify(
        format!("{header_segment}.{payload_segment}").as_bytes(),
        &signature,
    )
    .context("proof signature verification failed")?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload_segment)?)
        .context("invalid proof claims")
}

fn validate_proof_time(proof: &ProofClaims) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    if proof.iat < now - 300
        || proof.iat > now + 60
        || proof.exp.is_some_and(|exp| exp <= now)
        || proof.jti.trim().is_empty()
    {
        bail!("proof is expired or outside the allowed clock window");
    }
    Ok(())
}

fn jwt_parts(jwt: &str) -> Result<(&str, &str, &str)> {
    let mut parts = jwt.split('.');
    let header = parts.next().ok_or_else(|| anyhow!("JWT header missing"))?;
    let payload = parts.next().ok_or_else(|| anyhow!("JWT payload missing"))?;
    let signature = parts
        .next()
        .ok_or_else(|| anyhow!("JWT signature missing"))?;
    if parts.next().is_some() || header.is_empty() || payload.is_empty() || signature.is_empty() {
        bail!("malformed JWT");
    }
    Ok((header, payload, signature))
}

pub fn access_token_hash(token: &str) -> String {
    token_hash(token)
}
