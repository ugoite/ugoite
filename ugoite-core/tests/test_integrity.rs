mod common;
use _ugoite_core::integrity::{FakeIntegrityProvider, IntegrityProvider, RealIntegrityProvider};
use _ugoite_core::space;
use base64::{engine::general_purpose, Engine as _};
use common::setup_operator;
use serde_json::json;

#[test]
/// REQ-INT-001
fn test_integrity_req_int_001_fake_integrity_provider() {
    let provider = FakeIntegrityProvider;
    let content = "hello";

    let checksum = provider.checksum(content);
    assert!(checksum.starts_with("mock-checksum-"));
    assert!(checksum.contains(&content.len().to_string()));

    let signature = provider.signature(content);
    assert!(signature.starts_with("mock-signature-"));
}

#[tokio::test]
/// REQ-INT-001
async fn test_integrity_req_int_001_real_integrity_provider() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;

    // Test loading from space
    let provider = RealIntegrityProvider::from_space(&op, "test-space").await;
    assert!(provider.is_ok());
    let provider = provider.unwrap();

    let content = "hello world";
    let checksum = provider.checksum(content);
    let signature = provider.signature(content);

    // Check SHA-256 for "hello world"
    // b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
    assert_eq!(
        checksum,
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );

    // Signature should be valid hex and different from checksum
    assert_ne!(checksum, signature);
    assert_eq!(signature.len(), 64); // SHA-256 hex is 64 chars

    Ok(())
}

#[tokio::test]
/// REQ-INT-003
async fn test_response_hmac_material_is_space_scoped() -> anyhow::Result<()> {
    let op = setup_operator()?;

    let (key_id, secret) =
        _ugoite_core::integrity::load_response_hmac_material(&op, "default").await?;
    let payload: serde_json::Value =
        serde_json::from_slice(&op.read("spaces/default/hmac.json").await?.to_vec())?;

    assert_eq!(payload["hmac_key_id"], key_id);
    assert_eq!(
        general_purpose::STANDARD.decode(payload["hmac_key"].as_str().unwrap())?,
        secret
    );
    assert!(op.read("hmac.json").await.is_err());

    Ok(())
}

#[tokio::test]
/// REQ-INT-003
async fn test_response_hmac_material_rejects_invalid_space_id() -> anyhow::Result<()> {
    let op = setup_operator()?;

    let err = _ugoite_core::integrity::load_response_hmac_material(&op, "../escape")
        .await
        .expect_err("invalid space id should be rejected");

    assert!(err.to_string().contains("invalid space_id"));
    Ok(())
}

#[tokio::test]
/// REQ-INT-003
async fn test_response_hmac_material_defaults_missing_key_id() -> anyhow::Result<()> {
    let op = setup_operator()?;
    op.create_dir("spaces/").await?;
    op.create_dir("spaces/default/").await?;
    op.write(
        "spaces/default/hmac.json",
        serde_json::to_vec(&json!({
            "hmac_key": general_purpose::STANDARD.encode([b'y'; 32]),
            "last_rotation": "2025-01-01T00:00:00+00:00",
        }))?,
    )
    .await?;

    let (key_id, secret) =
        _ugoite_core::integrity::load_response_hmac_material(&op, "default").await?;

    assert_eq!(key_id, "default");
    assert_eq!(secret, vec![b'y'; 32]);
    Ok(())
}

#[tokio::test]
/// REQ-INT-003
async fn test_response_hmac_material_rejects_missing_hmac_key() -> anyhow::Result<()> {
    let op = setup_operator()?;
    op.create_dir("spaces/").await?;
    op.create_dir("spaces/default/").await?;
    op.write(
        "spaces/default/hmac.json",
        serde_json::to_vec(&json!({
            "hmac_key_id": "missing-key",
            "last_rotation": "2025-01-01T00:00:00+00:00",
        }))?,
    )
    .await?;

    let err = _ugoite_core::integrity::load_response_hmac_material(&op, "default")
        .await
        .expect_err("missing hmac_key should be rejected");

    assert!(err.to_string().contains("hmac_key missing"));
    Ok(())
}
