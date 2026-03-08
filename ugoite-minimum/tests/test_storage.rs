use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
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

    let (storage_type, root, scheme) = storage_type_and_root("s3://bucket/prefix");
    assert_eq!(storage_type, "s3");
    assert_eq!(root, "prefix");
    assert_eq!(scheme, "s3");
}
