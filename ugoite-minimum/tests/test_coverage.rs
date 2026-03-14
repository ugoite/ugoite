use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use ugoite_minimum::integrity::{
    checksum_hex, FakeIntegrityProvider, HmacIntegrityProvider, IntegrityProvider,
};
use ugoite_minimum::metadata::{
    is_reserved_metadata_column, is_reserved_metadata_form, metadata_columns, metadata_forms,
    register_metadata_columns, register_metadata_forms,
};
use ugoite_minimum::space::storage_type_and_root;
use ugoite_minimum::storage::{StorageBackend, StorageEntry};

#[derive(Clone, Default)]
struct MockStorage {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    dirs: Arc<Mutex<HashSet<String>>>,
}

#[async_trait]
impl StorageBackend for MockStorage {
    async fn exists(&self, path: &str) -> Result<bool> {
        let has_file = self
            .files
            .lock()
            .map_err(|_| anyhow!("files store lock poisoned"))?
            .contains_key(path);
        let has_dir = self
            .dirs
            .lock()
            .map_err(|_| anyhow!("dirs store lock poisoned"))?
            .contains(path);
        Ok(has_file || has_dir)
    }

    async fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.files
            .lock()
            .map_err(|_| anyhow!("files store lock poisoned"))?
            .get(path)
            .cloned()
            .ok_or_else(|| anyhow!("missing file: {path}"))
    }

    async fn write(&self, path: &str, data: Vec<u8>) -> Result<()> {
        self.files
            .lock()
            .map_err(|_| anyhow!("files store lock poisoned"))?
            .insert(path.to_string(), data);
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.dirs
            .lock()
            .map_err(|_| anyhow!("dirs store lock poisoned"))?
            .insert(path.to_string());
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<StorageEntry>> {
        let dirs = self
            .dirs
            .lock()
            .map_err(|_| anyhow!("dirs store lock poisoned"))?;
        let mut entries = dirs
            .iter()
            .filter(|dir| dir.starts_with(path) && dir.as_str() != path)
            .map(|dir| StorageEntry {
                name: dir.trim_start_matches(path).to_string(),
                is_dir: true,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }
}

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
fn test_metadata_req_form_005_reserved_metadata_columns_are_case_insensitive_and_extendable() {
    let columns = metadata_columns();
    assert!(columns.contains("title"));
    assert!(columns.contains("space_id"));
    assert!(is_reserved_metadata_column("Title"));
    assert!(is_reserved_metadata_column("SPACE_ID"));
    assert!(!is_reserved_metadata_column("issue777_custom_column"));

    register_metadata_columns(vec![
        "issue777_custom_column".to_string(),
        "Issue777CaseSensitive".to_string(),
    ]);

    let registered = metadata_columns();
    assert!(registered.contains("issue777_custom_column"));
    assert!(registered.contains("Issue777CaseSensitive"));
    assert!(is_reserved_metadata_column("ISSUE777_CUSTOM_COLUMN"));
    assert!(is_reserved_metadata_column("issue777casesensitive"));
}

#[test]
/// REQ-FORM-006
fn test_metadata_req_form_006_reserved_metadata_forms_are_trimmed_case_insensitive_and_extendable()
{
    let forms = metadata_forms();
    assert!(forms.contains("sql"));
    assert!(forms.contains("assets"));
    assert!(is_reserved_metadata_form("SQL"));
    assert!(is_reserved_metadata_form("assets"));
    assert!(!is_reserved_metadata_form("issue777_custom_form"));

    register_metadata_forms(vec!["  issue777_custom_form  ".to_string()]);

    let registered = metadata_forms();
    assert!(registered.contains("issue777_custom_form"));
    assert!(is_reserved_metadata_form("ISSUE777_CUSTOM_FORM"));
}

#[tokio::test]
/// REQ-STO-001
async fn test_storage_req_sto_001_json_helpers_use_storage_abstraction() -> Result<()> {
    let storage = MockStorage::default();
    storage.create_dir("spaces/").await?;
    storage.create_dir("spaces/demo/").await?;
    storage
        .write_json(
            "spaces/demo/meta.json",
            &json!({"id": "demo", "storage": {"type": "local"}}),
        )
        .await?;

    let meta: serde_json::Value = storage.read_json("spaces/demo/meta.json").await?;

    assert!(storage.exists("spaces/demo/meta.json").await?);
    assert!(storage.exists("spaces/demo/").await?);
    assert!(!storage.exists("spaces/missing/").await?);
    assert_eq!(meta["id"], "demo");
    assert_eq!(
        storage.list_dir("spaces/").await?,
        vec![StorageEntry {
            name: "demo/".to_string(),
            is_dir: true,
        }]
    );

    Ok(())
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
