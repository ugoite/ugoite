use ugoite_domain::integrity::{
    checksum_hex, FakeIntegrityProvider, HmacIntegrityProvider, IntegrityProvider,
};
use ugoite_domain::metadata::{is_reserved_metadata_column, is_reserved_metadata_form};
use ugoite_domain::space::storage_type_and_root;
use ugoite_domain::text::compute_word_count;

#[test]
/// REQ-INT-001
fn test_integrity_req_int_001_hmac_provider_matches_known_digest() {
    let fake = FakeIntegrityProvider;
    assert_eq!(fake.checksum("hello world"), "mock-checksum-11");
    assert_eq!(fake.signature("hello world"), "mock-signature-11");

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

#[test]
/// REQ-FORM-005
fn test_metadata_req_form_005_reserved_metadata_columns_are_case_insensitive_and_fixed() {
    let registry: serde_json::Value =
        serde_json::from_str(include_str!("../src/metadata.json")).expect("metadata registry");
    let columns = registry["columns"].as_array().expect("metadata columns");
    // Title-less Entry (REQ-ENTRY-011): `title` is a normal user-definable
    // Form field, not a reserved metadata column.
    assert!(!columns.iter().any(|name| name == "title"));
    assert!(columns.iter().any(|name| name == "space_id"));
    assert!(!is_reserved_metadata_column("Title"));
    assert!(is_reserved_metadata_column("SPACE_ID"));
    assert!(is_reserved_metadata_column(" space_id "));
    for name in columns {
        let name = name.as_str().expect("metadata column name");
        assert!(is_reserved_metadata_column(name));
        assert!(is_reserved_metadata_column(&format!(
            " {} ",
            name.to_uppercase()
        )));
    }
    assert!(!is_reserved_metadata_column("issue777_custom_column"));
}

#[test]
/// REQ-FORM-006
fn test_metadata_req_form_006_reserved_metadata_forms_are_trimmed_case_insensitive_and_fixed() {
    let registry: serde_json::Value =
        serde_json::from_str(include_str!("../src/metadata.json")).expect("metadata registry");
    let forms = registry["forms"].as_array().expect("metadata forms");
    assert!(forms.iter().any(|name| name == "SQL"));
    assert!(is_reserved_metadata_form("SQL"));
    assert!(!is_reserved_metadata_form("assets"));
    assert!(is_reserved_metadata_form(" sql "));
    for name in forms {
        let name = name.as_str().expect("metadata form name");
        assert!(is_reserved_metadata_form(name));
        assert!(is_reserved_metadata_form(&format!(
            " {} ",
            name.to_uppercase()
        )));
    }
    assert!(!is_reserved_metadata_form("issue777_custom_form"));
}

#[test]
/// REQ-STO-004
fn test_space_req_sto_004_storage_type_and_root_normalizes_local_and_remote_uris() {
    let (storage_type, root, scheme) = storage_type_and_root("fs:///tmp/ugoite");
    assert_eq!(storage_type, "local");
    assert_eq!(root, "/tmp/ugoite");
    assert_eq!(scheme, "fs");

    let (storage_type, root, scheme) = storage_type_and_root("file:///var/lib/ugoite");
    assert_eq!(storage_type, "local");
    assert_eq!(root, "/var/lib/ugoite");
    assert_eq!(scheme, "file");

    let (storage_type, root, scheme) = storage_type_and_root("s3://bucket/prefix");
    assert_eq!(storage_type, "s3");
    assert_eq!(root, "prefix");
    assert_eq!(scheme, "s3");

    let (storage_type, root, scheme) = storage_type_and_root("/var/lib/ugoite");
    assert_eq!(storage_type, "local");
    assert_eq!(root, "/var/lib/ugoite");
    assert_eq!(scheme, "file");
}

#[test]
/// REQ-SRCH-007
fn test_text_req_idx_005_word_count_portable_for_minimum_coverage_gate() {
    assert_eq!(compute_word_count("One two three"), 3);
    assert_eq!(compute_word_count("  tabs\tand\nnewlines  still count "), 5);
}
