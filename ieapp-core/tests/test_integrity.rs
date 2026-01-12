mod common;
use _ieapp_core::integrity::{FakeIntegrityProvider, IntegrityProvider};

#[test]
fn test_fake_integrity_provider() {
    let provider = FakeIntegrityProvider;
    let content = "hello";

    let checksum = provider.checksum(content);
    assert!(checksum.starts_with("mock-checksum-"));
    assert!(checksum.contains(&content.len().to_string()));

    let signature = provider.signature(content);
    assert!(signature.starts_with("mock-signature-"));
}
