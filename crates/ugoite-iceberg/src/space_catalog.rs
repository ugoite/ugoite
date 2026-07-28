#[cfg(test)]
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use iceberg::io::{FileIO, FileIOBuilder, Storage, StorageConfig, StorageFactory};
use iceberg::spec::{TableMetadata, TableMetadataBuilder};
use iceberg::{
    Catalog, Error, ErrorKind, MetadataLocation, Namespace, NamespaceIdent, Result, Runtime,
    TableCommit, TableCreation, TableIdent,
};
use iceberg_storage_opendal::{OpenDalResolvingStorageFactory, OpenDalStorage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ugoite_domain::id::SpaceId;
use ugoite_storage::{CatalogWriteMode, ExactCatalogHead, SpaceCatalogStore};
use uuid::Uuid;

const SPACE_FORMAT_VERSION: u32 = 1;
const MAX_HEAD_BYTES: usize = 1 << 20;

/// Holds the official OpenDAL Iceberg storage instance for test-only memory
/// spaces. It adds no I/O behavior; it merely keeps Iceberg metadata and
/// Catalog Head on the same OpenDAL operator.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FixedOpenDalStorageFactory {
    #[serde(skip, default = "default_memory_storage")]
    storage: Arc<OpenDalStorage>,
}

fn default_memory_storage() -> Arc<OpenDalStorage> {
    let operator = opendal::Operator::new(opendal::services::Memory::default())
        .expect("memory operator configuration")
        .finish();
    Arc::new(OpenDalStorage::Memory(operator))
}

#[typetag::serde(name = "UgoiteFixedOpenDalStorageFactory")]
impl StorageFactory for FixedOpenDalStorageFactory {
    fn build(&self, _config: &StorageConfig) -> Result<Arc<dyn Storage>> {
        Ok(self.storage.clone())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PublicationContext {
    pub command_id: String,
    pub command_kind: String,
    pub command_digest: String,
}

/// Durable outcome of a single command publication.  This deliberately
/// contains only domain-facing coordinates; Iceberg metadata locations stay
/// inside the catalog implementation.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PublicationReceipt {
    pub command_id: String,
    pub catalog_generation: u64,
    pub snapshot_id: Option<i64>,
}

impl PublicationContext {
    pub fn new(command_id: impl Into<String>, command_kind: impl Into<String>) -> Self {
        let command_id = command_id.into();
        let command_kind = command_kind.into();
        let command_digest = checksum(format!("{command_id}:{command_kind}").as_bytes());
        Self {
            command_id,
            command_kind,
            command_digest,
        }
    }

    /// Uses the digest of the domain command coordinated by the caller. A
    /// retry must reuse all three values; otherwise it is a different attempt.
    pub fn with_command_digest(
        command_id: impl Into<String>,
        command_kind: impl Into<String>,
        command_digest: impl Into<String>,
    ) -> Self {
        Self {
            command_id: command_id.into(),
            command_kind: command_kind.into(),
            command_digest: command_digest.into(),
        }
    }

    fn generated() -> Self {
        Self::new(Uuid::new_v4().to_string(), "iceberg-catalog")
    }
}

/// Immutable evidence captured before an Iceberg mutation writes any durable
/// object. The upstream `Catalog` trait supplies a `TableCommit`, but not this
/// Ugoite-specific publication context, so it stays scoped to one Catalog
/// method invocation rather than being kept in ambient mutable state.
#[derive(Debug, Clone)]
struct PublicationAttempt {
    publication: PublicationContext,
    expected_head: Option<CatalogHead>,
    expected_head_etag: Option<String>,
    expected_generation: Option<u64>,
    expected_head_checksum: Option<String>,
    expected_previous_publication: Option<String>,
}

impl PublicationAttempt {
    fn from_exact(
        publication: &PublicationContext,
        exact: Option<(CatalogHead, ExactCatalogHead)>,
    ) -> Self {
        match exact {
            Some((head, exact)) => Self {
                publication: publication.clone(),
                expected_generation: Some(head.generation),
                expected_head_checksum: Some(head.checksum.clone()),
                expected_previous_publication: head.publication_location.clone(),
                expected_head: Some(head),
                expected_head_etag: exact.etag,
            },
            None => Self {
                publication: publication.clone(),
                expected_head: None,
                expected_head_etag: None,
                expected_generation: None,
                expected_head_checksum: None,
                expected_previous_publication: None,
            },
        }
    }
}

#[derive(Debug)]
struct PublicationUpdate {
    affected_table: TableCoordinates,
    base_metadata_location: Option<String>,
    new_metadata_location: String,
    base_snapshot_id: Option<i64>,
    base_schema_id: Option<i32>,
    new_snapshot_id: Option<i64>,
    new_schema_id: i32,
}

#[derive(Clone)]
pub struct SpaceCatalog {
    store: SpaceCatalogStore,
    namespace: NamespaceIdent,
    space_id: SpaceId,
    file_io: FileIO,
    runtime: Runtime,
    publication: PublicationContext,
    mutation_claimed: Arc<AtomicBool>,
}

impl std::fmt::Debug for SpaceCatalog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpaceCatalog")
            .field("namespace", &self.namespace)
            .field("space_id", &self.space_id)
            .field("publication", &self.publication)
            .finish_non_exhaustive()
    }
}

impl SpaceCatalog {
    pub fn new(store: SpaceCatalogStore, space_id: SpaceId) -> Result<Self> {
        let storage = store.iceberg_storage();
        if store.write_mode() == CatalogWriteMode::Shared && !store.supports_shared_writes() {
            return Err(Error::new(
                ErrorKind::FeatureUnsupported,
                "shared SpaceCatalog writes require ETag-bound reads and conditional writes",
            ));
        }
        let file_io = if storage.scheme == "memory" {
            FileIOBuilder::new(Arc::new(FixedOpenDalStorageFactory {
                storage: Arc::new(OpenDalStorage::Memory(store.iceberg_operator())),
            }))
            .build()
        } else {
            FileIOBuilder::new(Arc::new(OpenDalResolvingStorageFactory::new()))
                .with_props(storage.properties.clone())
                .build()
        };
        Ok(Self {
            store,
            namespace: NamespaceIdent::new(format!("space_{}", space_id.as_uuid().simple())),
            space_id,
            file_io,
            runtime: Runtime::current(),
            publication: PublicationContext::generated(),
            mutation_claimed: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(crate) fn with_publication_context(mut self, publication: PublicationContext) -> Self {
        self.publication = publication;
        self
    }

    /// Creates a fresh single-command Catalog publication attempt over the
    /// same Space. Reads may share a catalog instance, but a write must never
    /// reuse its immutable publication record or command identifier.
    pub(crate) fn new_attempt(&self) -> Self {
        Self {
            store: self.store.clone(),
            namespace: self.namespace.clone(),
            space_id: self.space_id,
            file_io: self.file_io.clone(),
            runtime: self.runtime.clone(),
            publication: PublicationContext::generated(),
            mutation_claimed: Arc::new(AtomicBool::new(false)),
        }
    }

    #[cfg(test)]
    fn namespace(&self) -> &NamespaceIdent {
        &self.namespace
    }

    async fn exact_head(&self) -> Result<Option<(CatalogHead, ExactCatalogHead)>> {
        let Some(exact) = self.store.read_exact_head().await.map_err(storage_error)? else {
            return Ok(None);
        };
        let head = decode_head(&exact.bytes)?;
        if head.space_id != self.space_id.to_string() || head.namespace != *self.namespace.as_ref()
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head belongs to a different Space or namespace",
            ));
        }
        self.validate_head_publication(&head).await?;
        Ok(Some((head, exact)))
    }

    /// Finds a completed command through the immutable publication chain.
    /// Reusing an id with a different kind or digest is an invalid idempotency
    /// key reuse, rather than a request to replay a different mutation.
    pub(crate) async fn publication_receipt(
        &self,
        publication: &PublicationContext,
    ) -> Result<Option<PublicationReceipt>> {
        let Some((mut head, _)) = self.exact_head().await? else {
            return Ok(None);
        };
        let mut path = head.publication_location.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication record",
            )
        })?;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(path.clone()) {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog publication chain contains a cycle",
                ));
            }
            let record = decode_publication(
                &self
                    .store
                    .read_publication(&path)
                    .await
                    .map_err(storage_error)?,
            )?;
            validate_publication_matches_head(&record, &head)?;
            if record.command_id == publication.command_id {
                if record.command_kind != publication.command_kind
                    || record.command_digest != publication.command_digest
                {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "publication command id was reused with different command content",
                    ));
                }
                return Ok(Some(PublicationReceipt {
                    command_id: record.command_id,
                    catalog_generation: record.generation,
                    snapshot_id: record.new_snapshot_id,
                }));
            }
            let (previous_generation, previous_path, previous_checksum) = match (
                record.previous_generation,
                record.previous_publication,
                record.previous_head_checksum,
            ) {
                (None, None, None) if record.generation == 0 => return Ok(None),
                (Some(generation), Some(path), Some(checksum))
                    if generation + 1 == record.generation =>
                {
                    (generation, path, checksum)
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog publication chain is incomplete or corrupt",
                    ));
                }
            };
            let previous = decode_publication(
                &self
                    .store
                    .read_publication(&previous_path)
                    .await
                    .map_err(storage_error)?,
            )?;
            if previous.generation != previous_generation
                || previous.next_head_checksum != previous_checksum
            {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog publication predecessor is corrupt",
                ));
            }
            head = previous.next_head.clone();
            path = previous_path;
        }
    }

    fn table_key(table: &TableIdent) -> String {
        format!(
            "{}\u{001f}{}",
            table.namespace().to_url_string(),
            table.name()
        )
    }

    fn claim_mutation(&self) -> Result<()> {
        if self
            .mutation_claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "a SpaceCatalog instance represents one publication attempt; open a new attempt",
            ));
        }
        Ok(())
    }

    async fn load_head_table(
        &self,
        table: &TableIdent,
        head: &CatalogHead,
    ) -> Result<iceberg::table::Table> {
        let reference = head.tables.get(&Self::table_key(table)).ok_or_else(|| {
            Error::new(ErrorKind::DataInvalid, format!("table not found: {table}"))
        })?;
        let metadata =
            TableMetadata::read_from(&self.file_io, &reference.metadata_location).await?;
        iceberg::table::Table::builder()
            .identifier(table.clone())
            .metadata(metadata)
            .metadata_location(reference.metadata_location.clone())
            .file_io(self.file_io.clone())
            .runtime(self.runtime.clone())
            .build()
    }

    /// Publishes metadata built with Iceberg's standard metadata builder while
    /// preserving caller-assigned stable field IDs. The public Iceberg Rust
    /// transaction builder currently allocates new IDs for added columns, so
    /// this Catalog operation is the narrow place where a Form's already
    /// stable IDs enter the immutable Catalog publication protocol.
    pub(crate) async fn replace_table_metadata(
        &self,
        table: &TableIdent,
        metadata: TableMetadata,
    ) -> Result<iceberg::table::Table> {
        self.claim_mutation()?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let attempt = PublicationAttempt::from_exact(&self.publication, self.exact_head().await?);
        let head = attempt
            .expected_head
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        let base = self.load_head_table(table, head).await?;
        if metadata.uuid() != base.metadata().uuid() {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "table metadata UUID does not match the Catalog Head table",
            ));
        }
        let base_metadata_location = base.metadata_location_result()?.to_string();
        let metadata_location = MetadataLocation::from_str(&base_metadata_location)?
            .with_next_version()
            .with_new_metadata(&metadata)
            .to_string();
        metadata
            .write_to(
                &self.file_io,
                &MetadataLocation::from_str(&metadata_location)?,
            )
            .await?;
        let mut next = head.next_generation();
        next.tables.insert(
            Self::table_key(table),
            TableReference {
                identifier: TableCoordinates::from(table),
                form_id: metadata.properties().get("ugoite.form.id").cloned(),
                table_uuid: metadata.uuid().to_string(),
                metadata_location: metadata_location.clone(),
            },
        );
        let publication = self
            .publish_new_head(
                &attempt,
                next,
                PublicationUpdate {
                    affected_table: TableCoordinates::from(table),
                    base_metadata_location: Some(base_metadata_location),
                    new_metadata_location: metadata_location,
                    base_snapshot_id: base.metadata().current_snapshot_id(),
                    base_schema_id: Some(base.metadata().current_schema_id()),
                    new_snapshot_id: metadata.current_snapshot_id(),
                    new_schema_id: metadata.current_schema_id(),
                },
            )
            .await;
        match publication {
            Ok(()) => self.load_table(table).await,
            Err(_error) if self.resolve_unknown_outcome(&attempt).await? => {
                self.load_table(table).await
            }
            Err(error) => Err(error),
        }
    }

    async fn write_publication(&self, publication: &PublicationRecord) -> Result<String> {
        let path = self
            .store
            .publication_path(publication.generation, &publication.command_id);
        let bytes = encode_publication(publication)?;
        match self.store.create_publication(&path, bytes.clone()).await {
            Ok(()) => Ok(path),
            Err(error) if is_condition_conflict(&error) => {
                let existing = self
                    .store
                    .read_publication(&path)
                    .await
                    .map_err(storage_error)?;
                if existing == bytes {
                    Ok(path)
                } else {
                    Err(Error::new(
                        ErrorKind::Unexpected,
                        "publication path is already owned by another command",
                    ))
                }
            }
            Err(error) => Err(storage_error(error)),
        }
    }

    async fn publish_new_head(
        &self,
        attempt: &PublicationAttempt,
        mut next: CatalogHead,
        update: PublicationUpdate,
    ) -> Result<()> {
        let previous_generation = attempt.expected_generation;
        let previous_publication = attempt.expected_previous_publication.clone();
        let previous_head_checksum = attempt.expected_head_checksum.clone();
        let publication_path = self
            .store
            .publication_path(next.generation, &attempt.publication.command_id);
        next.publication_location = Some(publication_path);
        next.publication_command_id = Some(attempt.publication.command_id.clone());
        next.checksum = head_checksum(&next)?;
        let publication = PublicationRecord {
            generation: next.generation,
            previous_generation,
            previous_publication,
            previous_head_checksum,
            command_id: attempt.publication.command_id.clone(),
            command_kind: attempt.publication.command_kind.clone(),
            command_digest: attempt.publication.command_digest.clone(),
            affected_table: update.affected_table,
            base_metadata_location: update.base_metadata_location,
            new_metadata_location: update.new_metadata_location,
            base_snapshot_id: update.base_snapshot_id,
            base_schema_id: update.base_schema_id,
            new_snapshot_id: update.new_snapshot_id,
            new_schema_id: update.new_schema_id,
            next_head_checksum: next.checksum.clone(),
            next_head: next.clone(),
            checksum: String::new(),
        };
        let mut publication = publication;
        publication.checksum = publication_checksum(&publication)?;
        self.write_publication(&publication).await?;
        let bytes = encode_head(&next)?;
        if bytes.len() > MAX_HEAD_BYTES {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head exceeds its 1 MiB safety limit",
            ));
        }
        let result = if attempt.expected_head.is_some() {
            self.store
                .replace_head(attempt.expected_head_etag.as_deref(), bytes)
                .await
        } else {
            self.store.create_head(bytes).await
        };
        match result {
            Ok(()) => Ok(()),
            Err(error) if is_condition_conflict(&error) => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed before this publication could be committed",
            )),
            Err(error) => Err(storage_error(error)),
        }
    }

    async fn resolve_unknown_outcome(&self, attempt: &PublicationAttempt) -> Result<bool> {
        let Some((mut head, _)) = self.exact_head().await? else {
            return if attempt.expected_head.is_none() {
                Ok(false)
            } else {
                Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog Head disappeared while resolving an unknown publication outcome",
                ))
            };
        };
        let mut path = head.publication_location.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication record while resolving an unknown outcome",
            )
        })?;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(path.clone()) {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog publication chain contains a cycle",
                ));
            }
            let publication = decode_publication(
                &self
                    .store
                    .read_publication(&path)
                    .await
                    .map_err(storage_error)?,
            )?;
            validate_publication_matches_head(&publication, &head)?;
            if publication.command_id == attempt.publication.command_id {
                if publication.command_kind == attempt.publication.command_kind
                    && publication.command_digest == attempt.publication.command_digest
                {
                    if publication.previous_generation != attempt.expected_generation
                        || publication.previous_publication.as_deref()
                            != attempt.expected_previous_publication.as_deref()
                        || publication.previous_head_checksum.as_deref()
                            != attempt.expected_head_checksum.as_deref()
                    {
                        return Err(Error::new(
                            ErrorKind::DataInvalid,
                            "matching publication does not link to this attempt's exact base Head",
                        ));
                    }
                    return Ok(true);
                }
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "publication command id was reused with different command content",
                ));
            }
            if Some(publication.generation) == attempt.expected_generation {
                if Some(publication.next_head_checksum.as_str())
                    != attempt.expected_head_checksum.as_deref()
                    || Some(path.as_str()) != attempt.expected_previous_publication.as_deref()
                {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog publication chain does not reach this attempt's exact base Head",
                    ));
                }
                return Ok(false);
            }
            let (previous_generation, previous_path, previous_checksum) = match (
                publication.previous_generation,
                publication.previous_publication,
                publication.previous_head_checksum,
            ) {
                (None, None, None) if publication.generation == 0 => {
                    return if attempt.expected_generation.is_none() {
                        Ok(false)
                    } else {
                        Err(Error::new(
                            ErrorKind::DataInvalid,
                            "Catalog publication chain does not reach this attempt's base generation",
                        ))
                    };
                }
                (Some(generation), Some(path), Some(checksum))
                    if generation + 1 == publication.generation =>
                {
                    (generation, path, checksum)
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog publication chain is incomplete or corrupt",
                    ));
                }
            };
            let previous = decode_publication(
                &self
                    .store
                    .read_publication(&previous_path)
                    .await
                    .map_err(storage_error)?,
            )?;
            if previous.generation != previous_generation
                || previous.next_head_checksum != previous_checksum
            {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog publication predecessor is corrupt",
                ));
            }
            head = previous.next_head.clone();
            path = previous_path;
        }
    }

    /// Validates the immutable evidence directly referenced by Head. This is
    /// deliberately constant work: opening a Space must load Head and the
    /// metadata it references, not replay the whole publication history.
    /// Unknown-outcome resolution traverses the chain only as far as its
    /// exact attempt base generation.
    async fn validate_head_publication(&self, head: &CatalogHead) -> Result<()> {
        let publication_path = head.publication_location.as_deref().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication record",
            )
        })?;
        let publication = decode_publication(
            &self
                .store
                .read_publication(publication_path)
                .await
                .map_err(storage_error)?,
        )?;
        validate_publication_matches_head(&publication, head)?;
        match (
            publication.previous_generation,
            publication.previous_publication.as_deref(),
            publication.previous_head_checksum.as_deref(),
        ) {
            (None, None, None) if publication.generation == 0 => Ok(()),
            (Some(previous_generation), Some(previous_path), Some(previous_checksum))
                if previous_generation + 1 == publication.generation =>
            {
                let previous = decode_publication(
                    &self
                        .store
                        .read_publication(previous_path)
                        .await
                        .map_err(storage_error)?,
                )?;
                if previous.generation != previous_generation
                    || previous.next_head_checksum != previous_checksum
                {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog publication predecessor is corrupt",
                    ));
                }
                Ok(())
            }
            _ => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog publication chain is incomplete or corrupt",
            )),
        }
    }
}

#[async_trait]
impl Catalog for SpaceCatalog {
    async fn list_namespaces(
        &self,
        parent: Option<&NamespaceIdent>,
    ) -> Result<Vec<NamespaceIdent>> {
        if parent.is_none() {
            Ok(vec![self.namespace.clone()])
        } else {
            Ok(Vec::new())
        }
    }

    async fn create_namespace(
        &self,
        namespace: &NamespaceIdent,
        _properties: HashMap<String, String>,
    ) -> Result<Namespace> {
        self.get_namespace(namespace).await
    }

    async fn get_namespace(&self, namespace: &NamespaceIdent) -> Result<Namespace> {
        if namespace == &self.namespace {
            Ok(Namespace::new(namespace.clone()))
        } else {
            Err(Error::new(
                ErrorKind::DataInvalid,
                "unknown Ugoite Space namespace",
            ))
        }
    }

    async fn namespace_exists(&self, namespace: &NamespaceIdent) -> Result<bool> {
        Ok(namespace == &self.namespace)
    }

    async fn update_namespace(
        &self,
        _namespace: &NamespaceIdent,
        _properties: HashMap<String, String>,
    ) -> Result<()> {
        Err(unsupported(
            "namespace properties are not part of the Ugoite Catalog model",
        ))
    }

    async fn drop_namespace(&self, _namespace: &NamespaceIdent) -> Result<()> {
        Err(unsupported(
            "dropping a Space namespace is not exposed by Ugoite",
        ))
    }

    async fn list_tables(&self, namespace: &NamespaceIdent) -> Result<Vec<TableIdent>> {
        if namespace != &self.namespace {
            return Ok(Vec::new());
        }
        let Some((head, _)) = self.exact_head().await? else {
            return Ok(Vec::new());
        };
        Ok(head
            .tables
            .values()
            .map(|reference| reference.identifier.to_table_ident())
            .collect())
    }

    async fn create_table(
        &self,
        namespace: &NamespaceIdent,
        creation: TableCreation,
    ) -> Result<iceberg::table::Table> {
        self.claim_mutation()?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        if namespace != &self.namespace {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "table namespace does not match Space",
            ));
        }
        let table = TableIdent::new(namespace.clone(), creation.name.clone());
        let attempt = PublicationAttempt::from_exact(&self.publication, self.exact_head().await?);
        if let Some(head) = &attempt.expected_head {
            if head.tables.contains_key(&Self::table_key(&table)) {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    format!("table already exists: {table}"),
                ));
            }
        }
        let location = creation.location.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Ugoite requires an explicit Iceberg table location",
            )
        })?;
        // Iceberg Rust's public table-creation builder intentionally assigns
        // fresh field IDs. Ugoite's Form IDs are already stable Iceberg IDs,
        // so preserve that schema in the resulting standard Iceberg metadata
        // rather than maintaining a second mapping document.
        let requested_schema = creation.schema.clone();
        let metadata = preserve_schema_field_ids(
            TableMetadataBuilder::from_table_creation(creation)?
                .build()?
                .metadata,
            requested_schema,
        )?;
        let metadata_location = MetadataLocation::new_with_metadata(location, &metadata);
        metadata.write_to(&self.file_io, &metadata_location).await?;
        let metadata_location = metadata_location.to_string();
        let created = iceberg::table::Table::builder()
            .identifier(table.clone())
            .metadata(metadata)
            .metadata_location(metadata_location.clone())
            .file_io(self.file_io.clone())
            .runtime(self.runtime.clone())
            .build()?;

        let mut next = attempt.expected_head.as_ref().map_or_else(
            || CatalogHead::genesis(self.space_id, &self.namespace),
            CatalogHead::next_generation,
        );
        next.tables.insert(
            Self::table_key(&table),
            TableReference::from_table(&created)?,
        );
        next.form_registry_generation += 1;
        let base_metadata_location = attempt
            .expected_head
            .as_ref()
            .and_then(|head| head.tables.get(&Self::table_key(&table)))
            .map(|reference| reference.metadata_location.clone());
        let publish = self
            .publish_new_head(
                &attempt,
                next,
                PublicationUpdate {
                    affected_table: TableCoordinates::from(&table),
                    base_metadata_location,
                    new_metadata_location: metadata_location,
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: created.metadata().current_snapshot_id(),
                    new_schema_id: created.metadata().current_schema_id(),
                },
            )
            .await;
        match publish {
            Ok(()) => Ok(created),
            Err(_error) if self.resolve_unknown_outcome(&attempt).await? => {
                self.load_table(&table).await
            }
            Err(error) => Err(error),
        }
    }

    async fn load_table(&self, table: &TableIdent) -> Result<iceberg::table::Table> {
        let (head, _) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        self.load_head_table(table, &head).await
    }

    async fn drop_table(&self, _table: &TableIdent) -> Result<()> {
        Err(unsupported("dropping Form tables is not exposed by Ugoite"))
    }

    async fn purge_table(&self, _table: &TableIdent) -> Result<()> {
        Err(unsupported("purging Form tables is not exposed by Ugoite"))
    }

    async fn table_exists(&self, table: &TableIdent) -> Result<bool> {
        Ok(self
            .exact_head()
            .await?
            .is_some_and(|(head, _)| head.tables.contains_key(&Self::table_key(table))))
    }

    async fn rename_table(&self, _src: &TableIdent, _dest: &TableIdent) -> Result<()> {
        Err(unsupported("renaming Form tables is not exposed by Ugoite"))
    }

    async fn register_table(
        &self,
        _table: &TableIdent,
        _metadata_location: String,
    ) -> Result<iceberg::table::Table> {
        Err(unsupported(
            "registering tables would bypass Catalog Head publication",
        ))
    }

    async fn update_table(&self, commit: TableCommit) -> Result<iceberg::table::Table> {
        self.claim_mutation()?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let table = commit.identifier().clone();
        let attempt = PublicationAttempt::from_exact(&self.publication, self.exact_head().await?);
        let head = attempt
            .expected_head
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        let base = self.load_head_table(&table, head).await?;
        let base_metadata_location = base.metadata_location_result()?.to_string();
        let base_snapshot_id = base.metadata().current_snapshot_id();
        let base_schema_id = base.metadata().current_schema_id();
        let staged = commit.apply(base)?;
        let new_metadata_location = staged.metadata_location_result()?.to_string();
        staged
            .metadata()
            .write_to(
                staged.file_io(),
                &MetadataLocation::from_str(&new_metadata_location)?,
            )
            .await?;
        let mut next = head.next_generation();
        next.tables.insert(
            Self::table_key(&table),
            TableReference::from_table(&staged)?,
        );
        let publication = self
            .publish_new_head(
                &attempt,
                next,
                PublicationUpdate {
                    affected_table: TableCoordinates::from(&table),
                    base_metadata_location: Some(base_metadata_location),
                    new_metadata_location,
                    base_snapshot_id,
                    base_schema_id: Some(base_schema_id),
                    new_snapshot_id: staged.metadata().current_snapshot_id(),
                    new_schema_id: staged.metadata().current_schema_id(),
                },
            )
            .await;
        match publication {
            Ok(()) => Ok(staged),
            Err(_error) if self.resolve_unknown_outcome(&attempt).await? => {
                self.load_table(&table).await
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CatalogHead {
    format_version: u32,
    space_id: String,
    namespace: Vec<String>,
    generation: u64,
    form_registry_generation: u64,
    tables: BTreeMap<String, TableReference>,
    publication_location: Option<String>,
    publication_command_id: Option<String>,
    checksum: String,
}

impl CatalogHead {
    fn genesis(space_id: SpaceId, namespace: &NamespaceIdent) -> Self {
        Self {
            format_version: SPACE_FORMAT_VERSION,
            space_id: space_id.to_string(),
            namespace: namespace.as_ref().clone(),
            generation: 0,
            form_registry_generation: 0,
            tables: BTreeMap::new(),
            publication_location: None,
            publication_command_id: None,
            checksum: String::new(),
        }
    }

    fn next_generation(&self) -> Self {
        let mut next = self.clone();
        next.generation += 1;
        next
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TableCoordinates {
    namespace: Vec<String>,
    table: String,
}

impl From<&TableIdent> for TableCoordinates {
    fn from(table: &TableIdent) -> Self {
        Self {
            namespace: table.namespace().as_ref().clone(),
            table: table.name().to_string(),
        }
    }
}

impl TableCoordinates {
    fn to_table_ident(&self) -> TableIdent {
        TableIdent::new(
            NamespaceIdent::from_vec(self.namespace.clone()).expect("stored namespace"),
            self.table.clone(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TableReference {
    identifier: TableCoordinates,
    form_id: Option<String>,
    table_uuid: String,
    metadata_location: String,
}

impl TableReference {
    fn from_table(table: &iceberg::table::Table) -> Result<Self> {
        Ok(Self {
            identifier: TableCoordinates::from(table.identifier()),
            form_id: table.metadata().properties().get("ugoite.form.id").cloned(),
            table_uuid: table.metadata().uuid().to_string(),
            metadata_location: table.metadata_location_result()?.to_string(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PublicationRecord {
    generation: u64,
    previous_generation: Option<u64>,
    previous_publication: Option<String>,
    previous_head_checksum: Option<String>,
    command_id: String,
    command_kind: String,
    command_digest: String,
    affected_table: TableCoordinates,
    base_metadata_location: Option<String>,
    new_metadata_location: String,
    base_snapshot_id: Option<i64>,
    base_schema_id: Option<i32>,
    new_snapshot_id: Option<i64>,
    new_schema_id: i32,
    next_head_checksum: String,
    next_head: CatalogHead,
    checksum: String,
}

fn checksum(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn head_checksum(head: &CatalogHead) -> Result<String> {
    let mut canonical = head.clone();
    canonical.checksum.clear();
    Ok(checksum(
        &serde_json::to_vec(&canonical).map_err(json_error)?,
    ))
}

fn publication_checksum(publication: &PublicationRecord) -> Result<String> {
    let mut canonical = publication.clone();
    canonical.checksum.clear();
    Ok(checksum(
        &serde_json::to_vec(&canonical).map_err(json_error)?,
    ))
}

fn encode_head(head: &CatalogHead) -> Result<Vec<u8>> {
    let mut head = head.clone();
    head.checksum = head_checksum(&head)?;
    serde_json::to_vec(&head).map_err(json_error)
}

fn decode_head(bytes: &[u8]) -> Result<CatalogHead> {
    let head: CatalogHead = serde_json::from_slice(bytes).map_err(json_error)?;
    if head.format_version != SPACE_FORMAT_VERSION {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            "unsupported Space format version",
        ));
    }
    if head.checksum != head_checksum(&head)? {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            "Catalog Head checksum mismatch",
        ));
    }
    Ok(head)
}

fn encode_publication(publication: &PublicationRecord) -> Result<Vec<u8>> {
    let mut publication = publication.clone();
    publication.checksum = publication_checksum(&publication)?;
    serde_json::to_vec(&publication).map_err(json_error)
}

fn decode_publication(bytes: &[u8]) -> Result<PublicationRecord> {
    let publication: PublicationRecord = serde_json::from_slice(bytes).map_err(json_error)?;
    if publication.checksum != publication_checksum(&publication)? {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            "Catalog publication checksum mismatch",
        ));
    }
    if publication.next_head.checksum != publication.next_head_checksum {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            "Catalog publication Head checksum mismatch",
        ));
    }
    Ok(publication)
}

fn preserve_schema_field_ids(
    metadata: TableMetadata,
    requested_schema: iceberg::spec::Schema,
) -> Result<TableMetadata> {
    let requested_schema = requested_schema
        .into_builder()
        .with_schema_id(metadata.current_schema_id())
        .build()?;
    let mut encoded = serde_json::to_value(metadata).map_err(json_error)?;
    let object = encoded
        .as_object_mut()
        .ok_or_else(|| Error::new(ErrorKind::Unexpected, "Iceberg metadata is not an object"))?;
    object.insert(
        "schemas".to_string(),
        serde_json::to_value(vec![requested_schema.clone()]).map_err(json_error)?,
    );
    object.insert(
        "last-column-id".to_string(),
        Value::from(requested_schema.highest_field_id()),
    );
    serde_json::from_value(encoded).map_err(json_error)
}

fn validate_publication_matches_head(
    publication: &PublicationRecord,
    head: &CatalogHead,
) -> Result<()> {
    if publication.generation != head.generation
        || publication.next_head_checksum != head.checksum
        || publication.next_head != *head
        || head.publication_command_id.as_deref() != Some(publication.command_id.as_str())
    {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            "Catalog publication does not match Catalog Head",
        ));
    }
    Ok(())
}

fn storage_error(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Unexpected, error.to_string())
}

fn json_error(error: serde_json::Error) -> Error {
    Error::new(ErrorKind::DataInvalid, error.to_string())
}

fn unsupported(message: &str) -> Error {
    Error::new(ErrorKind::FeatureUnsupported, message)
}

fn is_condition_conflict(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<opendal::Error>()
        .is_some_and(|error| error.kind() == opendal::ErrorKind::ConditionNotMatch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iceberg::transaction::ApplyTransactionAction;
    use opendal::services::{Fs, Memory};
    use opendal::Operator;
    use tempfile::tempdir;

    #[tokio::test]
    async fn creates_reopens_and_updates_a_table_through_head_publication() -> AnyResult<()> {
        let temp = tempdir()?;
        let operator =
            Operator::new(Fs::default().root(temp.path().to_string_lossy().as_ref()))?.finish();
        let catalog = SpaceCatalog::new(
            SpaceCatalogStore::new(operator.clone(), "spaces/demo")?.single_process(),
            SpaceId::from(Uuid::from_u128(1)),
        )?;
        // This simulates a client losing the successful Head-CAS response.
        // The same command must remain provably successful after another
        // writer advances the Catalog.
        let initial_attempt =
            PublicationAttempt::from_exact(&catalog.publication, catalog.exact_head().await?);
        let namespace = catalog.namespace().clone();
        let schema = iceberg::spec::Schema::builder()
            .with_fields(vec![])
            .build()?;
        let created = catalog
            .create_table(
                &namespace,
                TableCreation::builder()
                    .name("form_00000000000000000000000000000001".to_string())
                    .location(format!(
                        "file://{}/spaces/demo/forms/form",
                        temp.path().display()
                    ))
                    .schema(schema)
                    .build(),
            )
            .await?;
        assert!(catalog.table_exists(created.identifier()).await?);
        let update_catalog = SpaceCatalog::new(
            SpaceCatalogStore::new(operator.clone(), "spaces/demo")?.single_process(),
            SpaceId::from(Uuid::from_u128(1)),
        )?;
        let update_table = update_catalog.load_table(created.identifier()).await?;
        let transaction = iceberg::transaction::Transaction::new(&update_table);
        let transaction = transaction
            .update_table_properties()
            .set("ugoite.test.commit".to_string(), "published".to_string())
            .apply(transaction)?;
        let committed = transaction.commit(&update_catalog).await?;
        assert_eq!(
            committed.metadata().properties().get("ugoite.test.commit"),
            Some(&"published".to_string())
        );
        let (head, _) = update_catalog.exact_head().await?.expect("Catalog Head");
        let publication_path = head
            .publication_location
            .as_deref()
            .expect("Head publication location");
        let publication = decode_publication(
            &update_catalog
                .store
                .read_publication(publication_path)
                .await?,
        )?;
        assert_eq!(
            publication.base_snapshot_id,
            created.metadata().current_snapshot_id()
        );
        assert_eq!(
            publication.base_schema_id,
            Some(created.metadata().current_schema_id())
        );
        assert_eq!(
            publication.new_snapshot_id,
            committed.metadata().current_snapshot_id()
        );
        assert_eq!(
            publication.new_schema_id,
            committed.metadata().current_schema_id()
        );
        assert!(catalog.resolve_unknown_outcome(&initial_attempt).await?);
        let reopened = SpaceCatalog::new(
            SpaceCatalogStore::new(operator.clone(), "spaces/demo")?.single_process(),
            SpaceId::from(Uuid::from_u128(1)),
        )?;
        assert_eq!(reopened.list_tables(&namespace).await?.len(), 1);
        let loaded = reopened.load_table(created.identifier()).await?;
        assert_eq!(
            loaded.metadata().properties().get("ugoite.test.commit"),
            Some(&"published".to_string())
        );
        operator
            .write(publication_path, b"corrupt".to_vec())
            .await?;
        let error = reopened
            .exact_head()
            .await
            .expect_err("corrupt reachable publication evidence must fail");
        assert_eq!(error.kind(), ErrorKind::DataInvalid);
        Ok(())
    }

    #[tokio::test]
    async fn keeps_memory_metadata_in_the_same_space_operator() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let catalog = SpaceCatalog::new(
            SpaceCatalogStore::new(operator.clone(), "spaces/memory")?.single_process(),
            SpaceId::from(Uuid::from_u128(2)),
        )?;
        let namespace = catalog.namespace().clone();
        let schema = iceberg::spec::Schema::builder()
            .with_fields(vec![])
            .build()?;
        let created = catalog
            .create_table(
                &namespace,
                TableCreation::builder()
                    .name("form_00000000000000000000000000000002".to_string())
                    .location("memory:///spaces/memory/forms/form".to_string())
                    .schema(schema)
                    .build(),
            )
            .await?;
        let reopened = SpaceCatalog::new(
            SpaceCatalogStore::new(operator, "spaces/memory")?.single_process(),
            SpaceId::from(Uuid::from_u128(2)),
        )?;
        assert!(reopened.table_exists(created.identifier()).await?);
        Ok(())
    }

    #[tokio::test]
    async fn ignores_an_interrupted_publication_before_head_cas() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let catalog = SpaceCatalog::new(
            SpaceCatalogStore::new(operator, "spaces/interrupted")?.single_process(),
            SpaceId::from(Uuid::from_u128(4)),
        )?
        .with_publication_context(PublicationContext::with_command_digest(
            "interrupted-command",
            "test",
            "interrupted-digest",
        ));
        let attempt =
            PublicationAttempt::from_exact(&catalog.publication, catalog.exact_head().await?);
        let table = TableIdent::new(catalog.namespace().clone(), "form_interrupted".to_string());
        let mut next = CatalogHead::genesis(catalog.space_id, catalog.namespace());
        let publication_path = catalog
            .store
            .publication_path(next.generation, &catalog.publication.command_id);
        next.publication_location = Some(publication_path);
        next.publication_command_id = Some(catalog.publication.command_id.clone());
        next.checksum = head_checksum(&next)?;
        let mut publication = PublicationRecord {
            generation: next.generation,
            previous_generation: None,
            previous_publication: None,
            previous_head_checksum: None,
            command_id: catalog.publication.command_id.clone(),
            command_kind: catalog.publication.command_kind.clone(),
            command_digest: catalog.publication.command_digest.clone(),
            affected_table: TableCoordinates::from(&table),
            base_metadata_location: None,
            new_metadata_location: "memory:///spaces/interrupted/forms/form/metadata.json"
                .to_string(),
            base_snapshot_id: None,
            base_schema_id: None,
            new_snapshot_id: None,
            new_schema_id: 0,
            next_head_checksum: next.checksum.clone(),
            next_head: next,
            checksum: String::new(),
        };
        publication.checksum = publication_checksum(&publication)?;
        catalog.write_publication(&publication).await?;

        assert!(catalog.exact_head().await?.is_none());
        assert!(!catalog.resolve_unknown_outcome(&attempt).await?);
        Ok(())
    }

    #[tokio::test]
    async fn stale_initialization_does_not_replace_the_winner_head() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?.finish();
        let winner = SpaceCatalog::new(
            SpaceCatalogStore::new(operator.clone(), "spaces/conflict")?.single_process(),
            SpaceId::from(Uuid::from_u128(5)),
        )?
        .with_publication_context(PublicationContext::with_command_digest(
            "winner-command",
            "test",
            "winner-digest",
        ));
        let table = TableIdent::new(winner.namespace().clone(), "form_conflict".to_string());
        let winner_attempt =
            PublicationAttempt::from_exact(&winner.publication, winner.exact_head().await?);
        winner
            .publish_new_head(
                &winner_attempt,
                CatalogHead::genesis(winner.space_id, winner.namespace()),
                PublicationUpdate {
                    affected_table: TableCoordinates::from(&table),
                    base_metadata_location: None,
                    new_metadata_location: "memory:///spaces/conflict/forms/form/metadata.json"
                        .to_string(),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await?;

        let loser = SpaceCatalog::new(
            SpaceCatalogStore::new(operator, "spaces/conflict")?.single_process(),
            SpaceId::from(Uuid::from_u128(5)),
        )?
        .with_publication_context(PublicationContext::with_command_digest(
            "loser-command",
            "test",
            "loser-digest",
        ));
        let stale_attempt = PublicationAttempt::from_exact(&loser.publication, None);
        let error = loser
            .publish_new_head(
                &stale_attempt,
                CatalogHead::genesis(loser.space_id, loser.namespace()),
                PublicationUpdate {
                    affected_table: TableCoordinates::from(&table),
                    base_metadata_location: None,
                    new_metadata_location: "memory:///spaces/conflict/forms/form/metadata.json"
                        .to_string(),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await
            .expect_err("a stale initial attempt cannot replace the winner Head");
        assert!(error.to_string().contains("Catalog Head already exists"));
        assert!(!loser.resolve_unknown_outcome(&stale_attempt).await?);
        let (head, _) = winner.exact_head().await?.expect("winner Head");
        assert_eq!(
            head.publication_command_id.as_deref(),
            Some("winner-command")
        );
        Ok(())
    }

    #[tokio::test]
    async fn opens_the_head_after_many_publications_without_replaying_history() -> AnyResult<()> {
        let temp = tempdir()?;
        let operator =
            Operator::new(Fs::default().root(temp.path().to_string_lossy().as_ref()))?.finish();
        let store =
            SpaceCatalogStore::new(operator.clone(), "spaces/many-publications")?.single_process();
        let space_id = SpaceId::from(Uuid::from_u128(3));
        let catalog = SpaceCatalog::new(store.clone(), space_id)?;
        let namespace = catalog.namespace().clone();
        let created = catalog
            .create_table(
                &namespace,
                TableCreation::builder()
                    .name("form_00000000000000000000000000000003".to_string())
                    .location(format!(
                        "file://{}/spaces/many-publications/forms/form",
                        temp.path().display()
                    ))
                    .schema(
                        iceberg::spec::Schema::builder()
                            .with_fields(vec![])
                            .build()?,
                    )
                    .build(),
            )
            .await?;
        for generation in 1..=128 {
            let attempt = SpaceCatalog::new(store.clone(), space_id)?;
            let table = attempt.load_table(created.identifier()).await?;
            let transaction = iceberg::transaction::Transaction::new(&table);
            let transaction = transaction
                .update_table_properties()
                .set("ugoite.test.generation".to_string(), generation.to_string())
                .apply(transaction)?;
            transaction.commit(&attempt).await?;
        }
        let reopened = SpaceCatalog::new(store, space_id)?;
        let table = reopened.load_table(created.identifier()).await?;
        assert_eq!(
            table.metadata().properties().get("ugoite.test.generation"),
            Some(&"128".to_string())
        );
        Ok(())
    }
}
