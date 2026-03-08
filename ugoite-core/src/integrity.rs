use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use opendal::Operator;
use rand::RngExt;
use serde_json::Value;
use uuid::Uuid;

use crate::storage::{OpendalStorage, StorageBackend};
use ugoite_minimum::integrity::HmacIntegrityProvider;
pub use ugoite_minimum::integrity::{FakeIntegrityProvider, IntegrityProvider};

pub struct RealIntegrityProvider {
    inner: HmacIntegrityProvider,
}

impl RealIntegrityProvider {
    pub fn new(secret: Vec<u8>) -> Self {
        Self {
            inner: HmacIntegrityProvider::new(secret),
        }
    }

    pub async fn from_space(op: &Operator, space_name: &str) -> Result<Self> {
        let storage = OpendalStorage::from_operator(op);
        Self::from_storage(&storage, space_name).await
    }

    async fn from_storage<S: StorageBackend + ?Sized>(
        storage: &S,
        space_name: &str,
    ) -> Result<Self> {
        let (_key_id, secret) = load_hmac_material_with_storage(storage, space_name).await?;
        Ok(Self::new(secret))
    }

    pub fn signature_bytes(&self, body: &[u8]) -> String {
        self.inner.signature_bytes(body)
    }
}

impl IntegrityProvider for RealIntegrityProvider {
    fn checksum(&self, content: &str) -> String {
        self.inner.checksum(content)
    }

    fn signature(&self, content: &str) -> String {
        self.inner.signature(content)
    }
}

fn generate_hmac_payload() -> Value {
    let mut key_bytes = [0u8; 32];
    rand::rng().fill(&mut key_bytes);
    serde_json::json!({
        "hmac_key_id": format!("key-{}", Uuid::new_v4().simple()),
        "hmac_key": general_purpose::STANDARD.encode(key_bytes),
        "last_rotation": Utc::now().to_rfc3339(),
    })
}

async fn ensure_space_hmac_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    space_name: &str,
) -> Result<()> {
    let meta_path = format!("spaces/{space_name}/meta.json");
    if !storage.exists(&meta_path).await? {
        return Err(anyhow!("Space not found: {space_name}"));
    }
    let mut meta: Value = storage.read_json(&meta_path).await?;
    let has_key = meta
        .get("hmac_key")
        .and_then(|value| value.as_str())
        .map(|value| !value.is_empty())
        .unwrap_or(false);
    if has_key {
        return Ok(());
    }

    let payload = generate_hmac_payload();
    meta["hmac_key_id"] = payload["hmac_key_id"].clone();
    meta["hmac_key"] = payload["hmac_key"].clone();
    meta["last_rotation"] = payload["last_rotation"].clone();
    storage.write_json(&meta_path, &meta).await?;
    Ok(())
}

async fn load_hmac_material_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
    space_name: &str,
) -> Result<(String, Vec<u8>)> {
    ensure_space_hmac_with_storage(storage, space_name).await?;

    let meta_path = format!("spaces/{space_name}/meta.json");
    let meta: Value = storage.read_json(&meta_path).await?;

    let key_b64 = meta
        .get("hmac_key")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("hmac_key missing in space meta.json"))?;
    let key_id = meta
        .get("hmac_key_id")
        .and_then(|value| value.as_str())
        .unwrap_or("default")
        .to_string();

    let secret = general_purpose::STANDARD.decode(key_b64)?;
    Ok((key_id, secret))
}

pub async fn load_hmac_material(op: &Operator, space_name: &str) -> Result<(String, Vec<u8>)> {
    let storage = OpendalStorage::from_operator(op);
    load_hmac_material_with_storage(&storage, space_name).await
}

async fn load_response_hmac_material_with_storage<S: StorageBackend + ?Sized>(
    storage: &S,
) -> Result<(String, Vec<u8>)> {
    let path = "hmac.json";
    if !storage.exists(path).await? {
        let payload = generate_hmac_payload();
        storage.write_json(path, &payload).await?;
    }
    let payload: Value = storage.read_json(path).await?;
    let key_b64 = payload
        .get("hmac_key")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("hmac_key missing in hmac.json"))?;
    let key_id = payload
        .get("hmac_key_id")
        .and_then(|value| value.as_str())
        .unwrap_or("default")
        .to_string();
    let secret = general_purpose::STANDARD.decode(key_b64)?;
    Ok((key_id, secret))
}

pub async fn load_response_hmac_material(op: &Operator) -> Result<(String, Vec<u8>)> {
    let storage = OpendalStorage::from_operator(op);
    load_response_hmac_material_with_storage(&storage).await
}

pub async fn build_response_signature(op: &Operator, body: &[u8]) -> Result<(String, String)> {
    let storage = OpendalStorage::from_operator(op);
    let (key_id, secret) = load_response_hmac_material_with_storage(&storage).await?;
    let provider = RealIntegrityProvider::new(secret);
    Ok((key_id, provider.signature_bytes(body)))
}
