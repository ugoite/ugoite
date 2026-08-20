use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use fs2::FileExt;
use opendal::{ErrorKind, Operator};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

const DELETED_RECORD: &[u8] = b"ugoite-control-tombstone-v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlRecord {
    pub value: Vec<u8>,
    pub version: String,
}

#[async_trait]
pub trait NodeControlStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<ControlRecord>>;
    async fn create_if_absent(&self, key: &str, value: Vec<u8>) -> Result<String>;
    async fn compare_and_swap(
        &self,
        key: &str,
        expected_version: &str,
        value: Vec<u8>,
    ) -> Result<String>;
    async fn delete_if_version(&self, key: &str, expected_version: &str) -> Result<()>;
    async fn list_prefix(&self, prefix: &str) -> Result<Vec<String>>;
}

#[derive(Clone)]
pub enum OpenDalNodeControlStore {
    Local {
        root: Arc<PathBuf>,
    },
    Remote {
        operator: Operator,
        prefix: Arc<str>,
    },
    Memory(Arc<Mutex<std::collections::BTreeMap<String, ControlRecord>>>),
}

impl OpenDalNodeControlStore {
    pub fn new(operator: Operator) -> Result<Self> {
        let scheme = operator.info().scheme();
        if matches!(scheme, "fs" | "file") {
            let root = Path::new(operator.info().root().as_str()).join("_ugoite");
            return Ok(Self::Local {
                root: Arc::new(root),
            });
        }
        if scheme == "memory" {
            bail!("in-memory node control storage is test-only");
        }
        let capabilities = operator.info().capability();
        if !capabilities.write_with_if_match || !capabilities.write_with_if_not_exists {
            bail!("configured node control storage lacks atomic conditional-write capabilities");
        }
        Ok(Self::Remote {
            operator,
            prefix: Arc::from("_ugoite/"),
        })
    }

    #[doc(hidden)]
    pub fn memory_for_tests() -> Self {
        Self::Memory(Arc::new(Mutex::new(std::collections::BTreeMap::new())))
    }

    fn local_path(root: &Path, key: &str) -> Result<PathBuf> {
        validate_key(key)?;
        Ok(root.join(key))
    }

    fn remote_path(prefix: &str, key: &str) -> Result<String> {
        validate_key(key)?;
        Ok(format!("{prefix}{key}"))
    }
}

#[async_trait]
impl NodeControlStore for OpenDalNodeControlStore {
    async fn get(&self, key: &str) -> Result<Option<ControlRecord>> {
        match self {
            Self::Local { root } => local_get(&Self::local_path(root, key)?),
            Self::Remote { operator, prefix } => {
                let path = Self::remote_path(prefix, key)?;
                let metadata = match operator.stat(&path).await {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                    Err(error) => return Err(error.into()),
                };
                let version = metadata
                    .etag()
                    .ok_or_else(|| {
                        anyhow!("control-store object has no ETag for compare-and-swap")
                    })?
                    .to_string();
                let value = operator
                    .read_with(&path)
                    .if_match(&version)
                    .await
                    .context("read versioned node control object")?
                    .to_vec();
                if value == DELETED_RECORD {
                    return Ok(None);
                }
                Ok(Some(ControlRecord { value, version }))
            }
            Self::Memory(records) => Ok(records
                .lock()
                .map_err(|_| anyhow!("memory control store lock poisoned"))?
                .get(key)
                .cloned()),
        }
    }

    async fn create_if_absent(&self, key: &str, value: Vec<u8>) -> Result<String> {
        match self {
            Self::Local { root } => local_create(&Self::local_path(root, key)?, &value),
            Self::Remote { operator, prefix } => {
                let path = Self::remote_path(prefix, key)?;
                operator
                    .write_with(&path, value)
                    .if_not_exists(true)
                    .await
                    .context("atomically create node control object")?;
                remote_version(operator, &path).await
            }
            Self::Memory(records) => {
                let mut records = records
                    .lock()
                    .map_err(|_| anyhow!("memory control store lock poisoned"))?;
                if records.contains_key(key) {
                    bail!("node control object already exists");
                }
                let version = digest_version(&value);
                records.insert(
                    key.to_string(),
                    ControlRecord {
                        value,
                        version: version.clone(),
                    },
                );
                Ok(version)
            }
        }
    }

    async fn compare_and_swap(
        &self,
        key: &str,
        expected_version: &str,
        value: Vec<u8>,
    ) -> Result<String> {
        match self {
            Self::Local { root } => {
                local_compare_and_swap(&Self::local_path(root, key)?, expected_version, &value)
            }
            Self::Remote { operator, prefix } => {
                let path = Self::remote_path(prefix, key)?;
                operator
                    .write_with(&path, value)
                    .if_match(expected_version)
                    .await
                    .context("compare-and-swap node control object")?;
                remote_version(operator, &path).await.map_err(|error| {
                    anyhow!("node control write committed with an ambiguous response: {error}")
                })
            }
            Self::Memory(records) => {
                let mut records = records
                    .lock()
                    .map_err(|_| anyhow!("memory control store lock poisoned"))?;
                let record = records
                    .get_mut(key)
                    .ok_or_else(|| anyhow!("node control object does not exist"))?;
                if record.version != expected_version {
                    bail!("node control object version conflict");
                }
                let version = digest_version(&value);
                *record = ControlRecord {
                    value,
                    version: version.clone(),
                };
                Ok(version)
            }
        }
    }

    async fn delete_if_version(&self, key: &str, expected_version: &str) -> Result<()> {
        match self {
            Self::Local { root } => {
                let path = Self::local_path(root, key)?;
                with_local_lock(&path, || {
                    let record = local_get(&path)?
                        .ok_or_else(|| anyhow!("node control object does not exist"))?;
                    if record.version != expected_version {
                        bail!("node control object version conflict");
                    }
                    fs::remove_file(&path)?;
                    Ok(())
                })
            }
            Self::Remote { operator, prefix } => {
                let path = Self::remote_path(prefix, key)?;
                operator
                    .write_with(&path, DELETED_RECORD.to_vec())
                    .if_match(expected_version)
                    .await
                    .context("atomically tombstone node control object")?;
                Ok(())
            }
            Self::Memory(records) => {
                let mut records = records
                    .lock()
                    .map_err(|_| anyhow!("memory control store lock poisoned"))?;
                let record = records
                    .get(key)
                    .ok_or_else(|| anyhow!("node control object does not exist"))?;
                if record.version != expected_version {
                    bail!("node control object version conflict");
                }
                records.remove(key);
                Ok(())
            }
        }
    }

    async fn list_prefix(&self, prefix_value: &str) -> Result<Vec<String>> {
        validate_key(prefix_value)?;
        match self {
            Self::Local { root } => local_list(root, prefix_value),
            Self::Remote { operator, prefix } => {
                let path = Self::remote_path(prefix, prefix_value)?;
                let mut keys = operator
                    .list_with(&path)
                    .recursive(true)
                    .await?
                    .into_iter()
                    .filter(|entry| entry.metadata().mode().is_file())
                    .map(|entry| entry.path().trim_start_matches(prefix.as_ref()).to_string())
                    .collect::<Vec<_>>();
                keys.sort();
                Ok(keys)
            }
            Self::Memory(records) => Ok(records
                .lock()
                .map_err(|_| anyhow!("memory control store lock poisoned"))?
                .keys()
                .filter(|key| key.starts_with(prefix_value))
                .cloned()
                .collect()),
        }
    }
}

fn validate_key(key: &str) -> Result<()> {
    if key.is_empty()
        || key.starts_with('/')
        || key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("invalid node control key");
    }
    Ok(())
}

fn digest_version(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn local_get(path: &Path) -> Result<Option<ControlRecord>> {
    match fs::read(path) {
        Ok(value) => Ok(Some(ControlRecord {
            version: digest_version(&value),
            value,
        })),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn local_create(path: &Path, value: &[u8]) -> Result<String> {
    with_local_lock(path, || {
        if path.exists() {
            bail!("node control object already exists");
        }
        atomic_write(path, value)?;
        Ok(digest_version(value))
    })
}

fn local_compare_and_swap(path: &Path, expected_version: &str, value: &[u8]) -> Result<String> {
    with_local_lock(path, || {
        let current =
            local_get(path)?.ok_or_else(|| anyhow!("node control object does not exist"))?;
        if current.version != expected_version {
            bail!("node control object version conflict");
        }
        atomic_write(path, value)?;
        Ok(digest_version(value))
    })
}

fn with_local_lock<T>(path: &Path, operation: impl FnOnce() -> Result<T>) -> Result<T> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        apply_dir_permissions(parent)?;
    }
    let lock_path = path.with_extension("lock");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    FileExt::lock_exclusive(&lock)?;
    let result = operation();
    FileExt::unlock(&lock)?;
    result
}

fn atomic_write(path: &Path, value: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("control path has no parent"))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("control"),
        uuid::Uuid::now_v7()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(value)?;
    file.sync_all()?;
    apply_file_permissions(&temporary)?;
    fs::rename(&temporary, path)?;
    OpenOptions::new().read(true).open(parent)?.sync_all()?;
    Ok(())
}

fn local_list(root: &Path, prefix: &str) -> Result<Vec<String>> {
    let start = root.join(prefix);
    if !start.exists() {
        return Ok(Vec::new());
    }
    let mut pending = vec![start];
    let mut keys = Vec::new();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else if path.extension().and_then(|extension| extension.to_str()) != Some("lock") {
            keys.push(
                path.strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    keys.sort();
    Ok(keys)
}

async fn remote_version(operator: &Operator, path: &str) -> Result<String> {
    let metadata = operator.stat(path).await?;
    metadata
        .etag()
        .or_else(|| metadata.version())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("control-store object has no ETag or generation"))
}

#[cfg(unix)]
fn apply_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn apply_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn apply_dir_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn apply_dir_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_store_enforces_compare_and_swap() -> Result<()> {
        let store = OpenDalNodeControlStore::memory_for_tests();
        let first = store
            .create_if_absent("nodes/a/session", b"one".to_vec())
            .await?;
        assert!(store
            .create_if_absent("nodes/a/session", Vec::new())
            .await
            .is_err());
        assert!(store
            .compare_and_swap("nodes/a/session", "stale", b"two".to_vec())
            .await
            .is_err());
        let second = store
            .compare_and_swap("nodes/a/session", &first, b"two".to_vec())
            .await?;
        assert_ne!(first, second);
        Ok(())
    }

    #[tokio::test]
    async fn local_store_uses_owner_only_atomic_files() -> Result<()> {
        let root = tempfile::tempdir()?;
        let store = OpenDalNodeControlStore::Local {
            root: Arc::new(root.path().join("_ugoite")),
        };
        let version = store.create_if_absent("node.json", b"one".to_vec()).await?;
        store
            .compare_and_swap("node.json", &version, b"two".to_vec())
            .await?;
        assert_eq!(store.get("node.json").await?.unwrap().value, b"two");
        Ok(())
    }
}
