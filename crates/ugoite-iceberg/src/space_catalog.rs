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
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
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

    pub fn with_publication_context(mut self, publication: PublicationContext) -> Self {
        self.publication = publication;
        self
    }

    pub fn namespace(&self) -> &NamespaceIdent {
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
        self.validate_publication_chain(&head).await?;
        Ok(Some((head, exact)))
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
        previous: Option<(&CatalogHead, Option<&str>)>,
        expected_etag: Option<&str>,
        mut next: CatalogHead,
        affected_table: &TableIdent,
        base_metadata_location: Option<String>,
        new_metadata_location: String,
    ) -> Result<()> {
        let previous_generation = previous.map(|(head, _)| head.generation);
        let previous_publication = previous.and_then(|(head, _)| head.publication_location.clone());
        let previous_head_checksum = previous.map(|(head, _)| head.checksum.clone());
        let publication_path = self
            .store
            .publication_path(next.generation, &self.publication.command_id);
        next.publication_location = Some(publication_path);
        next.publication_command_id = Some(self.publication.command_id.clone());
        next.checksum = head_checksum(&next)?;
        let publication = PublicationRecord {
            generation: next.generation,
            previous_generation,
            previous_publication,
            previous_head_checksum,
            command_id: self.publication.command_id.clone(),
            command_kind: self.publication.command_kind.clone(),
            command_digest: self.publication.command_digest.clone(),
            affected_table: TableCoordinates::from(affected_table),
            base_metadata_location,
            new_metadata_location,
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
        let result = match previous {
            Some(_) => self.store.replace_head(expected_etag, bytes).await,
            None => self.store.create_head(bytes).await,
        };
        result.map_err(storage_error)
    }

    async fn resolve_unknown_outcome(&self, base_generation: Option<u64>) -> Result<bool> {
        let Some((head, _)) = self.exact_head().await? else {
            return Ok(false);
        };
        let mut next_publication = head.publication_location.clone();
        while let Some(path) = next_publication {
            let publication = decode_publication(
                &self
                    .store
                    .read_publication(&path)
                    .await
                    .map_err(storage_error)?,
            )?;
            if publication.command_id == self.publication.command_id {
                if publication.command_kind == self.publication.command_kind
                    && publication.command_digest == self.publication.command_digest
                {
                    return Ok(true);
                }
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "publication command id was reused with different command content",
                ));
            }
            if publication.generation == base_generation.unwrap_or_default() {
                return Ok(false);
            }
            next_publication = publication.previous_publication;
        }
        Ok(false)
    }

    async fn validate_publication_chain(&self, head: &CatalogHead) -> Result<()> {
        let mut expected_generation = head.generation;
        let mut expected_checksum = head.checksum.clone();
        let mut publication_path = head.publication_location.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication record",
            )
        })?;
        loop {
            let publication = decode_publication(
                &self
                    .store
                    .read_publication(&publication_path)
                    .await
                    .map_err(storage_error)?,
            )?;
            if publication.generation != expected_generation
                || publication.next_head_checksum != expected_checksum
                || publication.next_head.checksum != expected_checksum
                || (publication.generation == head.generation
                    && (publication.next_head != *head
                        || head.publication_command_id.as_deref()
                            != Some(publication.command_id.as_str())))
            {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog publication chain does not match Catalog Head",
                ));
            }
            match (
                publication.previous_generation,
                publication.previous_publication,
            ) {
                (None, None) if publication.generation == 0 => return Ok(()),
                (Some(previous_generation), Some(previous_path))
                    if previous_generation + 1 == publication.generation =>
                {
                    expected_generation = previous_generation;
                    expected_checksum = publication.previous_head_checksum.ok_or_else(|| {
                        Error::new(
                            ErrorKind::DataInvalid,
                            "Catalog publication is missing its previous Head checksum",
                        )
                    })?;
                    publication_path = previous_path;
                }
                _ => {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog publication chain is incomplete or corrupt",
                    ));
                }
            }
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
        if let Some((head, _)) = self.exact_head().await? {
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
        let metadata = TableMetadataBuilder::from_table_creation(creation)?
            .build()?
            .metadata;
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

        let exact = self.exact_head().await?;
        let (mut next, previous, etag) = match exact {
            Some((head, exact)) => (head.next_generation(), Some(head), exact.etag),
            None => (
                CatalogHead::genesis(self.space_id, &self.namespace),
                None,
                None,
            ),
        };
        next.tables.insert(
            Self::table_key(&table),
            TableReference::from_table(&created)?,
        );
        next.form_registry_generation += 1;
        let base_metadata_location = previous
            .as_ref()
            .and_then(|head| head.tables.get(&Self::table_key(&table)))
            .map(|reference| reference.metadata_location.clone());
        let publish = self
            .publish_new_head(
                previous
                    .as_ref()
                    .map(|head| (head, head.publication_location.as_deref())),
                etag.as_deref(),
                next,
                &table,
                base_metadata_location,
                metadata_location,
            )
            .await;
        match publish {
            Ok(()) => Ok(created),
            Err(_error)
                if self
                    .resolve_unknown_outcome(previous.as_ref().map(|head| head.generation))
                    .await? =>
            {
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
        let (head, exact) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        let base = self.load_head_table(&table, &head).await?;
        let base_metadata_location = base.metadata_location_result()?.to_string();
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
                Some((&head, head.publication_location.as_deref())),
                exact.etag.as_deref(),
                next,
                &table,
                Some(base_metadata_location),
                new_metadata_location,
            )
            .await;
        match publication {
            Ok(()) => Ok(staged),
            Err(_error) if self.resolve_unknown_outcome(Some(head.generation)).await? => {
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
    next_head_checksum: String,
    next_head: CatalogHead,
    checksum: String,
}

fn checksum(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
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
        let reopened = SpaceCatalog::new(
            SpaceCatalogStore::new(operator, "spaces/demo")?.single_process(),
            SpaceId::from(Uuid::from_u128(1)),
        )?;
        assert_eq!(reopened.list_tables(&namespace).await?.len(), 1);
        let loaded = reopened.load_table(created.identifier()).await?;
        assert_eq!(
            loaded.metadata().properties().get("ugoite.test.commit"),
            Some(&"published".to_string())
        );
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
}
