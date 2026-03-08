use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

pub trait IntegrityProvider {
    fn checksum(&self, content: &str) -> String;
    fn signature(&self, content: &str) -> String;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FakeIntegrityProvider;

impl IntegrityProvider for FakeIntegrityProvider {
    fn checksum(&self, content: &str) -> String {
        format!("mock-checksum-{}", content.len())
    }

    fn signature(&self, content: &str) -> String {
        format!("mock-signature-{}", content.len())
    }
}

#[derive(Debug, Clone)]
pub struct HmacIntegrityProvider {
    secret: Vec<u8>,
}

impl HmacIntegrityProvider {
    pub fn new(secret: Vec<u8>) -> Self {
        Self { secret }
    }

    pub fn signature_bytes(&self, content: &[u8]) -> String {
        type HmacSha256 = Hmac<Sha256>;
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).expect("HMAC can take key of any size");
        mac.update(content);
        hex::encode(mac.finalize().into_bytes())
    }
}

impl IntegrityProvider for HmacIntegrityProvider {
    fn checksum(&self, content: &str) -> String {
        checksum_hex(content.as_bytes())
    }

    fn signature(&self, content: &str) -> String {
        self.signature_bytes(content.as_bytes())
    }
}

pub fn checksum_hex(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hex::encode(hasher.finalize())
}
