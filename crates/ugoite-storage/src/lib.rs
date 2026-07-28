//! Persistence adapter boundary for Ugoite.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use futures::TryStreamExt;
use opendal::options::{ReadOptions, WriteOptions};
use opendal::services::{Fs, Memory, S3};
use opendal::{EntryMode, ErrorKind, Operator};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

pub use ugoite_domain as domain;

/// Normalized backend information used by the Iceberg OpenDAL adapter.
///
/// It deliberately contains no `Operator`: storage owns the live operator used
/// for Catalog Head operations, while Iceberg receives only the configuration
/// it needs to build its official OpenDAL storage factory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcebergStorageConfig {
    pub scheme: String,
    pub warehouse_uri: String,
    pub properties: HashMap<String, String>,
}

impl IcebergStorageConfig {
    pub fn from_operator(operator: &Operator) -> Result<Self> {
        let info = operator.info();
        let scheme = info.scheme().to_string();
        let operator_root = info.root();
        let root = operator_root.trim_end_matches('/');
        let warehouse_uri = match scheme.as_str() {
            "fs" | "file" => format!("file://{}", if root.is_empty() { "/" } else { root }),
            "memory" => "memory:///".to_string(),
            "s3" => format!("s3://{}/{}", info.name(), root.trim_start_matches('/')),
            "gcs" => format!("gs://{}/{}", info.name(), root.trim_start_matches('/')),
            "oss" => format!("oss://{}/{}", info.name(), root.trim_start_matches('/')),
            "azdls" => format!("abfs://{}/{}", info.name(), root.trim_start_matches('/')),
            unsupported => {
                return Err(anyhow!("unsupported Iceberg storage scheme: {unsupported}"))
            }
        };
        Ok(Self {
            scheme,
            warehouse_uri: warehouse_uri.trim_end_matches('/').to_string(),
            properties: HashMap::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactCatalogHead {
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogWriteMode {
    Shared,
    SingleProcess,
}

const EXACT_HEAD_READ_ATTEMPTS: usize = 3;

/// Narrow OpenDAL boundary for the Space Catalog root and immutable
/// publication evidence. It is intentionally not a general-purpose wrapper.
#[derive(Clone)]
pub struct SpaceCatalogStore {
    operator: Operator,
    space_root: String,
    storage: IcebergStorageConfig,
    write_mode: CatalogWriteMode,
    single_process_serializer: Arc<AsyncMutex<()>>,
}

impl std::fmt::Debug for SpaceCatalogStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpaceCatalogStore")
            .field("space_root", &self.space_root)
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

impl SpaceCatalogStore {
    pub fn new(operator: Operator, space_root: impl Into<String>) -> Result<Self> {
        let space_root = space_root.into().trim_matches('/').to_string();
        let single_process_serializer = catalog_serializer(&operator, &space_root);
        Ok(Self {
            storage: IcebergStorageConfig::from_operator(&operator)?,
            operator,
            space_root,
            // Shared mode is opt-in only after `verify_shared_writes` proves
            // the actual backend honors every conditional operation we need.
            write_mode: CatalogWriteMode::SingleProcess,
            single_process_serializer,
        })
    }

    pub fn single_process(mut self) -> Self {
        self.write_mode = CatalogWriteMode::SingleProcess;
        self
    }

    pub fn write_mode(&self) -> CatalogWriteMode {
        self.write_mode
    }

    /// A process-local serializer, used only in explicit single-process mode.
    /// It is not a cross-process lock and does not participate in shared CAS.
    pub fn single_process_serializer(&self) -> Arc<AsyncMutex<()>> {
        self.single_process_serializer.clone()
    }

    /// Proves the configured persistent backend supports the exact conditional
    /// sequence required by shared Catalog Head publication, then enables
    /// shared mode for this store value. The immutable probe is evidence only;
    /// it is never used for recovery or coordination.
    pub async fn verify_shared_writes(mut self) -> Result<Self> {
        if !self.supports_shared_writes() {
            return Err(anyhow!(
                "shared Catalog writes require ETag-bound reads and conditional writes"
            ));
        }
        let path = self.catalog_path(&format!("probes/{}.json", Uuid::now_v7()));
        let initial = b"{\"format_version\":1,\"stage\":\"created\"}".to_vec();
        self.operator
            .write_options(
                &path,
                initial.clone(),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        let duplicate_create = self
            .operator
            .write_options(
                &path,
                initial.clone(),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
            .expect_err("conditional create probe must reject an existing object");
        if duplicate_create.kind() != ErrorKind::ConditionNotMatch {
            return Err(duplicate_create.into());
        }
        let first_etag = self
            .operator
            .stat(&path)
            .await?
            .etag()
            .filter(|etag| !etag.is_empty())
            .map(str::to_owned)
            .context("shared Catalog probe write did not return an ETag")?;
        let observed = self
            .operator
            .read_options(
                &path,
                ReadOptions {
                    if_match: Some(first_etag.clone()),
                    ..Default::default()
                },
            )
            .await?
            .to_vec();
        if observed != initial {
            return Err(anyhow!(
                "shared Catalog probe read returned different bytes"
            ));
        }
        let replaced = b"{\"format_version\":1,\"stage\":\"replaced\"}".to_vec();
        self.operator
            .write_options(
                &path,
                replaced.clone(),
                WriteOptions {
                    if_match: Some(first_etag.clone()),
                    ..Default::default()
                },
            )
            .await?;
        let second_etag = self
            .operator
            .stat(&path)
            .await?
            .etag()
            .filter(|etag| !etag.is_empty())
            .map(str::to_owned)
            .context("shared Catalog probe replacement did not return an ETag")?;
        if second_etag == first_etag {
            return Err(anyhow!(
                "shared Catalog probe replacement did not change the ETag"
            ));
        }
        let stale_read = self
            .operator
            .read_options(
                &path,
                ReadOptions {
                    if_match: Some(first_etag.clone()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("conditional read probe must reject a stale ETag");
        if stale_read.kind() != ErrorKind::ConditionNotMatch {
            return Err(stale_read.into());
        }
        let stale_replace = self
            .operator
            .write_options(
                &path,
                b"{\"format_version\":1,\"stage\":\"stale\"}".to_vec(),
                WriteOptions {
                    if_match: Some(first_etag),
                    ..Default::default()
                },
            )
            .await
            .expect_err("conditional replacement probe must reject a stale ETag");
        if stale_replace.kind() != ErrorKind::ConditionNotMatch {
            return Err(stale_replace.into());
        }
        let observed = self
            .operator
            .read_options(
                &path,
                ReadOptions {
                    if_match: Some(second_etag),
                    ..Default::default()
                },
            )
            .await?
            .to_vec();
        if observed != replaced {
            return Err(anyhow!(
                "shared Catalog probe replacement returned different bytes"
            ));
        }
        self.write_mode = CatalogWriteMode::Shared;
        Ok(self)
    }

    pub fn iceberg_storage(&self) -> &IcebergStorageConfig {
        &self.storage
    }

    /// The authoritative operator is shared only with the physical Iceberg
    /// adapter so its test-only memory service sees the same immutable table
    /// metadata as Catalog Head operations. Core never receives this handle.
    pub fn iceberg_operator(&self) -> Operator {
        self.operator.clone()
    }

    pub fn warehouse_uri(&self) -> String {
        if self.space_root.is_empty() {
            format!("{}/forms", self.storage.warehouse_uri)
        } else {
            format!("{}/{}/forms", self.storage.warehouse_uri, self.space_root)
        }
    }

    pub fn head_path(&self) -> String {
        self.catalog_path("head.json")
    }

    pub fn publication_path(&self, generation: u64, command_id: &str) -> String {
        self.catalog_path(&format!("publications/{generation}-{command_id}.json"))
    }

    /// Durable named checkpoints are immutable Space objects. They are not
    /// Catalog authority and never participate in Head publication.
    pub fn checkpoint_path(&self, name: &str) -> String {
        self.space_path(&format!("_ugoite/checkpoints/{name}.json"))
    }

    pub async fn read_exact_head(&self) -> Result<Option<ExactCatalogHead>> {
        let path = self.head_path();
        for attempt in 0..EXACT_HEAD_READ_ATTEMPTS {
            let metadata = match self.operator.stat(&path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let etag = metadata
                .etag()
                .filter(|etag| !etag.is_empty())
                .map(str::to_owned);
            let read = match (self.write_mode, etag.as_deref()) {
                (CatalogWriteMode::Shared, Some(etag)) => self
                    .operator
                    .read_options(
                        &path,
                        ReadOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await
                    .map(|bytes| bytes.to_vec()),
                (CatalogWriteMode::Shared, None) => {
                    return Err(anyhow!("Catalog Head stat did not return an ETag"));
                }
                (CatalogWriteMode::SingleProcess, _) => {
                    self.operator.read(&path).await.map(|bytes| bytes.to_vec())
                }
            };
            match read {
                Ok(bytes) => return Ok(Some(ExactCatalogHead { bytes, etag })),
                Err(error)
                    if self.write_mode == CatalogWriteMode::Shared
                        && error.kind() == ErrorKind::ConditionNotMatch
                        && attempt + 1 < EXACT_HEAD_READ_ATTEMPTS =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("exact Head read attempts always return or continue")
    }

    pub async fn create_head(&self, bytes: Vec<u8>) -> Result<()> {
        match self.write_mode {
            CatalogWriteMode::Shared => {
                self.operator
                    .write_options(
                        &self.head_path(),
                        bytes,
                        WriteOptions {
                            if_not_exists: true,
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                if self.operator.exists(&self.head_path()).await? {
                    return Err(anyhow!("Catalog Head already exists"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn replace_head(&self, etag: Option<&str>, bytes: Vec<u8>) -> Result<()> {
        match self.write_mode {
            CatalogWriteMode::Shared => {
                self.operator
                    .write_options(
                        &self.head_path(),
                        bytes,
                        WriteOptions {
                            if_match: Some(
                                etag.context("shared Catalog Head replacement requires an ETag")?
                                    .to_string(),
                            ),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn create_publication(&self, path: &str, bytes: Vec<u8>) -> Result<()> {
        self.operator
            .write_options(
                path,
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_publication(&self, path: &str) -> opendal::Result<Vec<u8>> {
        Ok(self.operator.read(path).await?.to_vec())
    }

    pub async fn create_checkpoint(&self, name: &str, bytes: Vec<u8>) -> Result<()> {
        self.operator
            .write_options(
                &self.checkpoint_path(name),
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_checkpoint(&self, name: &str) -> opendal::Result<Vec<u8>> {
        Ok(self
            .operator
            .read(&self.checkpoint_path(name))
            .await?
            .to_vec())
    }

    pub fn supports_shared_writes(&self) -> bool {
        let capabilities = self.operator.info().full_capability();
        capabilities.read_with_if_match
            && capabilities.write_with_if_match
            && capabilities.write_with_if_not_exists
    }

    fn catalog_path(&self, suffix: &str) -> String {
        self.space_path(&format!("_ugoite/catalog/{suffix}"))
    }

    fn space_path(&self, suffix: &str) -> String {
        if self.space_root.is_empty() {
            suffix.to_string()
        } else {
            format!("{}/{suffix}", self.space_root)
        }
    }
}

static CATALOG_SERIALIZERS: OnceLock<Mutex<HashMap<String, Weak<AsyncMutex<()>>>>> =
    OnceLock::new();

fn catalog_serializer(operator: &Operator, space_root: &str) -> Arc<AsyncMutex<()>> {
    let key = format!(
        "{}:{}:{}",
        operator.info().scheme(),
        operator.info().root(),
        space_root
    );
    let serializers = CATALOG_SERIALIZERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut serializers = serializers
        .lock()
        .expect("catalog serializer registry poisoned");
    if let Some(serializer) = serializers.get(&key).and_then(Weak::upgrade) {
        return serializer;
    }
    let serializer = Arc::new(AsyncMutex::new(()));
    serializers.insert(key, Arc::downgrade(&serializer));
    serializer
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageEntry {
    pub name: String,
    pub is_dir: bool,
}

#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn exists(&self, path: &str) -> Result<bool>;
    async fn read(&self, path: &str) -> Result<Vec<u8>>;
    async fn write(&self, path: &str, data: Vec<u8>) -> Result<()>;
    async fn create_dir(&self, path: &str) -> Result<()>;
    async fn list_dir(&self, path: &str) -> Result<Vec<StorageEntry>>;

    async fn read_json<T>(&self, path: &str) -> Result<T>
    where
        T: DeserializeOwned + Send,
    {
        Ok(serde_json::from_slice(&self.read(path).await?)?)
    }

    async fn write_json<T>(&self, path: &str, value: &T) -> Result<()>
    where
        T: Serialize + Sync,
    {
        self.write(path, serde_json::to_vec_pretty(value)?).await
    }
}

static MEMORY_OPERATORS: OnceLock<Mutex<HashMap<String, Operator>>> = OnceLock::new();

fn memory_cache() -> &'static Mutex<HashMap<String, Operator>> {
    MEMORY_OPERATORS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn local_operator_from_uri(uri: &str) -> Result<Operator> {
    let root = uri
        .strip_prefix("fs://")
        .or_else(|| uri.strip_prefix("file://"))
        .unwrap_or(uri);
    let op = Operator::new(Fs::default().root(root))?.finish();
    Ok(op)
}

pub fn operator_from_uri(uri: &str) -> Result<Operator> {
    operator_from_uri_with_endpoint(uri, None)
}

pub fn operator_from_uri_with_endpoint(uri: &str, endpoint: Option<&str>) -> Result<Operator> {
    if uri.starts_with("memory://") {
        let mut cache = memory_cache()
            .lock()
            .map_err(|_| anyhow::anyhow!("memory operator cache lock poisoned"))?;
        if let Some(op) = cache.get(uri) {
            return Ok(op.clone());
        }
        let op = Operator::new(Memory::default())?.finish();
        cache.insert(uri.to_string(), op.clone());
        return Ok(op);
    }

    if uri.starts_with("fs://")
        || uri.starts_with("file://")
        || uri.starts_with('/')
        || uri.starts_with('.')
    {
        return local_operator_from_uri(uri);
    }

    if uri.starts_with("s3://") {
        let parsed = url::Url::parse(uri)?;
        let bucket = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("s3 storage URI must include a bucket"))?;
        let root = parsed.path().trim_start_matches('/');
        let mut builder = S3::default().bucket(bucket).root(root).region("us-east-1");
        if let Some(endpoint) = endpoint {
            builder = builder.endpoint(endpoint);
        }
        return Ok(Operator::new(builder)?.finish());
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

#[cfg(test)]
mod tests {
    use super::{
        operator_from_uri, operator_from_uri_with_endpoint, OpendalStorage, SpaceCatalogStore,
        StorageBackend,
    };
    use anyhow::Result;
    use opendal::services::Memory;
    use opendal::Operator;

    #[tokio::test]
    async fn operator_from_uri_supports_fs_and_memory() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let fs_uri = format!("fs://{}", temp_dir.path().display());
        let fs_operator = operator_from_uri(&fs_uri)?;
        fs_operator
            .write("hello.txt", b"hello world".to_vec())
            .await?;
        let fs_bytes = fs_operator.read("hello.txt").await?.to_vec();
        assert_eq!(fs_bytes, b"hello world");

        let memory_operator = operator_from_uri("memory://storage-crate")?;
        memory_operator
            .write("hello.txt", b"hello world".to_vec())
            .await?;
        let memory_bytes = memory_operator.read("hello.txt").await?.to_vec();
        assert_eq!(memory_bytes, b"hello world");

        Ok(())
    }

    #[tokio::test]
    /// REQ-STO-001: JSON helpers remain part of the storage boundary after
    /// I/O is moved out of the portable domain crate.
    async fn test_storage_req_sto_001_json_helpers_use_storage_abstraction() -> Result<()> {
        let op = operator_from_uri("memory://storage-contract")?;
        let storage = OpendalStorage::from_operator(&op);
        storage.create_dir("spaces/demo/").await?;
        storage
            .write("spaces/demo/readme.md", b"hello".to_vec())
            .await?;

        assert!(storage.exists("spaces/demo/readme.md").await?);
        assert!(storage.exists("spaces/demo/").await?);
        assert!(!storage.exists("spaces/missing/").await?);
        assert_eq!(storage.read("spaces/demo/readme.md").await?, b"hello");
        let entries = storage.list_dir("spaces/demo/").await?;
        assert!(entries
            .iter()
            .any(|entry| entry.name.ends_with("readme.md") && !entry.is_dir));

        storage
            .write_json("spaces/demo/meta.json", &serde_json::json!({"id": "demo"}))
            .await?;
        let metadata: serde_json::Value = storage.read_json("spaces/demo/meta.json").await?;
        assert_eq!(metadata["id"], "demo");
        assert_eq!(
            storage.list_dir("spaces/").await?,
            vec![super::StorageEntry {
                name: "demo/".to_string(),
                is_dir: true,
            }]
        );

        Ok(())
    }

    #[test]
    fn operator_from_uri_with_endpoint_builds_s3_with_custom_endpoint() -> Result<()> {
        let op = operator_from_uri_with_endpoint(
            "s3://bucket-name/prefix",
            Some("https://s3.example.test"),
        )?;

        assert_eq!(op.info().scheme(), "s3");
        assert_eq!(op.info().root(), "/prefix/");

        Ok(())
    }

    #[tokio::test]
    async fn catalog_head_reads_are_exact_in_single_process_mode() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let store = SpaceCatalogStore::new(operator, "spaces/demo")?;

        assert!(store.read_exact_head().await?.is_none());
        store.create_head(b"first".to_vec()).await?;
        let first = store.read_exact_head().await?.expect("Catalog Head exists");
        assert_eq!(first.bytes, b"first");

        store.replace_head(None, b"second".to_vec()).await?;
        let second = store.read_exact_head().await?.expect("Catalog Head exists");
        assert_eq!(second.bytes, b"second");

        Ok(())
    }

    #[tokio::test]
    async fn shared_catalog_mode_fails_closed_without_an_exact_etag_contract() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let error = SpaceCatalogStore::new(operator, "spaces/demo")?
            .verify_shared_writes()
            .await
            .expect_err("Memory has no ETag-bound shared-write contract");

        assert!(error
            .to_string()
            .contains("ETag-bound reads and conditional writes"));
        Ok(())
    }
}
