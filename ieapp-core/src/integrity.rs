use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use hmac::{Hmac, Mac};
use opendal::Operator;
use sha2::{Digest, Sha256};

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

    pub async fn from_workspace(op: &Operator, ws_name: &str) -> Result<Self> {
        // Logic to read global.json basically.
        // In the Rust architecture, workspace.rs handles global.json.
        // We might want to construct this by reading global.json directly or passing the key.
        // For now, let's assume we read global.json again.

        let global_path = "global.json";
        if !op.exists(global_path).await? {
            return Err(anyhow!("global.json not found"));
        }

        let bytes = op.read(global_path).await?;
        let global_config: serde_json::Value = serde_json::from_slice(&bytes.to_vec())?;

        let key_b64 = global_config["hmac_key"]
            .as_str()
            .ok_or_else(|| anyhow!("hmac_key missing in global.json"))?;

        let secret = general_purpose::STANDARD.decode(key_b64)?;

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
