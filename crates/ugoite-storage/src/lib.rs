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
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use tokio::io::AsyncWriteExt;
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

/// Minimal durable coordinate published by one rebuildable relation.  Iceberg
/// metadata owns the table details; this document only binds the visible
/// materialization to a single immutable build.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DerivedRelationHead {
    pub format_version: u32,
    pub space_id: String,
    pub relation_id: String,
    pub generation: u64,
    pub definition_version: u32,
    pub definition_fingerprint: String,
    pub producer_id: String,
    pub producer_fingerprint: String,
    pub compatibility_epoch: u64,
    pub materialization_id: String,
    pub table_identifier: serde_json::Value,
    pub table_uuid: String,
    pub metadata_location: String,
    pub snapshot_id: Option<i64>,
    pub schema_id: i32,
    pub base_generation: u64,
    pub target_generation: u64,
    pub build_id: String,
    pub input_digest: String,
    pub source_coordinate: serde_json::Value,
    pub materialization_manifest_location: String,
    pub materialization_manifest_checksum: String,
    pub last_command_id: String,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactDerivedRelationHead {
    pub head: DerivedRelationHead,
    pub bytes: Vec<u8>,
    pub etag: Option<String>,
}

/// Relation-local Head storage. It deliberately shares only OpenDAL object
/// mechanics with the authoritative Catalog store and has no Catalog history
/// or checkpoint behavior.
#[derive(Clone)]
pub struct DerivedRelationHeadStore {
    operator: Operator,
    space_root: String,
    relation_id: Uuid,
    write_mode: CatalogWriteMode,
    serializer: Arc<AsyncMutex<()>>,
}

impl std::fmt::Debug for DerivedRelationHeadStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DerivedRelationHeadStore")
            .field("space_root", &self.space_root)
            .field("relation_id", &self.relation_id)
            .field("write_mode", &self.write_mode)
            .finish()
    }
}

impl DerivedRelationHeadStore {
    pub fn new(operator: Operator, space_root: impl Into<String>, relation_id: Uuid) -> Self {
        let space_root = space_root.into().trim_matches('/').to_string();
        let serializer =
            catalog_serializer(&operator, &format!("{space_root}/derived/{relation_id}"));
        Self {
            operator,
            space_root,
            relation_id,
            write_mode: CatalogWriteMode::SingleProcess,
            serializer,
        }
    }

    pub async fn shared(mut self) -> Result<Self> {
        // Reuse the existing full OpenDAL contract probe.  Capability bits
        // alone are insufficient: a backend must prove changing ETags,
        // exact reads, create-if-absent, if-match replacement, and stale
        // rejection before shared Relation Head writes are admitted.
        SpaceCatalogStore::new(self.operator.clone(), self.space_root.clone())?
            .verify_shared_writes()
            .await?;
        self.write_mode = CatalogWriteMode::Shared;
        Ok(self)
    }

    pub fn single_process(mut self) -> Self {
        self.write_mode = CatalogWriteMode::SingleProcess;
        self
    }

    pub fn write_mode(&self) -> CatalogWriteMode {
        self.write_mode
    }

    pub fn head_path(&self) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/head.json",
            self.space_root, self.relation_id
        )
    }

    pub fn materializations_path(&self, materialization_id: &str) -> String {
        format!(
            "{}/_ugoite/derived/relations/{}/materializations/{materialization_id}",
            self.space_root, self.relation_id
        )
    }

    pub async fn read_exact(&self) -> Result<Option<ExactDerivedRelationHead>> {
        for attempt in 0..3 {
            let metadata = match self.operator.stat(&self.head_path()).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let etag = metadata
                .etag()
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            let read = match etag.as_deref() {
                Some(etag) => {
                    self.operator
                        .read_options(
                            &self.head_path(),
                            ReadOptions {
                                if_match: Some(etag.to_string()),
                                ..Default::default()
                            },
                        )
                        .await
                }
                None if self.write_mode == CatalogWriteMode::Shared => {
                    return Err(anyhow!(
                        "exact DerivedRelation Head stat did not return an ETag"
                    ))
                }
                None => self.operator.read(&self.head_path()).await,
            };
            match read {
                Ok(bytes) => {
                    let bytes = bytes.to_vec();
                    let head: DerivedRelationHead =
                        serde_json::from_slice(&bytes).context("decode DerivedRelation Head")?;
                    validate_derived_head_checksum(&head)?;
                    return Ok(Some(ExactDerivedRelationHead { head, bytes, etag }));
                }
                Err(error) if error.kind() == ErrorKind::ConditionNotMatch && attempt < 2 => {
                    continue
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("exact derived Head read attempts always return or continue")
    }

    pub async fn create(&self, head: &DerivedRelationHead) -> Result<()> {
        let bytes = canonical_head_bytes(head)?;
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
                let _guard = self.serializer.lock().await;
                if self.operator.exists(&self.head_path()).await? {
                    return Err(anyhow!("DerivedRelation Head already exists"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn replace(
        &self,
        expected_etag: Option<&str>,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        let bytes = canonical_head_bytes(head)?;
        match self.write_mode {
            CatalogWriteMode::Shared => {
                let etag =
                    expected_etag.context("shared DerivedRelation replacement requires an ETag")?;
                self.operator
                    .write_options(
                        &self.head_path(),
                        bytes,
                        WriteOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                let _guard = self.serializer.lock().await;
                let current = self
                    .read_exact()
                    .await?
                    .context("DerivedRelation Head disappeared")?;
                if expected_etag.is_some() && current.etag.as_deref() != expected_etag {
                    return Err(anyhow!("DerivedRelation Head changed"));
                }
                self.operator.write(&self.head_path(), bytes).await?;
            }
        }
        Ok(())
    }

    pub async fn publish(
        &self,
        expected: Option<&ExactDerivedRelationHead>,
        head: &DerivedRelationHead,
    ) -> Result<()> {
        match expected {
            None => self.create(head).await,
            Some(expected) => self.replace(expected.etag.as_deref(), head).await,
        }
    }
}

fn canonical_head_bytes(head: &DerivedRelationHead) -> Result<Vec<u8>> {
    let mut canonical = head.clone();
    canonical.checksum.clear();
    canonical.checksum = ugoite_domain::derived_relation::sha256_digest(
        &serde_json::to_vec(&canonical).context("canonicalize DerivedRelation Head")?,
    );
    Ok(serde_json::to_vec(&canonical).context("encode DerivedRelation Head")?)
}

fn validate_derived_head_checksum(head: &DerivedRelationHead) -> Result<()> {
    let mut canonical = head.clone();
    let observed = canonical.checksum.clone();
    canonical.checksum.clear();
    let expected = ugoite_domain::derived_relation::sha256_digest(
        &serde_json::to_vec(&canonical).context("canonicalize DerivedRelation Head")?,
    );
    if observed != expected {
        return Err(anyhow!("DerivedRelation Head checksum mismatch"));
    }
    Ok(())
}

/// Read-only declaration of the OpenDAL contract backing a Space Catalog.
/// It deliberately reports only capability bits and whether shared mode was
/// previously admitted; it never triggers a write probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogBackendCapabilities {
    pub etag: bool,
    pub read_with_if_match: bool,
    pub write_with_if_match: bool,
    pub write_with_if_not_exists: bool,
    pub shared_write_contract: bool,
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
    read_counter: Option<Arc<AtomicUsize>>,
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
            read_counter: None,
        })
    }

    /// Attaches a logical object-read counter for deterministic storage
    /// instrumentation in Catalog scalability tests. The counter is not part
    /// of the production coordination protocol.
    pub fn with_read_counter(mut self) -> (Self, Arc<AtomicUsize>) {
        let counter = Arc::new(AtomicUsize::new(0));
        self.read_counter = Some(counter.clone());
        (self, counter)
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

    pub fn command_receipt_path(&self, command_id: &str) -> String {
        self.catalog_path(&format!("command-receipts/{command_id}.json"))
    }

    /// Durable named checkpoints are immutable Space objects. They are not
    /// Catalog authority and never participate in Head publication.
    pub fn checkpoint_path(&self, name: &str) -> String {
        self.space_path(&format!("_ugoite/checkpoints/{name}.json"))
    }

    pub async fn read_exact_head(&self) -> Result<Option<ExactCatalogHead>> {
        let Some((bytes, etag)) = self.read_exact_object(&self.head_path()).await? else {
            return Ok(None);
        };
        Ok(Some(ExactCatalogHead { bytes, etag }))
    }

    async fn read_exact_object(&self, path: &str) -> Result<Option<(Vec<u8>, Option<String>)>> {
        if let Some(counter) = &self.read_counter {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        for attempt in 0..EXACT_HEAD_READ_ATTEMPTS {
            let metadata = match self.operator.stat(path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error.into()),
            };
            let etag = metadata
                .etag()
                .filter(|etag| !etag.is_empty())
                .map(str::to_owned);
            let read = match etag.as_deref() {
                Some(etag) => self
                    .operator
                    .read_options(
                        path,
                        ReadOptions {
                            if_match: Some(etag.to_string()),
                            ..Default::default()
                        },
                    )
                    .await
                    .map(|bytes| bytes.to_vec()),
                None if self.write_mode == CatalogWriteMode::Shared => {
                    return Err(anyhow!("exact Catalog object stat did not return an ETag"));
                }
                None => self.operator.read(path).await.map(|bytes| bytes.to_vec()),
            };
            match read {
                Ok(bytes) => return Ok(Some((bytes, etag))),
                Err(error)
                    if error.kind() == ErrorKind::ConditionNotMatch
                        && attempt + 1 < EXACT_HEAD_READ_ATTEMPTS =>
                {
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
        }
        unreachable!("exact Head read attempts always return or continue")
    }

    async fn replace_exact_object(
        &self,
        path: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        match self.write_mode {
            CatalogWriteMode::Shared => {
                self.operator
                    .write_options(
                        path,
                        bytes,
                        WriteOptions {
                            if_match: Some(
                                etag.context("shared exact object replacement requires an ETag")?
                                    .to_string(),
                            ),
                            ..Default::default()
                        },
                    )
                    .await?;
            }
            CatalogWriteMode::SingleProcess => {
                self.operator.write(path, bytes).await?;
            }
        }
        Ok(())
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

    /// Creates the deterministic lifecycle marker for one Asset.  The marker
    /// is deliberately outside Catalog Head: Head size must not grow with the
    /// number of deleted blobs.  `if_not_exists` also makes two deletion
    /// attempts contend at the storage boundary.
    pub async fn create_asset_lifecycle_marker(
        &self,
        asset_id: &str,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let path = self.catalog_path(&format!("asset-lifecycle/{asset_id}"));
        self.operator
            .write_options(
                &path,
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_asset_lifecycle_marker(
        &self,
        asset_id: &str,
    ) -> Result<Option<(Vec<u8>, Option<String>)>> {
        self.read_exact_object(&self.catalog_path(&format!("asset-lifecycle/{asset_id}")))
            .await
    }

    pub async fn replace_asset_lifecycle_marker(
        &self,
        asset_id: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.replace_exact_object(
            &self.catalog_path(&format!("asset-lifecycle/{asset_id}")),
            etag,
            bytes,
        )
        .await
    }

    pub async fn delete_asset_lifecycle_marker(&self, asset_id: &str) -> Result<()> {
        let path = self.catalog_path(&format!("asset-lifecycle/{asset_id}"));
        match self.operator.delete(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
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
        if let Some(counter) = &self.read_counter {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        Ok(self.operator.read(path).await?.to_vec())
    }

    pub async fn create_command_receipt(&self, command_id: &str, bytes: Vec<u8>) -> Result<()> {
        self.operator
            .write_options(
                &self.command_receipt_path(command_id),
                bytes,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn read_command_receipt(
        &self,
        command_id: &str,
    ) -> Result<Option<(Vec<u8>, Option<String>)>> {
        self.read_exact_object(&self.command_receipt_path(command_id))
            .await
    }

    pub async fn replace_command_receipt(
        &self,
        command_id: &str,
        etag: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<()> {
        self.replace_exact_object(&self.command_receipt_path(command_id), etag, bytes)
            .await
    }

    pub async fn create_checkpoint(&self, name: &str, bytes: Vec<u8>) -> Result<()> {
        if !self
            .operator
            .info()
            .full_capability()
            .write_with_if_not_exists
        {
            return Err(anyhow!(
                "immutable checkpoint creation requires OpenDAL if_not_exists support"
            ));
        }
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

    pub fn backend_capabilities(&self) -> CatalogBackendCapabilities {
        let capabilities = self.operator.info().full_capability();
        let shared_write_contract = self.supports_shared_writes();
        CatalogBackendCapabilities {
            // OpenDAL exposes ETags through stat metadata rather than a
            // separate capability bit. Exact shared Head reads additionally
            // require the conditional-read contract.
            etag: capabilities.stat && capabilities.read_with_if_match,
            read_with_if_match: capabilities.read_with_if_match,
            write_with_if_match: capabilities.write_with_if_match,
            write_with_if_not_exists: capabilities.write_with_if_not_exists,
            shared_write_contract,
        }
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
    async fn write_if_absent(&self, path: &str, data: Vec<u8>) -> Result<()>;
    async fn set_private(&self, _path: &str) -> Result<()> {
        Ok(())
    }
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
    let atomic_write_dir = Path::new(root).join(".ugoite-atomic-writes");
    let op = Operator::new(
        Fs::default()
            .root(root)
            .atomic_write_dir(atomic_write_dir.to_string_lossy().as_ref()),
    )?
    .finish();
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

    fn local_path(&self, path: &str) -> Option<std::path::PathBuf> {
        match self.operator.info().scheme() {
            "fs" | "file" => Some(Path::new(self.operator.info().root().as_str()).join(path)),
            _ => None,
        }
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

    async fn write_if_absent(&self, path: &str, data: Vec<u8>) -> Result<()> {
        if let Some(target) = self.local_path(path) {
            let parent = target
                .parent()
                .ok_or_else(|| anyhow!("storage path has no parent: {path}"))?;
            let file_name = target
                .file_name()
                .ok_or_else(|| anyhow!("storage path has no file name: {path}"))?
                .to_string_lossy();
            let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::now_v7()));
            let mut options = tokio::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).await?;
            file.write_all(&data).await?;
            file.sync_all().await?;

            // A direct create_new(target) makes a partially written target
            // visible to readers. Publish a fully written, synced temporary
            // file through a hard link instead: link(2) is atomic and fails
            // when another writer already owns the destination.
            drop(file);
            let link_result = tokio::fs::hard_link(&temporary, &target).await;
            let cleanup_result = tokio::fs::remove_file(&temporary).await;
            if let Err(error) = link_result {
                let _ = cleanup_result;
                return Err(error.into());
            }
            cleanup_result?;

            #[cfg(unix)]
            if let Ok(directory) = tokio::fs::File::open(parent).await {
                directory.sync_all().await?;
            }

            return Ok(());
        }

        if !self
            .operator
            .info()
            .full_capability()
            .write_with_if_not_exists
        {
            return Err(anyhow!(
                "storage backend does not support conditional object creation"
            ));
        }
        self.operator
            .write_options(
                path,
                data,
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    async fn set_private(&self, path: &str) -> Result<()> {
        let Some(target) = self.local_path(path) else {
            return Ok(());
        };

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let root_path = self.operator.info().root();
            let root = Path::new(root_path.as_str());
            let mut current = target.parent();
            while let Some(directory) = current {
                if directory == root {
                    break;
                }
                if tokio::fs::try_exists(directory).await? {
                    tokio::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))
                        .await?;
                }
                current = directory.parent();
            }

            if tokio::fs::try_exists(&target).await? {
                tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).await?;
            }
        }

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
        canonical_head_bytes, operator_from_uri, operator_from_uri_with_endpoint,
        DerivedRelationHead, DerivedRelationHeadStore, OpendalStorage, SpaceCatalogStore,
        StorageBackend,
    };
    use anyhow::Result;
    use futures::future::join_all;
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

    #[cfg(unix)]
    #[tokio::test]
    async fn local_conditional_create_is_private_and_single_winner() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir()?;
        let op = operator_from_uri(&format!("fs://{}", temp_dir.path().display()))?;
        let storage = OpendalStorage::from_operator(&op);
        storage.create_dir("response_hmac/").await?;

        let results = join_all((0..8).map(|index| {
            let storage = storage.clone();
            async move {
                storage
                    .write_if_absent(
                        "response_hmac/default.json",
                        format!("key-{index}").into_bytes(),
                    )
                    .await
            }
        }))
        .await;

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let published = storage.read("response_hmac/default.json").await?;
        assert!(published.starts_with(b"key-"));
        storage.set_private("response_hmac/default.json").await?;
        let file_mode = std::fs::metadata(temp_dir.path().join("response_hmac/default.json"))?
            .permissions()
            .mode()
            & 0o777;
        let dir_mode = std::fs::metadata(temp_dir.path().join("response_hmac"))?
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);

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

    #[tokio::test]
    async fn derived_head_uses_checksum_and_single_process_publication() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA001);
        let store = DerivedRelationHeadStore::new(operator.clone(), "spaces/demo", relation_id)
            .single_process();
        let first = DerivedRelationHead {
            format_version: 1,
            space_id: "demo".into(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer-fingerprint".into(),
            compatibility_epoch: 1,
            materialization_id: "materialization-a".into(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: "table-uuid".into(),
            metadata_location: "memory:///metadata.json".into(),
            snapshot_id: None,
            schema_id: 0,
            base_generation: 0,
            target_generation: 1,
            build_id: "build-a".into(),
            input_digest: "input-a".into(),
            source_coordinate: serde_json::json!({"catalog_head_sha256":null}),
            materialization_manifest_location: "manifest-a.json".into(),
            materialization_manifest_checksum: "manifest-checksum".into(),
            last_command_id: "command-a".into(),
            checksum: String::new(),
        };
        store.create(&first).await?;
        let exact_first = store.read_exact().await?.expect("derived Head");
        assert!(!exact_first.head.checksum.is_empty());

        let mut second = first.clone();
        second.generation = 2;
        second.base_generation = 1;
        second.target_generation = 2;
        second.materialization_id = "materialization-b".into();
        store.replace(None, &second).await?;
        let mut invalid: serde_json::Value =
            serde_json::from_slice(&canonical_head_bytes(&second)?)?;
        invalid["generation"] = serde_json::json!(99);
        operator
            .write(&store.head_path(), serde_json::to_vec(&invalid)?)
            .await?;
        let corrupt = store
            .read_exact()
            .await
            .expect_err("corrupt Relation Head checksum must be rejected");
        assert!(corrupt.to_string().contains("checksum"), "{corrupt}");
        Ok(())
    }

    #[tokio::test]
    async fn derived_head_shared_mode_fails_closed_without_contract() -> Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let operator = operator_from_uri(&format!("fs://{}", temp_dir.path().display()))?;
        let error =
            DerivedRelationHeadStore::new(operator, "spaces/demo", uuid::Uuid::from_u128(0xA001))
                .shared()
                .await
                .expect_err("filesystem backend has no exact shared-write contract");
        assert!(error
            .to_string()
            .contains("ETag-bound reads and conditional writes"));
        Ok(())
    }

    #[tokio::test]
    async fn derived_head_single_process_create_has_one_winner() -> Result<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let relation_id = uuid::Uuid::from_u128(0xA002);
        let store =
            DerivedRelationHeadStore::new(operator, "spaces/demo", relation_id).single_process();
        let head = DerivedRelationHead {
            format_version: 1,
            space_id: "demo".into(),
            relation_id: relation_id.to_string(),
            generation: 1,
            definition_version: 1,
            definition_fingerprint: "definition".into(),
            producer_id: "producer".into(),
            producer_fingerprint: "producer-fingerprint".into(),
            compatibility_epoch: 1,
            materialization_id: "materialization".into(),
            table_identifier: serde_json::json!({"table":"derived"}),
            table_uuid: "table-uuid".into(),
            metadata_location: "memory:///metadata.json".into(),
            snapshot_id: None,
            schema_id: 0,
            base_generation: 0,
            target_generation: 1,
            build_id: "build".into(),
            input_digest: "input".into(),
            source_coordinate: serde_json::json!({}),
            materialization_manifest_location: "manifest.json".into(),
            materialization_manifest_checksum: "manifest-checksum".into(),
            last_command_id: "command".into(),
            checksum: String::new(),
        };
        let results = join_all((0..8).map(|_| {
            let store = store.clone();
            let head = head.clone();
            async move { store.create(&head).await }
        }))
        .await;
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert!(store.read_exact().await?.is_some());
        Ok(())
    }
}
