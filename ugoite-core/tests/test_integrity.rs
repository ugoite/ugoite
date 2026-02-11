mod common;
use _ugoite_core::integrity::{FakeIntegrityProvider, IntegrityProvider, RealIntegrityProvider};
use _ugoite_core::space;
use common::setup_operator;

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
