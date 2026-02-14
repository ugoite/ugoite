use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use hmac::{Hmac, Mac};
use opendal::Operator;
use rand::RngExt;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub trait IntegrityProvider {
    fn checksum(&self, content: &str) -> String;
    fn signature(&self, content: &str) -> String;
}

pub struct FakeIntegrityProvider;

impl IntegrityProvider for FakeIntegrityProvider {
    fn checksum(&self, content: &str) -> String {
        format!("mock-checksum-{}", content.len())
    }
    fn signature(&self, content: &str) -> String {
        format!("mock-signature-{}", content.len())
    }
}

pub struct RealIntegrityProvider {
    secret: Vec<u8>,
}

impl RealIntegrityProvider {
    pub fn new(secret: Vec<u8>) -> Self {
        Self { secret }
    }

    pub async fn from_space(op: &Operator, _space_name: &str) -> Result<Self> {
        let (_key_id, secret) = load_hmac_material(op, _space_name).await?;
        Ok(Self::new(secret))
    }
}

impl IntegrityProvider for RealIntegrityProvider {
    fn checksum(&self, content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        hex::encode(hasher.finalize())
    }

    fn signature(&self, content: &str) -> String {
        type HmacSha256 = Hmac<Sha256>;
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC can take key of any size");
        mac.update(content.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
}

async fn ensure_space_hmac(op: &Operator, space_name: &str) -> Result<()> {
    let meta_path = format!("spaces/{}/meta.json", space_name);
    if !op.exists(&meta_path).await? {
        return Err(anyhow!("Space not found: {}", space_name));
    }
    let bytes = op.read(&meta_path).await?;
    let mut meta: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;
    let has_key = meta
        .get("hmac_key")
        .and_then(|v| v.as_str())
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    if has_key {
        return Ok(());
    }
    let mut key_bytes = [0u8; 32];
    rand::rng().fill(&mut key_bytes);
    let hmac_key = general_purpose::STANDARD.encode(key_bytes);
    let key_id = format!("key-{}", Uuid::new_v4().simple());

    meta["hmac_key_id"] = serde_json::Value::String(key_id);
    meta["hmac_key"] = serde_json::Value::String(hmac_key);
    meta["last_rotation"] = serde_json::Value::String(Utc::now().to_rfc3339());

    op.write(&meta_path, serde_json::to_vec_pretty(&meta)?)
        .await?;
    Ok(())
}

pub async fn load_hmac_material(op: &Operator, space_name: &str) -> Result<(String, Vec<u8>)> {
    ensure_space_hmac(op, space_name).await?;

    let meta_path = format!("spaces/{}/meta.json", space_name);
    let bytes = op.read(&meta_path).await?;
    let meta: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;

    let key_b64 = meta
        .get("hmac_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("hmac_key missing in space meta.json"))?;
    let key_id = meta
        .get("hmac_key_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let secret = general_purpose::STANDARD.decode(key_b64)?;
    Ok((key_id, secret))
}

pub async fn load_response_hmac_material(op: &Operator) -> Result<(String, Vec<u8>)> {
    let path = "hmac.json";
    if !op.exists(path).await? {
        let mut key_bytes = [0u8; 32];
        rand::rng().fill(&mut key_bytes);
        let payload = serde_json::json!({
            "hmac_key_id": format!("key-{}", Uuid::new_v4().simple()),
            "hmac_key": general_purpose::STANDARD.encode(key_bytes),
            "last_rotation": Utc::now().to_rfc3339(),
        });
        op.write(path, serde_json::to_vec_pretty(&payload)?).await?;
    }
    let bytes = op.read(path).await?;
    let payload: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;
    let key_b64 = payload
        .get("hmac_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("hmac_key missing in hmac.json"))?;
    let key_id = payload
        .get("hmac_key_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();
    let secret = general_purpose::STANDARD.decode(key_b64)?;
    Ok((key_id, secret))
}

pub async fn build_response_signature(op: &Operator, body: &[u8]) -> Result<(String, String)> {
    let (key_id, secret) = load_response_hmac_material(op).await?;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&secret)?;
    mac.update(body);
    let signature = hex::encode(mac.finalize().into_bytes());
    Ok((key_id, signature))
}
