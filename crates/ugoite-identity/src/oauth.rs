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
    /// Human tokens carry the account generation captured at issuance.
    /// Agent tokens intentionally leave this unset.
    #[serde(default)]
    pub credential_generation: Option<u64>,
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
    let proof = verify_p256_jwt(assertion, registered_jwk, false)?;
    let payload = decode_jwt_payload(assertion)?;
    if ["htm", "htu", "ath", "nonce"]
        .iter()
        .any(|claim| payload.get(*claim).is_some())
    {
        bail!("client assertion contains DPoP proof claims");
    }
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
    let proof = verify_p256_jwt(proof_jwt, registered_jwk, true)?;
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

fn verify_p256_jwt(jwt: &str, registered_jwk: &Value, dpop: bool) -> Result<ProofClaims> {
    let (header_segment, payload_segment, signature_segment) = jwt_parts(jwt)?;
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header_segment)?)?;
    if header.get("alg").and_then(Value::as_str) != Some("ES256") {
        bail!("proof JWT must use ES256");
    }
    if dpop {
        if header.get("typ").and_then(Value::as_str) != Some("dpop+jwt") {
            bail!("DPoP proof must use typ dpop+jwt");
        }
    } else if header.get("typ").and_then(Value::as_str) == Some("dpop+jwt") {
        bail!("client assertion cannot use DPoP typ");
    }
    if let Some(presented) = header.get("jwk") {
        if jwk_thumbprint(presented)? != jwk_thumbprint(registered_jwk)? {
            bail!("proof key does not match registered credential");
        }
    } else if dpop {
        bail!("DPoP proof must embed its public JWK");
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

fn decode_jwt_payload(jwt: &str) -> Result<Value> {
    let (_, payload_segment, _) = jwt_parts(jwt)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::{signature::Signer, SigningKey};
    use serde_json::json;

    fn test_key_and_jwk() -> (SigningKey, Value) {
        let key = SigningKey::from_bytes((&[7_u8; 32]).into()).expect("test signing key");
        let point = key.verifying_key().to_encoded_point(false);
        let jwk = json!({
            "kty": "EC",
            "crv": "P-256",
            "x": URL_SAFE_NO_PAD.encode(point.x().expect("x")),
            "y": URL_SAFE_NO_PAD.encode(point.y().expect("y")),
        });
        (key, jwk)
    }

    fn signed_assertion(key: &SigningKey, jwk: &Value, payload: Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&json!({"alg":"ES256","typ":"JWT","jwk":jwk})).expect("header"),
        );
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).expect("payload"));
        let signing_input = format!("{header}.{payload}");
        let signature: Signature = key.sign(signing_input.as_bytes());
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    #[test]
    fn client_assertions_reject_dpop_claim_presence_but_keep_jwt_jwk_assertions() {
        let (key, jwk) = test_key_and_jwk();
        let base = json!({
            "iss": "client",
            "sub": "client",
            "aud": "audience",
            "iat": chrono::Utc::now().timestamp(),
            "jti": "assertion-1"
        });
        assert!(verify_client_assertion(
            &signed_assertion(&key, &jwk, base.clone()),
            &jwk,
            "audience"
        )
        .is_ok());
        for claim in ["nonce", "htm", "htu", "ath"] {
            let mut payload = base.clone();
            payload[claim] = Value::Null;
            assert!(
                verify_client_assertion(&signed_assertion(&key, &jwk, payload), &jwk, "audience")
                    .is_err(),
                "client assertion must reject present {claim} claim"
            );
        }
    }
}
