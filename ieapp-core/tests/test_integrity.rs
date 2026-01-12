mod common;
use _ieapp_core::integrity::{FakeIntegrityProvider, IntegrityProvider};

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
