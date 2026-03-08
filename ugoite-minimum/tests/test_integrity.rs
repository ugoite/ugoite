use ugoite_minimum::integrity::{checksum_hex, HmacIntegrityProvider, IntegrityProvider};

#[test]
/// REQ-INT-001
fn test_integrity_req_int_001_hmac_provider_matches_known_digest() {
    let provider = HmacIntegrityProvider::new(b"secret".to_vec());

    assert_eq!(
        checksum_hex(b"hello world"),
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    assert_eq!(
        provider.checksum("hello world"),
        "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
    assert_eq!(
        provider.signature("hello world"),
        "734cc62f32841568f45715aeb9f4d7891324e6d948e4c6c60c0621cdac48623a"
    );
    assert_eq!(
        provider.signature_bytes(b"hello world"),
        "734cc62f32841568f45715aeb9f4d7891324e6d948e4c6c60c0621cdac48623a"
    );
}
