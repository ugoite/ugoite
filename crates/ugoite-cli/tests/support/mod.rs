//! Shared test process helper.
//!
//! This module intentionally provides no compatibility behavior: it
//! re-exports the standard process [`Command`] so tests invoke the
//! canonical CLI directly. Tests set up canonical config/context
//! explicitly (`config init`, `config connection set`, `space create`)
//! instead of relying on behind-the-scenes generation.

pub use std::process::Command;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use p256::ecdsa::SigningKey;

/// Fixed valid key for isolated test fixtures; production key generation uses
/// the operating system CSPRNG in the CLI auth module.
pub fn test_signing_key() -> SigningKey {
    SigningKey::from_bytes((&[7_u8; 32]).into()).expect("test signing key")
}

pub fn public_jwk(key: &SigningKey) -> serde_json::Value {
    let point = key.verifying_key().to_sec1_bytes();
    let point = point.as_ref();
    assert_eq!(point.len(), 65, "uncompressed P-256 point has 65 bytes");
    assert_eq!(point[0], 0x04, "uncompressed SEC1 point prefix");
    serde_json::json!({
        "kty": "EC",
        "crv": "P-256",
        "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
        "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
    })
}
