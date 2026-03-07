use anyhow::Result;
use async_trait::async_trait;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
pub use ugoite_minimum::storage::{StorageBackend, StorageEntry};

static MEMORY_OPERATORS: OnceLock<Mutex<HashMap<String, Operator>>> = OnceLock::new();

fn memory_cache() -> &'static Mutex<HashMap<String, Operator>> {
    MEMORY_OPERATORS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn operator_from_uri(uri: &str) -> Result<Operator> {
    if uri.starts_with("memory://") {
        let mut cache = memory_cache()
            .lock()
            .map_err(|_| anyhow::anyhow!("memory operator cache lock poisoned"))?;
        if let Some(op) = cache.get(uri) {
            return Ok(op.clone());
        }
        let op = Operator::from_uri(uri)?;
        cache.insert(uri.to_string(), op.clone());
        return Ok(op);
    }

    Ok(Operator::from_uri(uri)?)
}

#[derive(Clone)]
pub struct OpendalStorage {
    operator: Operator,
}

impl OpendalStorage {
    pub fn new(operator: Operator) -> Self {
        Self { operator }
    }

    pub fn from_operator(operator: &Operator) -> Self {
        Self::new(operator.clone())
    }
}

#[async_trait]
impl StorageBackend for OpendalStorage {
    async fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.operator.exists(path).await?)
    }

    async fn read(&self, path: &str) -> Result<Vec<u8>> {
        Ok(self.operator.read(path).await?.to_vec())
    }

    async fn write(&self, path: &str, data: Vec<u8>) -> Result<()> {
        self.operator.write(path, data).await?;
        Ok(())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        self.operator.create_dir(path).await?;
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<StorageEntry>> {
        let mut entries = Vec::new();
        let mut lister = self.operator.lister(path).await?;
        while let Some(entry) = lister.try_next().await? {
            entries.push(StorageEntry {
                name: entry.name().to_string(),
                is_dir: entry.metadata().mode() == EntryMode::DIR,
            });
        }
        Ok(entries)
    }
}
