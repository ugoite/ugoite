use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use hmac::{Hmac, Mac};
use opendal::Operator;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

    pub async fn from_workspace(op: &Operator, _ws_name: &str) -> Result<Self> {
        let (_key_id, secret) = load_hmac_material(op).await?;
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

async fn ensure_global_json(op: &Operator) -> Result<()> {
    if op.exists("global.json").await? {
        return Ok(());
    }

    let mut key_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut key_bytes);
    let hmac_key = general_purpose::STANDARD.encode(key_bytes);
    let key_id = format!("key-{}", Uuid::new_v4().simple());

    let payload = serde_json::json!({
        "version": 1,
        "default_storage": "",
        "workspaces": [],
        "hmac_key_id": key_id,
        "hmac_key": hmac_key,
        "last_rotation": Utc::now().to_rfc3339(),
    });

    op.write("global.json", serde_json::to_vec_pretty(&payload)?)
        .await?;
    Ok(())
}

pub async fn load_hmac_material(op: &Operator) -> Result<(String, Vec<u8>)> {
    ensure_global_json(op).await?;

    let bytes = op.read("global.json").await?;
    let global_config: HashMap<String, serde_json::Value> =
        serde_json::from_slice(&bytes.to_vec())?;

    let key_b64 = global_config
        .get("hmac_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("hmac_key missing in global.json"))?;
    let key_id = global_config
        .get("hmac_key_id")
        .and_then(|v| v.as_str())
        .unwrap_or("default")
        .to_string();

    let secret = general_purpose::STANDARD.decode(key_b64)?;
    Ok((key_id, secret))
}

pub async fn build_response_signature(op: &Operator, body: &[u8]) -> Result<(String, String)> {
    let (key_id, secret) = load_hmac_material(op).await?;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&secret)?;
    mac.update(body);
    let signature = hex::encode(mac.finalize().into_bytes());
    Ok((key_id, signature))
}
