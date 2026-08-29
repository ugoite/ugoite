#[cfg(test)]
use anyhow::Result as AnyResult;
use async_trait::async_trait;
use iceberg::io::{FileIO, FileIOBuilder};
use iceberg::spec::{FormatVersion, TableMetadata, TableMetadataBuilder};
use iceberg::{
    Catalog, Error, ErrorKind, MetadataLocation, Namespace, NamespaceIdent, Result, Runtime,
    TableCommit, TableCreation, TableIdent,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use ugoite_domain::change::ChangeDescriptor;
use ugoite_domain::checkpoint::{CheckpointTable, SpaceCheckpoint};
use ugoite_domain::id::{validate_asset_id, FormId, SpaceId};
use ugoite_domain::pin::PinEntry;
use ugoite_domain::publication_ref::PublicationRef;
use ugoite_domain::space_key::{SpaceKey, SpaceUri};
use ugoite_storage::{
    CatalogMutationPermit, CatalogWriteMode, ExactCatalogHead, SpaceCatalogStore,
};
use uuid::Uuid;

use crate::health::{
    BackendHealth, BackendMode, BackendProbeStatus, CatalogHeadHealth, CheckpointHealth,
    FileSizeDistribution, HealthIssue, HealthStatus, SpaceHealthReport, TableHealth,
    TableIdentifierHealth, UnavailableCapability,
};
use crate::logical_storage::{logical_space_uid, LogicalStorageFactory};
use crate::FORM_ID_PROPERTY;

const SPACE_FORMAT_VERSION: u32 = 1;
const MAX_HEAD_BYTES: usize = 1 << 20;
const SMALL_FILE_THRESHOLD_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PIN_NAME_BYTES: usize = 128;
const MAX_PIN_COUNT: usize = 1024;

fn validate_pin_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.len() > MAX_PIN_NAME_BYTES
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
    {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            "pin name is empty, too long, or contains a path component",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PublicationContext {
    pub(crate) command_id: String,
    pub(crate) command_kind: String,
    pub(crate) command_digest: String,
    /// Semantic metadata for a user-visible Knowledge mutation.
    pub(crate) change: Option<ChangeDescriptor>,
}

/// Outcome of a committed command publication. The durable evidence is the
/// immutable publication reachable from Catalog Head; this value is only the
/// domain-facing result returned by a read of that evidence.
#[derive(Debug, Clone, Eq, PartialEq)]
pub(crate) struct PublicationOutcome {
    pub command_id: String,
    pub catalog_generation: u64,
    pub snapshot_id: Option<i64>,
}

impl PublicationContext {
    pub fn command_id(&self) -> &str {
        &self.command_id
    }

    pub(crate) fn new(command_id: impl Into<String>, command_kind: impl Into<String>) -> Self {
        let command_id = command_id.into();
        let command_kind = command_kind.into();
        let command_digest = checksum(format!("{command_id}:{command_kind}").as_bytes());
        Self {
            command_id,
            command_kind,
            command_digest,
            change: None,
        }
    }

    /// Uses the digest of the domain command coordinated by the caller. A
    /// retry must reuse all three values; otherwise it is a different attempt.
    pub(crate) fn with_command_digest(
        command_id: impl Into<String>,
        command_kind: impl Into<String>,
        command_digest: impl Into<String>,
    ) -> Self {
        Self {
            command_id: command_id.into(),
            command_kind: command_kind.into(),
            command_digest: command_digest.into(),
            change: None,
        }
    }

    pub(crate) fn with_change_descriptor(
        mut self,
        change: ChangeDescriptor,
    ) -> std::result::Result<Self, ugoite_domain::change::ChangeValidationError> {
        change.validate()?;
        self.change = Some(change);
        Ok(self)
    }

    fn generated() -> Self {
        Self::new(Uuid::new_v4().to_string(), "iceberg-catalog")
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.command_id.trim().is_empty() || self.command_kind.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "publication command identity must not be empty",
            ));
        }
        if let Some(change) = &self.change {
            change
                .validate()
                .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))?;
        } else if !matches!(self.command_kind.as_str(), "pin.create" | "pin.delete")
            && !self.command_kind.starts_with("test.")
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Knowledge publication requires a Change descriptor",
            ));
        }
        Ok(())
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
    logical_space_uid: Uuid,
    file_io: FileIO,
    runtime: Runtime,
    publication: PublicationContext,
    mutation_claimed: Arc<AtomicBool>,
    #[cfg(debug_assertions)]
    publication_gate: Option<Arc<crate::TestPublicationGate>>,
    /// When present, every read and mutation on this catalog is evaluated
    /// against this exact Head.  A coordinator creates one of these for an
    /// operation so validation and publication cannot silently switch to a
    /// newer base Head between the two steps.
    bound_attempt: Option<Arc<PublicationAttempt>>,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublishedChange {
    pub change_id: String,
    pub generation: u64,
    pub change: ChangeDescriptor,
    pub publication: PublicationRef,
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
        let logical_space_uid = logical_space_uid(space_id);
        let file_io = FileIOBuilder::new(Arc::new(LogicalStorageFactory::new(
            store.operator().clone(),
            store.space_root(),
            logical_space_uid,
        )))
        .build();
        Ok(Self {
            store,
            namespace: NamespaceIdent::new(format!("space_{}", space_id.as_uuid().simple())),
            space_id,
            logical_space_uid,
            file_io,
            runtime: Runtime::current(),
            publication: PublicationContext::generated(),
            mutation_claimed: Arc::new(AtomicBool::new(false)),
            #[cfg(debug_assertions)]
            publication_gate: crate::current_test_publication_gate(),
            bound_attempt: None,
        })
    }

    pub(crate) fn ensure_authoritative_mutation_contract(&self) -> anyhow::Result<()> {
        self.store.mutation_permit().map(|_| ()).map_err(|_| {
            ugoite_core::error::AppError::dependency_unavailable(
                ugoite_core::error::ErrorCode::StorageMutationUnavailable,
                "authoritative Space mutations require a verified exact-read and single-Head-CAS storage contract",
            )
            .into()
        })
    }

    fn mutation_permit(&self) -> anyhow::Result<CatalogMutationPermit> {
        self.store.mutation_permit()
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
            logical_space_uid: self.logical_space_uid,
            file_io: self.file_io.clone(),
            runtime: self.runtime.clone(),
            publication: PublicationContext::generated(),
            mutation_claimed: Arc::new(AtomicBool::new(false)),
            #[cfg(debug_assertions)]
            publication_gate: crate::current_test_publication_gate(),
            bound_attempt: None,
        }
    }

    /// Captures one immutable publication attempt.  The same attempt is used
    /// by all Iceberg table reads and by the eventual Catalog Head CAS.
    pub(crate) async fn bind_exact_head(mut self) -> Result<Self> {
        let exact = self.exact_head().await?;
        self.bound_attempt = Some(Arc::new(PublicationAttempt::from_exact(
            &self.publication,
            exact,
        )));
        Ok(self)
    }

    async fn publication_attempt(&self) -> Result<PublicationAttempt> {
        if let Some(attempt) = &self.bound_attempt {
            return Ok((**attempt).clone());
        }
        Ok(PublicationAttempt::from_exact(
            &self.publication,
            self.exact_head().await?,
        ))
    }

    async fn load_live_table(&self, table: &TableIdent) -> Result<iceberg::table::Table> {
        let (head, _) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        self.load_head_table(table, &head).await
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
        validate_head_pins(&head, self.logical_space_uid)?;
        self.validate_head_publication(&head).await?;
        Ok(Some((head, exact)))
    }

    async fn publication_ref_for_head(&self, head: &CatalogHead) -> Result<PublicationRef> {
        let publication_path = head.publication_location.as_deref().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication location",
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
        self.publication_ref_for_record(publication_path, &publication)
    }

    pub(crate) async fn current_publication(&self) -> Result<PublicationRef> {
        let (head, _) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        self.validate_pin_references(&head).await?;
        self.publication_ref_for_head(&head).await
    }

    /// Active Pins are read exactly from Catalog Head. Storage listing is not
    /// involved and the returned map is therefore a complete current view.
    pub async fn list_pins(&self) -> Result<BTreeMap<String, PinEntry>> {
        let Some((head, _)) = self.exact_head().await? else {
            return Ok(BTreeMap::new());
        };
        self.validate_pin_references(&head).await?;
        Ok(head.pins)
    }

    /// Resolves one active Pin from the authoritative Head.  The returned
    /// value is still only the portable publication coordinate; callers must
    /// resolve that coordinate before opening any immutable table metadata.
    pub async fn get_pin(&self, name: &str) -> Result<PinEntry> {
        validate_pin_name(name)?;
        let Some((head, _)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head is missing",
            ));
        };
        self.validate_pin_references(&head).await?;
        head.pins
            .get(name)
            .cloned()
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "pin not found"))
    }

    /// Reconstruct committed Changes from the reachable immutable publication
    /// chain. This is intentionally a history read, not a secondary index.
    pub async fn list_changes(&self) -> Result<Vec<PublishedChange>> {
        let (mut head, _) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        let mut path = head.publication_location.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication location",
            )
        })?;
        let mut visited = BTreeSet::new();
        let mut changes = Vec::new();
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
            let publication_ref = self.publication_ref_for_record(&path, &publication)?;
            if let Some(change) = publication.change.clone() {
                changes.push(PublishedChange {
                    change_id: publication.command_id.clone(),
                    generation: publication.generation,
                    change,
                    publication: publication_ref,
                });
            }
            let (previous_generation, previous_path, previous_checksum) = match (
                publication.previous_generation,
                publication.previous_publication,
                publication.previous_head_checksum,
            ) {
                (None, None, None) if publication.generation == 0 => break,
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
            head = previous.next_head;
            path = previous_path;
        }
        changes.reverse();
        Ok(changes)
    }

    fn publication_uri(&self, publication_path: &str) -> Result<SpaceUri> {
        let prefix = if self.store.space_root().is_empty() {
            String::new()
        } else {
            format!("{}/", self.store.space_root())
        };
        let key = publication_path.strip_prefix(&prefix).ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog publication is outside the bound Space",
            )
        })?;
        SpaceUri::new(
            self.logical_space_uid,
            SpaceKey::parse(key)
                .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))?,
        )
        .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))
    }

    /// Converts one committed publication from its canonical storage key into
    /// the domain coordinate exposed by every public adapter. The path check
    /// keeps semantic command IDs out of storage keys and makes malformed or
    /// detached publication records fail closed before serialization.
    fn publication_ref_for_record(
        &self,
        publication_path: &str,
        publication: &PublicationRecord,
    ) -> Result<PublicationRef> {
        if publication_path
            != self
                .store
                .publication_path(publication.generation, &publication.command_id)
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog publication is not at its canonical storage coordinate",
            ));
        }
        let publication_uri = self.publication_uri(publication_path)?;
        PublicationRef::new(
            publication.generation,
            publication_uri,
            publication.checksum.clone(),
        )
        .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))
    }

    async fn validate_pin_references(&self, head: &CatalogHead) -> Result<()> {
        if head.pins.is_empty() {
            return Ok(());
        }
        let mut remaining = head
            .pins
            .values()
            .map(|pin| {
                (
                    pin.coordinate.generation,
                    pin.coordinate.publication_uri.to_string(),
                    pin.coordinate.publication_checksum.clone(),
                )
            })
            .collect::<BTreeSet<_>>();
        let mut cursor = head.clone();
        let mut path = head.publication_location.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication location",
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
                    .map_err(pin_reference_target_error)?,
            )?;
            validate_publication_matches_head(&publication, &cursor)?;
            let publication_ref = self.publication_ref_for_record(&path, &publication)?;
            let coordinate = (
                publication_ref.generation,
                publication_ref.publication_uri.to_string(),
                publication_ref.publication_checksum,
            );
            remaining.remove(&coordinate);
            let (previous_generation, previous_path, previous_checksum) = match (
                publication.previous_generation,
                publication.previous_publication,
                publication.previous_head_checksum,
            ) {
                (None, None, None) if publication.generation == 0 => break,
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
                    .map_err(pin_reference_target_error)?,
            )?;
            if previous.generation != previous_generation
                || previous.next_head_checksum != previous_checksum
            {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog publication predecessor is corrupt",
                ));
            }
            cursor = previous.next_head;
            path = previous_path;
        }
        if remaining.is_empty() {
            Ok(())
        } else {
            Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head contains a Pin to an unreachable publication",
            ))
        }
    }

    /// Resolves a portable PublicationRef through the authoritative Head and
    /// returns the same immutable table coordinates that a checkpoint reader
    /// needs.  The temporary SpaceCheckpoint is never persisted: the
    /// PublicationRef and its reachable publication are the sole selection
    /// authority.
    pub(crate) async fn resolve_publication_checkpoint(
        &self,
        coordinate: &PublicationRef,
    ) -> anyhow::Result<SpaceCheckpoint> {
        coordinate
            .validate()
            .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()))?;
        if coordinate.publication_uri.space_uid() != self.logical_space_uid
            || !coordinate
                .publication_uri
                .key()
                .as_str()
                .starts_with("_ugoite/catalog/publications/")
        {
            return Err(crate::CheckpointIntegrityError::new(
                "publication reference belongs to another Space or is not a Catalog publication",
            )
            .into());
        }
        let target_path = self.publication_path(&coordinate.publication_uri)?;
        let (mut head, _) = match self.exact_head().await {
            Ok(Some(head)) => head,
            Ok(None) => return Err(crate::CheckpointUnavailable::new("Catalog Head").into()),
            Err(error) if error.to_string().contains("NotFound") => {
                return Err(crate::CheckpointUnavailable::new("Catalog Head publication").into());
            }
            Err(error) => return Err(anyhow::Error::new(error)),
        };
        let mut path = head
            .publication_location
            .clone()
            .ok_or_else(|| crate::CheckpointUnavailable::new("Catalog publication chain"))?;
        let mut visited = BTreeSet::new();
        let mut selected = None;
        loop {
            if !visited.insert(path.clone()) {
                return Err(crate::CheckpointIntegrityError::new(
                    "Catalog publication chain contains a cycle",
                )
                .into());
            }
            let publication = decode_publication(
                &self
                    .store
                    .read_publication(&path)
                    .await
                    .map_err(checkpoint_target_error)?,
            )?;
            validate_publication_matches_head(&publication, &head)
                .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()))?;
            if path == target_path {
                if publication.generation != coordinate.generation
                    || publication.checksum != coordinate.publication_checksum
                {
                    return Err(crate::CheckpointIntegrityError::new(
                        "publication reference does not match immutable publication evidence",
                    )
                    .into());
                }
                let mut tables = Vec::with_capacity(head.tables.len());
                for reference in head.tables.values() {
                    tables.push(self.capture_checkpoint_table(reference).await?);
                }
                tables.sort_by(|left, right| left.form_id.cmp(&right.form_id));
                selected = Some(SpaceCheckpoint::new(
                    self.space_id,
                    head.generation,
                    head.checksum,
                    path.clone(),
                    publication.checksum.clone(),
                    head.form_registry_generation,
                    tables,
                ));
            }

            let (previous_generation, previous_path, previous_checksum) = match (
                publication.previous_generation,
                publication.previous_publication,
                publication.previous_head_checksum,
            ) {
                (None, None, None) if publication.generation == 0 => {
                    break;
                }
                (Some(generation), Some(path), Some(checksum))
                    if generation + 1 == publication.generation =>
                {
                    (generation, path, checksum)
                }
                _ => {
                    return Err(crate::CheckpointIntegrityError::new(
                        "Catalog publication chain is incomplete or corrupt",
                    )
                    .into());
                }
            };
            let previous = decode_publication(
                &self
                    .store
                    .read_publication(&previous_path)
                    .await
                    .map_err(checkpoint_target_error)?,
            )?;
            if previous.generation != previous_generation
                || previous.next_head_checksum != previous_checksum
            {
                return Err(crate::CheckpointIntegrityError::new(
                    "Catalog publication predecessor is corrupt",
                )
                .into());
            }
            head = previous.next_head;
            path = previous_path;
        }
        selected.ok_or_else(|| crate::CheckpointUnavailable::new("unreachable publication").into())
    }

    fn publication_path(&self, uri: &SpaceUri) -> anyhow::Result<String> {
        let key = uri.key().as_str();
        if !key.starts_with("_ugoite/catalog/publications/") {
            return Err(crate::CheckpointIntegrityError::new(
                "publication URI is outside the Catalog publication prefix",
            )
            .into());
        }
        let prefix = if self.store.space_root().is_empty() {
            String::new()
        } else {
            format!("{}/", self.store.space_root())
        };
        Ok(format!("{prefix}{key}"))
    }

    pub(crate) fn publication_ref_for_checkpoint(
        &self,
        checkpoint: &SpaceCheckpoint,
    ) -> anyhow::Result<PublicationRef> {
        let publication_uri = self
            .publication_uri(&checkpoint.publication_location)
            .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()))?;
        PublicationRef::new(
            checkpoint.catalog_generation,
            publication_uri,
            checkpoint.publication_checksum.clone(),
        )
        .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()).into())
    }

    pub async fn create_pin(
        &self,
        name: &str,
        created_by_principal_id: &str,
        created_at_micros: i64,
        command_id: &str,
    ) -> Result<PinEntry> {
        validate_pin_name(name)?;
        if created_by_principal_id.trim().is_empty() {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "pin creator must not be empty",
            ));
        }
        let command_digest =
            checksum(&serde_json::to_vec(&(name, created_by_principal_id)).map_err(json_error)?);
        let publication =
            PublicationContext::with_command_digest(command_id, "pin.create", command_digest);
        publication.validate()?;
        self.ensure_authoritative_mutation_contract()
            .map_err(storage_error)?;
        if self.publication_outcome(&publication).await?.is_some() {
            return self.active_pin(name).await;
        }
        self.claim_mutation()?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        if self.publication_outcome(&publication).await?.is_some() {
            return self.active_pin(name).await;
        }
        let (head, exact) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        self.validate_pin_references(&head).await?;
        if head.pins.contains_key(name) {
            if self.publication_outcome(&publication).await?.is_some() {
                return self.active_pin(name).await;
            }
            return Err(Error::new(ErrorKind::DataInvalid, "pin already exists"));
        }
        let pin = PinEntry {
            coordinate: self.publication_ref_for_head(&head).await?,
            created_at_micros,
            created_by_principal_id: created_by_principal_id.to_owned(),
        };
        pin.validate()
            .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))?;
        let mut next = head.next_generation();
        next.pins.insert(name.to_owned(), pin.clone());
        let attempt = PublicationAttempt::from_exact(&publication, Some((head, exact)));
        let result = self
            .publish_new_head(
                &attempt,
                next,
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: self.namespace.as_ref().clone(),
                        table: "_ugoite_pins".to_owned(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: format!("pin://{name}"),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await;
        match result {
            Ok(()) => Ok(pin),
            Err(_error) if self.resolve_unknown_outcome(&attempt).await? => Ok(pin),
            Err(error) => Err(error),
        }
    }

    pub async fn delete_pin(&self, name: &str, command_id: &str) -> Result<()> {
        validate_pin_name(name)?;
        let command_digest = checksum(&serde_json::to_vec(&name).map_err(json_error)?);
        let publication =
            PublicationContext::with_command_digest(command_id, "pin.delete", command_digest);
        publication.validate()?;
        self.ensure_authoritative_mutation_contract()
            .map_err(storage_error)?;
        if self.publication_outcome(&publication).await?.is_some() {
            return Ok(());
        }
        self.claim_mutation()?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        if self.publication_outcome(&publication).await?.is_some() {
            return Ok(());
        }
        let (head, exact) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        self.validate_pin_references(&head).await?;
        if !head.pins.contains_key(name) {
            if self.publication_outcome(&publication).await?.is_some() {
                return Ok(());
            }
            return Err(Error::new(ErrorKind::DataInvalid, "pin not found"));
        }
        let mut next = head.next_generation();
        next.pins.remove(name);
        let attempt = PublicationAttempt::from_exact(&publication, Some((head, exact)));
        let result = self
            .publish_new_head(
                &attempt,
                next,
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: self.namespace.as_ref().clone(),
                        table: "_ugoite_pins".to_owned(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: format!("pin://{name}"),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await;
        match result {
            Ok(()) => Ok(()),
            Err(_error) if self.resolve_unknown_outcome(&attempt).await? => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn active_pin(&self, name: &str) -> Result<PinEntry> {
        let (head, _) = self
            .exact_head()
            .await?
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        head.pins.get(name).cloned().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "replayed pin publication is no longer active",
            )
        })
    }

    /// Captures the one exact Catalog Head currently visible to this reader.
    /// This is a read-only operation: it neither claims a mutation nor takes a
    /// writer serializer or lease.
    pub(crate) async fn capture_checkpoint(&self) -> anyhow::Result<SpaceCheckpoint> {
        let (head, _) = self
            .exact_head()
            .await?
            .ok_or_else(|| crate::CheckpointUnavailable::new("Catalog Head"))?;
        let publication_location = head.publication_location.clone().ok_or_else(|| {
            crate::CheckpointIntegrityError::new("Catalog Head has no publication location")
        })?;
        let publication = decode_publication(
            &self
                .store
                .read_publication(&publication_location)
                .await
                .map_err(checkpoint_target_error)?,
        )?;
        validate_publication_matches_head(&publication, &head)?;

        let mut tables = Vec::with_capacity(head.tables.len());
        for reference in head.tables.values() {
            tables.push(self.capture_checkpoint_table(reference).await?);
        }
        tables.sort_by(|left, right| left.form_id.cmp(&right.form_id));

        Ok(SpaceCheckpoint::new(
            self.space_id,
            head.generation,
            head.checksum,
            publication_location,
            publication.checksum,
            head.form_registry_generation,
            tables,
        ))
    }

    /// Reads only the authoritative Head, its reachable metadata, and named
    /// checkpoint targets. It deliberately never lists storage or scans table
    /// rows: neither can establish Catalog authority or orphan evidence.
    pub(crate) async fn health_report(
        &self,
        checkpoint_names: &[String],
    ) -> anyhow::Result<SpaceHealthReport> {
        // Do not use `exact_head` here: normal opens deliberately collapse
        // integrity errors into one failure, while doctor must retain their
        // stable classifications. This reads the same exact authoritative
        // bytes and never reconstructs Catalog state from a listing.
        let exact = match self.store.read_exact_head().await {
            Ok(Some(exact)) => exact,
            Ok(None) => {
                return Ok(self.unavailable_head_health(checkpoint_names, "catalog_head_missing"))
            }
            Err(_) => {
                return Ok(self.unavailable_head_health(checkpoint_names, "catalog_head_unreadable"))
            }
        };
        let head = match decode_head_for_health(&exact.bytes) {
            Ok(head) => head,
            Err(code) => return Ok(self.unavailable_head_health(checkpoint_names, code)),
        };
        if head.space_id != self.space_id.to_string() || head.namespace != *self.namespace.as_ref()
        {
            return Ok(
                self.unavailable_head_health(checkpoint_names, "catalog_head_identity_mismatch")
            );
        }
        let pin_issue = validate_head_pins(&head, self.logical_space_uid).err();
        let publication_issue = self
            .validate_publication_chain_for_health(&head)
            .await
            .err();
        let mut tables = Vec::with_capacity(head.tables.len());
        for reference in head.tables.values() {
            tables.push(self.table_health(reference).await);
        }
        tables.sort_by(|left, right| {
            left.identifier
                .namespace
                .cmp(&right.identifier.namespace)
                .then(left.identifier.table.cmp(&right.identifier.table))
        });

        let mut checkpoints = Vec::with_capacity(checkpoint_names.len());
        for name in checkpoint_names {
            checkpoints.push(self.checkpoint_health(name).await);
        }

        let status = if pin_issue.is_some()
            || publication_issue.is_some()
            || tables
                .iter()
                .any(|table| table.status == HealthStatus::Degraded)
            || checkpoints
                .iter()
                .any(|checkpoint| checkpoint.status == HealthStatus::Degraded)
        {
            HealthStatus::Degraded
        } else {
            HealthStatus::Healthy
        };
        Ok(SpaceHealthReport {
            status,
            catalog_head: CatalogHeadHealth {
                readable: true,
                checksum: Some(head.checksum),
                etag: exact.etag,
                generation: Some(head.generation),
                form_registry_generation: Some(head.form_registry_generation),
                issue: pin_issue
                    .map(|_| health_issue("catalog_head_pin_invalid", "catalog_head"))
                    .or_else(|| publication_issue.map(|code| health_issue(code, "publication"))),
            },
            tables,
            checkpoints,
            backend: self.backend_health(),
            unreachable_failed_attempts: Vec::new(),
            unavailable_capabilities: self.unavailable_capabilities(),
            recommendations: vec![
                "Enable object versioning or maintain operator backups for the Catalog Head.",
            ],
        })
    }

    fn unavailable_head_health(
        &self,
        checkpoint_names: &[String],
        code: &'static str,
    ) -> SpaceHealthReport {
        SpaceHealthReport {
            status: HealthStatus::Degraded,
            catalog_head: CatalogHeadHealth {
                readable: false,
                checksum: None,
                etag: None,
                generation: None,
                form_registry_generation: None,
                issue: Some(health_issue(code, "catalog_head")),
            },
            tables: Vec::new(),
            checkpoints: checkpoint_names
                .iter()
                .map(|name| CheckpointHealth {
                    name: name.clone(),
                    status: HealthStatus::Degraded,
                    issue: Some(health_issue("catalog_head_unavailable", "catalog_head")),
                })
                .collect(),
            backend: self.backend_health(),
            unreachable_failed_attempts: Vec::new(),
            unavailable_capabilities: self.unavailable_capabilities(),
            recommendations: vec![
                "Restore the Catalog Head from object versioning or an operator backup before taking action.",
            ],
        }
    }

    fn backend_health(&self) -> BackendHealth {
        let capability = self.store.backend_capabilities();
        let mode = match self.store.write_mode() {
            CatalogWriteMode::SingleProcess => BackendMode::SingleProcess,
            CatalogWriteMode::SharedReadOnly => BackendMode::SharedReadOnly,
            CatalogWriteMode::SharedVerified => BackendMode::SharedVerified,
        };
        BackendHealth {
            mode,
            etag: capability.etag,
            read_with_if_match: capability.read_with_if_match,
            write_with_if_match: capability.write_with_if_match,
            write_with_if_not_exists: capability.write_with_if_not_exists,
            // Capability bits describe what the provider advertises. The
            // health contract reports mutation admission, which is only true
            // after the behavioral probe has promoted a shared store.
            shared_write_contract: capability.shared_write_contract
                && self.store.write_mode().is_verified(),
            probe_status: match mode {
                BackendMode::SharedVerified => BackendProbeStatus::ActiveProbeVerified,
                BackendMode::SharedReadOnly => BackendProbeStatus::ActiveProbeUnavailable,
                // No durable per-store probe history exists for the
                // single-process contract, and health intentionally does not
                // write one merely to answer this request.
                BackendMode::SingleProcess => BackendProbeStatus::CapabilityDeclaration,
            },
        }
    }

    fn unavailable_capabilities(&self) -> Vec<UnavailableCapability> {
        vec![
            UnavailableCapability {
                capability: "orphan_discovery",
                reason: "object_listing_is_not_catalog_evidence",
            },
            UnavailableCapability {
                capability: "failed_attempt_candidates",
                reason: "no_durable_failed_attempt_coordinates",
            },
        ]
    }

    /// Doctor-only traversal of every immutable publication reachable from
    /// the exact Head. Normal Space opening intentionally remains constant
    /// work; this path exists solely to give an operator enough evidence to
    /// decide whether restoration is required.
    async fn validate_publication_chain_for_health(
        &self,
        head: &CatalogHead,
    ) -> std::result::Result<(), &'static str> {
        let Some(mut path) = head.publication_location.clone() else {
            return Err("publication_missing");
        };
        let mut expected_generation = head.generation;
        let mut expected_checksum = head.checksum.clone();
        let mut is_head_publication = true;
        let mut visited = BTreeSet::new();
        loop {
            if !visited.insert(path.clone()) {
                return Err("publication_chain_corrupt");
            }
            let bytes = match self.store.read_publication(&path).await {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == opendal::ErrorKind::NotFound => {
                    return Err(if is_head_publication {
                        "publication_missing"
                    } else {
                        "publication_predecessor_missing"
                    });
                }
                Err(_) => return Err("publication_unreadable"),
            };
            let publication = decode_publication_for_health(&bytes)?;
            if publication.generation != expected_generation
                || publication.next_head_checksum != expected_checksum
                || (is_head_publication && publication.next_head != *head)
                || (is_head_publication
                    && head.publication_command_id.as_deref()
                        != Some(publication.command_id.as_str()))
            {
                return Err(if is_head_publication {
                    "publication_head_mismatch"
                } else {
                    "publication_chain_corrupt"
                });
            }
            match (
                publication.previous_generation,
                publication.previous_publication,
                publication.previous_head_checksum,
            ) {
                (None, None, None) if publication.generation == 0 => return Ok(()),
                (Some(previous_generation), Some(previous_path), Some(previous_checksum))
                    if previous_generation + 1 == publication.generation =>
                {
                    path = previous_path;
                    expected_generation = previous_generation;
                    expected_checksum = previous_checksum;
                    is_head_publication = false;
                }
                _ => return Err("publication_chain_gap"),
            }
        }
    }

    async fn table_health(&self, reference: &TableReference) -> TableHealth {
        let identifier = TableIdentifierHealth {
            namespace: reference.identifier.namespace.clone(),
            table: reference.identifier.table.clone(),
        };
        let mut report = TableHealth {
            status: HealthStatus::Healthy,
            identifier,
            form_id: reference.form_id.clone(),
            table_uuid: reference.table_uuid.clone(),
            metadata_location_redacted: true,
            schema_id: None,
            snapshot_id: None,
            snapshot_count: None,
            manifest_count: None,
            manifest_size_bytes: None,
            total_record_count: None,
            total_data_file_count: None,
            total_data_file_size_bytes: None,
            file_size_distribution: None,
            issue: None,
        };
        let metadata = match TableMetadata::read_from(&self.file_io, &reference.metadata_location)
            .await
        {
            Ok(metadata) => metadata,
            Err(_) => {
                report.status = HealthStatus::Degraded;
                report.issue = Some(health_issue("table_metadata_unavailable", "table_metadata"));
                return report;
            }
        };
        if metadata.uuid().to_string() != reference.table_uuid {
            report.status = HealthStatus::Degraded;
            report.issue = Some(health_issue("table_uuid_mismatch", "table_metadata"));
            return report;
        }
        let Some(head_form_id) = reference.form_id.as_deref() else {
            report.status = HealthStatus::Degraded;
            report.issue = Some(health_issue("head_form_id_missing", "catalog_head"));
            return report;
        };
        let Ok(head_form_id) = head_form_id.parse::<Uuid>() else {
            report.status = HealthStatus::Degraded;
            report.issue = Some(health_issue("head_form_id_malformed", "catalog_head"));
            return report;
        };
        if reference.identifier.table != crate::physical_form_name(FormId::from(head_form_id)) {
            report.status = HealthStatus::Degraded;
            report.issue = Some(health_issue(
                "form_table_identifier_mismatch",
                "catalog_head",
            ));
            return report;
        }
        let Some(metadata_form_id) = metadata.properties().get(FORM_ID_PROPERTY) else {
            report.status = HealthStatus::Degraded;
            report.issue = Some(health_issue("table_form_id_missing", "table_metadata"));
            return report;
        };
        let Ok(metadata_form_id) = metadata_form_id.parse::<Uuid>() else {
            report.status = HealthStatus::Degraded;
            report.issue = Some(health_issue("table_form_id_malformed", "table_metadata"));
            return report;
        };
        if metadata_form_id != head_form_id {
            report.status = HealthStatus::Degraded;
            report.issue = Some(health_issue("form_id_mismatch", "table_metadata"));
            return report;
        }
        report.schema_id = Some(metadata.current_schema_id());
        report.snapshot_id = metadata.current_snapshot_id();
        report.snapshot_count = Some(metadata.snapshots().len());
        if let Some(snapshot) = metadata.current_snapshot() {
            let summary = &snapshot.summary().additional_properties;
            report.total_record_count = summary
                .get("total-records")
                .and_then(|value| value.parse().ok());
            report.total_data_file_count = summary
                .get("total-data-files")
                .and_then(|value| value.parse().ok());
            report.total_data_file_size_bytes = summary
                .get("total-file-size-in-bytes")
                .and_then(|value| value.parse().ok());
            let snapshot_id = snapshot.snapshot_id();
            let table = iceberg::table::Table::builder()
                .identifier(reference.identifier.to_table_ident())
                .metadata(metadata)
                .metadata_location(reference.metadata_location.clone())
                .file_io(self.file_io.clone())
                .runtime(self.runtime.clone())
                .build();
            match table {
                Ok(table) => {
                    let snapshot = table
                        .metadata()
                        .snapshot_by_id(snapshot_id)
                        .expect("current snapshot remains in its metadata");
                    match table.manifest_list_reader(snapshot).load().await {
                        Ok(manifests) => {
                            report.manifest_count = Some(manifests.entries().len());
                            report.manifest_size_bytes = Some(
                                manifests
                                    .entries()
                                    .iter()
                                    .map(|manifest| manifest.manifest_length)
                                    .sum(),
                            );
                            let mut record_count = 0_u64;
                            let mut data_file_count = 0_u64;
                            let mut data_file_size = 0_u64;
                            let mut min_file_size = None;
                            let mut max_file_size = None;
                            let mut small_file_count = 0_u64;
                            for manifest_file in manifests.entries() {
                                let manifest =
                                    match table.manifest_reader().read(manifest_file).await {
                                        Ok(manifest) => manifest,
                                        Err(_) => {
                                            report.status = HealthStatus::Degraded;
                                            report.issue = Some(health_issue(
                                                "manifest_unavailable",
                                                "manifest",
                                            ));
                                            return report;
                                        }
                                    };
                                for entry in
                                    manifest.entries().iter().filter(|entry| entry.is_alive())
                                {
                                    // Only data files describe current table data. Delete
                                    // manifests are nevertheless loaded above as integrity
                                    // evidence, but are not mixed into data-file metrics.
                                    if entry.content_type() != iceberg::spec::DataContentType::Data
                                    {
                                        continue;
                                    }
                                    let size = entry.file_size_in_bytes();
                                    data_file_count += 1;
                                    record_count += entry.record_count();
                                    data_file_size += size;
                                    min_file_size = Some(
                                        min_file_size
                                            .map_or(size, |current: u64| current.min(size)),
                                    );
                                    max_file_size = Some(
                                        max_file_size
                                            .map_or(size, |current: u64| current.max(size)),
                                    );
                                    if size < SMALL_FILE_THRESHOLD_BYTES {
                                        small_file_count += 1;
                                    }
                                }
                            }
                            report.total_record_count = Some(record_count);
                            report.total_data_file_count = Some(data_file_count);
                            report.total_data_file_size_bytes = Some(data_file_size);
                            report.file_size_distribution = Some(FileSizeDistribution {
                                min_bytes: min_file_size,
                                max_bytes: max_file_size,
                                average_bytes: (data_file_count > 0)
                                    .then_some(data_file_size / data_file_count),
                                small_file_count,
                                small_file_threshold_bytes: SMALL_FILE_THRESHOLD_BYTES,
                            });
                        }
                        Err(_) => {
                            report.status = HealthStatus::Degraded;
                            report.issue =
                                Some(health_issue("manifest_list_unavailable", "manifest_list"));
                        }
                    }
                }
                Err(_) => {
                    report.status = HealthStatus::Degraded;
                    report.issue = Some(health_issue("table_metadata_invalid", "table_metadata"));
                }
            }
        }
        report
    }

    async fn checkpoint_health(&self, name: &str) -> CheckpointHealth {
        let bytes = match self.store.read_checkpoint(name).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => {
                return checkpoint_issue(name, "checkpoint_object_missing", "checkpoint")
            }
            Err(_) => return checkpoint_issue(name, "checkpoint_unreadable", "checkpoint"),
        };
        let checkpoint: SpaceCheckpoint = match serde_json::from_slice(&bytes) {
            Ok(checkpoint) => checkpoint,
            Err(_) => return checkpoint_issue(name, "checkpoint_decode_failure", "checkpoint"),
        };
        if !checkpoint.validate_coordinate_checksum() {
            return checkpoint_issue(
                name,
                "checkpoint_coordinate_checksum_mismatch",
                "checkpoint",
            );
        }
        if checkpoint.validate_structure().is_err() {
            return checkpoint_issue(name, "checkpoint_coordinate_invalid", "checkpoint");
        }
        if checkpoint.space_id != self.space_id {
            return checkpoint_issue(name, "checkpoint_space_id_mismatch", "checkpoint");
        }
        let publication_bytes = match self
            .store
            .read_publication(&checkpoint.publication_location)
            .await
        {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => {
                return checkpoint_issue(name, "checkpoint_publication_missing", "publication")
            }
            Err(_) => {
                return checkpoint_issue(name, "checkpoint_publication_unreadable", "publication")
            }
        };
        let publication = match decode_publication_for_health(&publication_bytes) {
            Ok(publication) => publication,
            Err(code) => return checkpoint_issue(name, code, "publication"),
        };
        if publication.checksum != checkpoint.publication_checksum {
            return checkpoint_issue(
                name,
                "checkpoint_publication_checksum_mismatch",
                "publication",
            );
        }
        let head = &publication.next_head;
        if head.generation != checkpoint.catalog_generation
            || head.checksum != checkpoint.catalog_head_checksum
            || head.form_registry_generation != checkpoint.form_registry_generation
        {
            return checkpoint_issue(
                name,
                "checkpoint_catalog_generation_mismatch",
                "publication",
            );
        }
        if head.tables.len() != checkpoint.tables.len()
            || !head.tables.values().all(|reference| {
                checkpoint
                    .tables
                    .iter()
                    .any(|coordinate| checkpoint_table_matches_reference(coordinate, reference))
            })
        {
            return checkpoint_issue(name, "checkpoint_table_coordinate_missing", "checkpoint");
        }
        for coordinate in &checkpoint.tables {
            let metadata = match TableMetadata::read_from(
                &self.file_io,
                &coordinate.metadata_location,
            )
            .await
            {
                Ok(metadata) => metadata,
                Err(_) => {
                    return checkpoint_issue(name, "checkpoint_metadata_missing", "table_metadata")
                }
            };
            if metadata.uuid().to_string() != coordinate.table_uuid {
                return checkpoint_issue(name, "checkpoint_table_uuid_mismatch", "table_metadata");
            }
            if metadata.current_schema_id() != coordinate.schema_id {
                return checkpoint_issue(name, "checkpoint_schema_id_mismatch", "table_metadata");
            }
            if metadata.current_snapshot_id() != coordinate.snapshot_id {
                return checkpoint_issue(name, "checkpoint_snapshot_id_mismatch", "table_metadata");
            }
            let namespace = match NamespaceIdent::from_vec(coordinate.namespace.clone()) {
                Ok(namespace) => namespace,
                Err(_) => {
                    return checkpoint_issue(
                        name,
                        "checkpoint_table_coordinate_invalid",
                        "checkpoint",
                    )
                }
            };
            let table = match iceberg::table::Table::builder()
                .identifier(TableIdent::new(namespace, coordinate.table.clone()))
                .metadata(metadata)
                .metadata_location(coordinate.metadata_location.clone())
                .file_io(self.file_io.clone())
                .runtime(self.runtime.clone())
                .build()
            {
                Ok(table) => table,
                Err(_) => {
                    return checkpoint_issue(name, "checkpoint_metadata_invalid", "table_metadata")
                }
            };
            if let Some(snapshot_id) = coordinate.snapshot_id {
                let Some(snapshot) = table.metadata().snapshot_by_id(snapshot_id) else {
                    return checkpoint_issue(name, "checkpoint_snapshot_missing", "table_metadata");
                };
                let manifests = match table.manifest_list_reader(snapshot).load().await {
                    Ok(manifests) => manifests,
                    Err(_) => {
                        return checkpoint_issue(
                            name,
                            "checkpoint_manifest_list_missing",
                            "manifest_list",
                        )
                    }
                };
                for manifest_file in manifests.entries() {
                    let manifest = match table.manifest_reader().read(manifest_file).await {
                        Ok(manifest) => manifest,
                        Err(_) => {
                            return checkpoint_issue(
                                name,
                                "checkpoint_manifest_missing",
                                "manifest",
                            )
                        }
                    };
                    for entry in manifest.entries().iter().filter(|entry| entry.is_alive()) {
                        let file = match self.file_io.new_input(entry.file_path()) {
                            Ok(file) => file,
                            Err(_) => {
                                return checkpoint_issue(
                                    name,
                                    "checkpoint_data_file_unreadable",
                                    "data_file",
                                )
                            }
                        };
                        match file.exists().await {
                            Ok(true) => {}
                            Ok(false) => {
                                return checkpoint_issue(
                                    name,
                                    "checkpoint_data_file_missing",
                                    "data_file",
                                )
                            }
                            Err(_) => {
                                return checkpoint_issue(
                                    name,
                                    "checkpoint_data_file_unreadable",
                                    "data_file",
                                )
                            }
                        }
                    }
                }
            }
        }
        CheckpointHealth {
            name: name.to_string(),
            status: HealthStatus::Healthy,
            issue: None,
        }
    }

    async fn capture_checkpoint_table(
        &self,
        reference: &TableReference,
    ) -> anyhow::Result<CheckpointTable> {
        let form_id = reference
            .form_id
            .as_deref()
            .ok_or_else(|| {
                crate::CheckpointIntegrityError::new("Catalog Head table has no Form ID")
            })?
            .parse::<Uuid>()
            .map(FormId::from)
            .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()))?;
        let metadata = TableMetadata::read_from(&self.file_io, &reference.metadata_location)
            .await
            .map_err(checkpoint_metadata_error)?;
        validate_logical_location(
            metadata.location(),
            self.logical_space_uid,
            "Iceberg table location",
        )
        .map_err(checkpoint_metadata_error)?;
        if metadata.uuid().to_string() != reference.table_uuid {
            return Err(crate::CheckpointIntegrityError::new(
                "Iceberg table UUID does not match the Catalog Head",
            )
            .into());
        }
        Ok(CheckpointTable {
            form_id,
            namespace: reference.identifier.namespace.clone(),
            table: reference.identifier.table.clone(),
            table_uuid: reference.table_uuid.clone(),
            metadata_location: reference.metadata_location.clone(),
            snapshot_id: metadata.current_snapshot_id(),
            schema_id: metadata.current_schema_id(),
        })
    }

    /// Resolves a table only from the immutable coordinates recorded in a
    /// checkpoint. It deliberately does not reread the mutable Catalog Head.
    pub(crate) async fn load_checkpoint_table(
        &self,
        checkpoint: &SpaceCheckpoint,
        coordinate: &CheckpointTable,
    ) -> anyhow::Result<iceberg::table::Table> {
        self.validate_checkpoint_evidence(checkpoint).await?;
        if !checkpoint.tables.contains(coordinate) {
            return Err(crate::CheckpointIntegrityError::new(
                "checkpoint table coordinate is not part of this checkpoint",
            )
            .into());
        }
        let namespace =
            NamespaceIdent::from_vec(coordinate.namespace.clone()).map_err(|error| {
                crate::CheckpointIntegrityError::new(format!("invalid table namespace: {error}"))
            })?;
        let identifier = TableIdent::new(namespace, coordinate.table.clone());
        let metadata = TableMetadata::read_from(&self.file_io, &coordinate.metadata_location)
            .await
            .map_err(checkpoint_metadata_error)?;
        validate_logical_location(
            metadata.location(),
            self.logical_space_uid,
            "Iceberg table location",
        )
        .map_err(checkpoint_metadata_error)?;
        if metadata.uuid().to_string() != coordinate.table_uuid {
            return Err(crate::CheckpointIntegrityError::new(
                "Iceberg table UUID does not match the checkpoint",
            )
            .into());
        }
        if metadata.current_schema_id() != coordinate.schema_id {
            return Err(crate::CheckpointIntegrityError::new(
                "Iceberg schema ID does not match the checkpoint",
            )
            .into());
        }
        if metadata.current_snapshot_id() != coordinate.snapshot_id {
            return Err(crate::CheckpointIntegrityError::new(
                "Iceberg snapshot ID does not match the checkpoint",
            )
            .into());
        }
        Ok(iceberg::table::Table::builder()
            .identifier(identifier)
            .metadata(metadata)
            .metadata_location(coordinate.metadata_location.clone())
            .file_io(self.file_io.clone())
            .runtime(self.runtime.clone())
            .build()?)
    }

    /// Re-establishes the immutable publication -> canonical Head chain that
    /// authorizes checkpoint coordinates. The coordinate checksum detects
    /// corruption, but only this evidence prevents a rewritten checkpoint
    /// from selecting arbitrary metadata.
    pub(crate) async fn validate_checkpoint_evidence(
        &self,
        checkpoint: &SpaceCheckpoint,
    ) -> anyhow::Result<()> {
        let publication_bytes = self
            .store
            .read_publication(&checkpoint.publication_location)
            .await
            .map_err(checkpoint_target_error)?;
        let publication = decode_publication(&publication_bytes)
            .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()))?;
        if publication.checksum != checkpoint.publication_checksum {
            return Err(crate::CheckpointIntegrityError::new(
                "publication checksum does not match the checkpoint",
            )
            .into());
        }
        let head = &publication.next_head;
        if head.checksum != checkpoint.catalog_head_checksum
            || head.generation != checkpoint.catalog_generation
            || head.form_registry_generation != checkpoint.form_registry_generation
            || head.space_id != self.space_id.to_string()
            || head.namespace != *self.namespace.as_ref()
            || head.publication_location.as_deref()
                != Some(checkpoint.publication_location.as_str())
        {
            return Err(crate::CheckpointIntegrityError::new(
                "canonical Catalog Head does not match the checkpoint",
            )
            .into());
        }
        validate_publication_matches_head(&publication, head)
            .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()))?;
        if head.tables.len() != checkpoint.tables.len()
            || !head.tables.values().all(|reference| {
                checkpoint
                    .tables
                    .iter()
                    .any(|table| checkpoint_table_matches_reference(table, reference))
            })
        {
            return Err(crate::CheckpointIntegrityError::new(
                "checkpoint tables do not exactly match the canonical Catalog Head",
            )
            .into());
        }
        Ok(())
    }

    /// Resolves a command only from immutable publications reachable from
    /// the authoritative Catalog Head. The Head is the retry/idempotency
    /// boundary; no exact-key receipt or other durable command state exists.
    pub(crate) async fn publication_outcome(
        &self,
        publication: &PublicationContext,
    ) -> Result<Option<PublicationOutcome>> {
        let Some((head, _)) = self.exact_head().await? else {
            return Ok(None);
        };
        self.find_publication_from_head(head, publication)
            .await?
            .map(|record| {
                Ok(PublicationOutcome {
                    command_id: record.command_id,
                    catalog_generation: record.generation,
                    snapshot_id: record.new_snapshot_id,
                })
            })
            .transpose()
    }

    /// Resumes a publication object left behind before its Head CAS. The
    /// object is considered only when its deterministic path and immutable
    /// contents match this exact attempt; reachability from Head remains the
    /// authority for all ordinary reads.
    pub(crate) async fn recover_existing_publication(&self) -> Result<Option<PublicationOutcome>> {
        let attempt = self.publication_attempt().await?;
        let generation = attempt
            .expected_generation
            .map_or(0, |generation| generation + 1);
        let publication_path = self
            .store
            .publication_path(generation, &attempt.publication.command_id);
        let publication = match self.store.read_publication(&publication_path).await {
            Ok(bytes) => decode_publication(&bytes)?,
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        self.validate_publication_for_attempt(&attempt, &publication, &publication_path)?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        self.adopt_existing_publication_for_attempt(
            &attempt,
            &publication_path,
            publication.clone(),
        )
        .await?;
        Ok(Some(PublicationOutcome {
            command_id: publication.command_id,
            catalog_generation: publication.generation,
            snapshot_id: publication.new_snapshot_id,
        }))
    }

    async fn find_publication_from_head(
        &self,
        mut head: CatalogHead,
        publication: &PublicationContext,
    ) -> Result<Option<PublicationRecord>> {
        let mut path = head.publication_location.clone().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head has no publication record while resolving a command outcome",
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
                return Ok(Some(record));
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

    /// Validates that an immutable publication describes exactly the
    /// attempt captured by this catalog and cannot be adopted for another
    /// base Head.
    fn validate_publication_for_attempt(
        &self,
        attempt: &PublicationAttempt,
        publication: &PublicationRecord,
        publication_path: &str,
    ) -> Result<()> {
        if publication.command_id != attempt.publication.command_id
            || publication.command_kind != attempt.publication.command_kind
            || publication.command_digest != attempt.publication.command_digest
            || publication.previous_generation != attempt.expected_generation
            || publication.previous_publication != attempt.expected_previous_publication
            || publication.previous_head_checksum != attempt.expected_head_checksum
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "immutable publication does not match the command attempt",
            ));
        }
        let expected_generation = attempt
            .expected_generation
            .map_or(0, |generation| generation.saturating_add(1));
        if publication.generation != expected_generation
            || publication.next_head.generation != publication.generation
            || publication.next_head_checksum != publication.next_head.checksum
            || head_checksum(&publication.next_head)? != publication.next_head_checksum
            || publication.next_head.space_id != self.space_id.to_string()
            || publication.next_head.namespace != *self.namespace.as_ref()
            || publication.next_head.publication_location.as_deref() != Some(publication_path)
            || publication.next_head.publication_command_id.as_deref()
                != Some(attempt.publication.command_id.as_str())
            || self
                .store
                .publication_path(publication.generation, &publication.command_id)
                != publication_path
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "immutable publication does not describe the attempt's next Head",
            ));
        }
        validate_publication_matches_head(publication, &publication.next_head)
    }

    /// Adopts a matching immutable publication without rerunning Iceberg's
    /// data/manifest/metadata writer. The exact current Head is reread under
    /// the single-process serializer where needed, then the embedded next
    /// Head is published with the same conditional boundary as a fresh write.
    async fn adopt_existing_publication_head(
        &self,
        attempt: &PublicationAttempt,
        publication_path: &str,
        publication: &PublicationRecord,
        observed_head: &CatalogHead,
        observed_exact: &ExactCatalogHead,
    ) -> Result<()> {
        if attempt.expected_head.as_ref() != Some(observed_head) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while adopting an immutable publication",
            ));
        }
        self.validate_publication_for_attempt(attempt, publication, publication_path)?;
        // All production callers enter this helper from a mutation path that
        // already owns the single-process serializer. Shared backends use the
        // conditional Head replacement below as the concurrency boundary.
        let Some((head, exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head disappeared while adopting an immutable publication",
            ));
        };
        if !exact_head_matches(observed_head, observed_exact.etag.as_deref(), &head, &exact) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while adopting an immutable publication",
            ));
        }
        self.validate_publication_for_attempt(attempt, publication, publication_path)?;
        let bytes = encode_head(&publication.next_head)?;
        if bytes.len() > MAX_HEAD_BYTES {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head exceeds its 1 MiB safety limit",
            ));
        }
        let permit = self.mutation_permit().map_err(storage_error)?;
        match self
            .store
            .replace_head(&permit, exact.etag.as_deref(), bytes)
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if is_condition_conflict(&error) => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while adopting an immutable publication",
            )),
            Err(error) => Err(storage_error(error)),
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
        validate_logical_location(
            metadata.location(),
            self.logical_space_uid,
            "Iceberg table location",
        )?;
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
        let attempt = self.publication_attempt().await?;
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
            .try_with_new_metadata(&metadata)?
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
            Ok(()) => self.load_live_table(table).await,
            Err(_error) if self.resolve_unknown_outcome(&attempt).await? => {
                self.load_live_table(table).await
            }
            Err(error) => Err(error),
        }
    }

    /// Asset visibility is derived from the authoritative Head's reachable
    /// publication chain. Physical bytes are intentionally retained in v1;
    /// this method never consults or writes a lifecycle sidecar.
    pub(crate) async fn asset_is_deleted(&self, asset_id: &str) -> Result<bool> {
        validate_asset_id(asset_id)
            .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))?;
        let Some((mut head, _)) = self.exact_head().await? else {
            return Ok(false);
        };
        let Some(mut path) = head.publication_location.clone() else {
            return Ok(false);
        };
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
            if is_asset_delete_publication(&publication, asset_id) {
                return Ok(true);
            }

            let (previous_generation, previous_path, previous_checksum) = match (
                publication.previous_generation,
                publication.previous_publication,
                publication.previous_head_checksum,
            ) {
                (None, None, None) if publication.generation == 0 => return Ok(false),
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

    /// Publishes an asset deletion as an immutable publication whose
    /// reachability from Catalog Head is the only durable visibility marker.
    pub(crate) async fn mark_asset_deleted(&self, asset_id: &str) -> Result<()> {
        validate_asset_id(asset_id)
            .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))?;
        self.claim_mutation()?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let attempt = self.publication_attempt().await?;
        let head = attempt
            .expected_head
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        if self.asset_is_deleted(asset_id).await? {
            return Ok(());
        }

        let publication = self
            .publish_asset_deletion(asset_id, &attempt, head.next_generation())
            .await;
        match publication {
            Ok(()) => Ok(()),
            Err(_error) if self.resolve_unknown_outcome(&attempt).await? => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn publish_asset_deletion(
        &self,
        asset_id: &str,
        attempt: &PublicationAttempt,
        next: CatalogHead,
    ) -> Result<()> {
        let head = attempt
            .expected_head
            .as_ref()
            .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
        self.publish_new_head(
            attempt,
            next,
            PublicationUpdate {
                affected_table: TableCoordinates {
                    namespace: head.namespace.clone(),
                    table: format!("_asset_delete_{asset_id}"),
                },
                base_metadata_location: None,
                new_metadata_location: format!("asset://deleted/{asset_id}"),
                base_snapshot_id: None,
                base_schema_id: None,
                new_snapshot_id: None,
                new_schema_id: 0,
            },
        )
        .await
    }

    async fn write_publication(&self, publication: &PublicationRecord) -> Result<String> {
        let path = self
            .store
            .publication_path(publication.generation, &publication.command_id);
        let bytes = encode_publication(publication)?;
        let permit = self.mutation_permit().map_err(storage_error)?;
        match self
            .store
            .create_publication(&permit, &path, bytes.clone())
            .await
        {
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

    async fn adopt_existing_publication_for_attempt(
        &self,
        attempt: &PublicationAttempt,
        publication_path: &str,
        publication: PublicationRecord,
    ) -> Result<()> {
        match (attempt.expected_head.as_ref(), self.exact_head().await?) {
            (Some(expected), Some((head, exact))) => {
                if expected != &head || attempt.expected_head_etag != exact.etag {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog Head changed while adopting an immutable publication",
                    ));
                }
                self.adopt_existing_publication_head(
                    attempt,
                    publication_path,
                    &publication,
                    &head,
                    &exact,
                )
                .await
            }
            (None, None) => {
                let bytes = encode_head(&publication.next_head)?;
                if bytes.len() > MAX_HEAD_BYTES {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog Head exceeds its 1 MiB safety limit",
                    ));
                }
                let permit = self.mutation_permit().map_err(storage_error)?;
                match self.store.create_head(&permit, bytes).await {
                    Ok(()) => Ok(()),
                    Err(error) if is_condition_conflict(&error) => Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog Head changed while adopting an immutable publication",
                    )),
                    Err(error) => Err(storage_error(error)),
                }
            }
            _ => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while adopting an immutable publication",
            )),
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
        next.publication_location = Some(publication_path.clone());
        next.publication_command_id = Some(attempt.publication.command_id.clone());
        next.checksum = head_checksum(&next)?;
        if attempt.expected_head.is_some() {
            match self.store.read_publication(&publication_path).await {
                Ok(bytes) => {
                    let publication = decode_publication(&bytes)?;
                    self.adopt_existing_publication_for_attempt(
                        attempt,
                        &publication_path,
                        publication,
                    )
                    .await?;
                    return Ok(());
                }
                Err(error) if error.kind() == opendal::ErrorKind::NotFound => {}
                Err(error) => return Err(storage_error(error)),
            }
        }
        let publication = PublicationRecord {
            generation: next.generation,
            previous_generation,
            previous_publication,
            previous_head_checksum,
            command_id: attempt.publication.command_id.clone(),
            command_kind: attempt.publication.command_kind.clone(),
            command_digest: attempt.publication.command_digest.clone(),
            change: attempt.publication.change.clone(),
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
        #[cfg(debug_assertions)]
        if let Some(gate) = &self.publication_gate {
            // Test-only crash point: the immutable publication is durable,
            // while the authoritative Head still proves the exact base.
            gate.pause().await;
        }
        let bytes = encode_head(&next)?;
        if bytes.len() > MAX_HEAD_BYTES {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head exceeds its 1 MiB safety limit",
            ));
        }
        if self.store.write_mode() == CatalogWriteMode::SingleProcess
            && attempt.expected_head.is_some()
        {
            // Single-process OpenDAL backends do not expose an ETag CAS. The
            // serializer above still gives us a safe publication boundary;
            // compare the exact captured Head while holding that serializer
            // before replacing it.
            let current = self.exact_head().await?.map(|(head, _)| head);
            if current != attempt.expected_head {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog Head changed before this publication could be committed",
                ));
            }
        }
        let permit = self.mutation_permit().map_err(storage_error)?;
        let result = if attempt.expected_head.is_some() {
            self.store
                .replace_head(&permit, attempt.expected_head_etag.as_deref(), bytes)
                .await
        } else {
            self.store.create_head(&permit, bytes).await
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
        let Some((head, _)) = self.exact_head().await? else {
            return if attempt.expected_head.is_none() {
                Ok(false)
            } else {
                Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog Head disappeared while resolving an unknown publication outcome",
                ))
            };
        };
        self.resolve_publication_from_head(head, attempt).await
    }

    async fn resolve_publication_from_head(
        &self,
        mut head: CatalogHead,
        attempt: &PublicationAttempt,
    ) -> Result<bool> {
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
        self.publication_ref_for_record(publication_path, &publication)?;
        for reference in head.tables.values() {
            validate_logical_location(
                &reference.metadata_location,
                self.logical_space_uid,
                "Catalog Head table metadata location",
            )?;
        }
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

/// Builds the official Iceberg FileIO used by a relation-local derived
/// catalog.  Derived materializations use their Relation Head for visibility;
/// this helper only shares the operator-backed Iceberg storage mechanics.
pub(crate) fn file_io_for_store(store: &SpaceCatalogStore, space_id: SpaceId) -> FileIO {
    FileIOBuilder::new(Arc::new(LogicalStorageFactory::new(
        store.operator().clone(),
        store.space_root(),
        logical_space_uid(space_id),
    )))
    .build()
}

fn checkpoint_table_matches_reference(
    coordinate: &CheckpointTable,
    reference: &TableReference,
) -> bool {
    reference.form_id.as_deref() == Some(coordinate.form_id.to_string().as_str())
        && reference.identifier.namespace == coordinate.namespace
        && reference.identifier.table == coordinate.table
        && reference.table_uuid == coordinate.table_uuid
        && reference.metadata_location == coordinate.metadata_location
}

fn checkpoint_target_error(error: opendal::Error) -> anyhow::Error {
    if error.kind() == opendal::ErrorKind::NotFound || error.to_string().contains("NotFound") {
        crate::CheckpointUnavailable::new(error.to_string()).into()
    } else {
        anyhow::Error::new(error).context("read checkpoint target")
    }
}

fn checkpoint_metadata_error(error: iceberg::Error) -> anyhow::Error {
    if error_chain_contains_not_found(&error) {
        crate::CheckpointUnavailable::new("checkpoint Iceberg metadata").into()
    } else if error.kind() == ErrorKind::DataInvalid {
        crate::CheckpointIntegrityError::new(error.to_string()).into()
    } else {
        anyhow::Error::new(error).context("read checkpoint Iceberg metadata")
    }
}

/// Iceberg wraps its OpenDAL I/O errors in an `anyhow::Error`, so inspecting
/// only Iceberg's top-level `ErrorKind` would turn a missing immutable object
/// into an indistinguishable operational failure. Keep this check at the
/// checkpoint boundary: missing targets have a stable public meaning, while
/// corrupt metadata and transient I/O retain their distinct classifications.
pub(crate) fn error_chain_contains_not_found(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        if source
            .downcast_ref::<opendal::Error>()
            .is_some_and(|error| error.kind() == opendal::ErrorKind::NotFound)
        {
            return true;
        }
        current = source.source();
    }
    false
}

impl SpaceCatalog {
    /// Returns at most `max_tables` identifiers without first allocating an
    /// unbounded identifier vector. Rebuild and authorization paths use this
    /// as their catalog-size guard before loading table metadata.
    pub(crate) async fn list_tables_bounded(
        &self,
        namespace: &NamespaceIdent,
        max_tables: usize,
    ) -> Result<Vec<TableIdent>> {
        if namespace != &self.namespace {
            return Ok(Vec::new());
        }
        let head = if let Some(attempt) = &self.bound_attempt {
            attempt.expected_head.clone()
        } else {
            self.exact_head().await?.map(|(head, _)| head)
        };
        let Some(head) = head else {
            return Ok(Vec::new());
        };
        let mut identifiers = Vec::new();
        for reference in head.tables.values() {
            if identifiers.len() >= max_tables {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "catalog table count exceeds its configured limit",
                ));
            }
            identifiers.push(reference.identifier.to_table_ident());
        }
        Ok(identifiers)
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
        self.list_tables_bounded(namespace, usize::MAX).await
    }

    async fn create_table(
        &self,
        namespace: &NamespaceIdent,
        creation: TableCreation,
    ) -> Result<iceberg::table::Table> {
        self.ensure_authoritative_mutation_contract()
            .map_err(storage_error)?;
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
        let attempt = self.publication_attempt().await?;
        if let Some(head) = &attempt.expected_head {
            if head.tables.contains_key(&Self::table_key(&table)) {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    format!("table already exists: {table}"),
                ));
            }
        }
        if creation.location.is_none() {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Ugoite requires an explicit Iceberg table location",
            ));
        }
        // Iceberg Rust's public table-creation builder intentionally assigns
        // fresh field IDs. Ugoite's Form IDs are already stable Iceberg IDs,
        // so preserve that schema in the resulting standard Iceberg metadata
        // rather than maintaining a second mapping document.
        let requested_schema = creation.schema.clone();
        let mut metadata_builder = TableMetadataBuilder::from_table_creation(creation)?;
        if requested_schema.calc_min_compatible_format() == FormatVersion::V3 {
            metadata_builder = metadata_builder.upgrade_format_version(FormatVersion::V3)?;
        }
        let metadata =
            preserve_schema_field_ids(metadata_builder.build()?.metadata, requested_schema)?;
        let metadata_location = MetadataLocation::try_new_with_metadata(&metadata)?;
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
            TableReference::from_table(&created, self.logical_space_uid)?,
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
                self.load_live_table(&table).await
            }
            Err(error) => Err(error),
        }
    }

    async fn load_table(&self, table: &TableIdent) -> Result<iceberg::table::Table> {
        if let Some(attempt) = &self.bound_attempt {
            let head = attempt
                .expected_head
                .as_ref()
                .ok_or_else(|| Error::new(ErrorKind::DataInvalid, "Catalog Head is missing"))?;
            self.load_head_table(table, head).await
        } else {
            self.load_live_table(table).await
        }
    }

    async fn drop_table(&self, _table: &TableIdent) -> Result<()> {
        Err(unsupported("dropping Form tables is not exposed by Ugoite"))
    }

    async fn purge_table(&self, _table: &TableIdent) -> Result<()> {
        Err(unsupported("purging Form tables is not exposed by Ugoite"))
    }

    async fn table_exists(&self, table: &TableIdent) -> Result<bool> {
        if let Some(attempt) = &self.bound_attempt {
            return Ok(attempt
                .expected_head
                .as_ref()
                .is_some_and(|head| head.tables.contains_key(&Self::table_key(table))));
        }
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
        self.ensure_authoritative_mutation_contract()
            .map_err(storage_error)?;
        self.claim_mutation()?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let table = commit.identifier().clone();
        let attempt = self.publication_attempt().await?;
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
            TableReference::from_table(&staged, self.logical_space_uid)?,
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
                self.load_live_table(&table).await
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
    pins: BTreeMap<String, PinEntry>,
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
            pins: BTreeMap::new(),
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
    fn from_table(table: &iceberg::table::Table, space_uid: Uuid) -> Result<Self> {
        let metadata_location = table.metadata_location_result()?.to_string();
        validate_logical_location(
            &metadata_location,
            space_uid,
            "Iceberg table metadata location",
        )?;
        validate_logical_location(
            table.metadata().location(),
            space_uid,
            "Iceberg table location",
        )?;
        Ok(Self {
            identifier: TableCoordinates::from(table.identifier()),
            form_id: table.metadata().properties().get("ugoite.form.id").cloned(),
            table_uuid: table.metadata().uuid().to_string(),
            metadata_location,
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
    change: Option<ChangeDescriptor>,
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

fn validate_logical_location(location: &str, space_uid: Uuid, context: &str) -> Result<()> {
    let uri = SpaceUri::parse(location).map_err(|error| {
        Error::new(
            ErrorKind::DataInvalid,
            format!("{context} must be a Ugoite logical URI"),
        )
        .with_source(error)
    })?;
    if uri.space_uid() != space_uid {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            format!("{context} belongs to another Space"),
        ));
    }
    Ok(())
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

fn decode_head_for_health(bytes: &[u8]) -> std::result::Result<CatalogHead, &'static str> {
    let head: CatalogHead =
        serde_json::from_slice(bytes).map_err(|_| "catalog_head_decode_failure")?;
    if head.format_version != SPACE_FORMAT_VERSION {
        return Err("catalog_head_decode_failure");
    }
    if head.checksum != head_checksum(&head).map_err(|_| "catalog_head_decode_failure")? {
        return Err("catalog_head_checksum_mismatch");
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

fn decode_publication_for_health(
    bytes: &[u8],
) -> std::result::Result<PublicationRecord, &'static str> {
    let publication: PublicationRecord =
        serde_json::from_slice(bytes).map_err(|_| "publication_decode_failure")?;
    if publication.checksum
        != publication_checksum(&publication).map_err(|_| "publication_decode_failure")?
    {
        return Err("publication_checksum_mismatch");
    }
    if publication.next_head.checksum != publication.next_head_checksum {
        return Err("publication_head_mismatch");
    }
    Ok(publication)
}

fn health_issue(code: &'static str, target: &'static str) -> HealthIssue {
    HealthIssue { code, target }
}

fn checkpoint_issue(name: &str, code: &'static str, target: &'static str) -> CheckpointHealth {
    CheckpointHealth {
        name: name.to_string(),
        status: HealthStatus::Degraded,
        issue: Some(health_issue(code, target)),
    }
}

pub(crate) fn preserve_schema_field_ids(
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

fn is_asset_delete_publication(publication: &PublicationRecord, asset_id: &str) -> bool {
    publication.command_kind == "asset.delete"
        && publication.affected_table.table == format!("_asset_delete_{asset_id}")
        && publication.new_metadata_location == format!("asset://deleted/{asset_id}")
}

fn validate_head_pins(head: &CatalogHead, logical_space_uid: Uuid) -> Result<()> {
    if head.pins.len() > MAX_PIN_COUNT {
        return Err(Error::new(
            ErrorKind::DataInvalid,
            "Catalog Head contains too many publication Pins",
        ));
    }
    for (name, pin) in &head.pins {
        validate_pin_name(name)?;
        pin.validate()
            .map_err(|error| Error::new(ErrorKind::DataInvalid, error.to_string()))?;
        if pin.coordinate.generation > head.generation
            || pin.coordinate.publication_uri.space_uid() != logical_space_uid
            || !pin
                .coordinate
                .publication_uri
                .key()
                .as_str()
                .starts_with("_ugoite/catalog/publications/")
            || SpaceUri::parse(&pin.coordinate.publication_uri.to_string()).is_err()
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head contains an invalid publication Pin",
            ));
        }
    }
    Ok(())
}

fn exact_head_matches(
    observed_head: &CatalogHead,
    observed_etag: Option<&str>,
    current_head: &CatalogHead,
    current_exact: &ExactCatalogHead,
) -> bool {
    observed_head == current_head && observed_etag == current_exact.etag.as_deref()
}

fn storage_error(error: impl std::fmt::Display) -> Error {
    Error::new(ErrorKind::Unexpected, error.to_string())
}

fn pin_reference_target_error(error: opendal::Error) -> Error {
    if error.kind() == opendal::ErrorKind::NotFound {
        Error::new(ErrorKind::DataInvalid, "Pin publication target unavailable")
    } else {
        storage_error(error)
    }
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
    use ugoite_storage::operator_from_uri;

    fn logical_test_location(space_id: SpaceId, key: &str) -> String {
        crate::logical_storage::logical_uri(
            crate::logical_storage::logical_space_uid(space_id),
            key,
        )
        .expect("test logical location")
    }

    #[tokio::test]
    async fn creates_reopens_and_updates_a_table_through_head_publication() -> AnyResult<()> {
        let temp = tempdir()?;
        let operator = Operator::new(Fs::default().root(temp.path().to_string_lossy().as_ref()))?;
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
                    .location(logical_test_location(
                        SpaceId::from(Uuid::from_u128(1)),
                        "forms/form",
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
        let operator = Operator::new(Memory::default())?;
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
                    .location(logical_test_location(
                        SpaceId::from(Uuid::from_u128(2)),
                        "forms/form",
                    ))
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
        let operator = Operator::new(Memory::default())?;
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
            change: catalog.publication.change.clone(),
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
        let operator = Operator::new(Memory::default())?;
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
        let operator = Operator::new(Fs::default().root(temp.path().to_string_lossy().as_ref()))?;
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
                    .location(logical_test_location(space_id, "forms/form"))
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

    #[tokio::test]
    async fn direct_unverified_non_local_catalog_writes_fail_before_metadata_io() -> AnyResult<()> {
        let operator = operator_from_uri("s3://ugoite-test-bucket/catalog-table-boundary")?;
        let store = SpaceCatalogStore::new(operator, "spaces/catalog-table-boundary")?;
        let space_id = SpaceId::from(Uuid::from_u128(18_526));
        let catalog = SpaceCatalog::new(store, space_id)?;
        let namespace = catalog.namespace().clone();
        let error = catalog
            .create_table(
                &namespace,
                TableCreation::builder()
                    .name("form_direct_boundary".to_string())
                    .location("s3://ugoite-test-bucket/catalog-table-boundary/form".to_string())
                    .schema(
                        iceberg::spec::Schema::builder()
                            .with_fields(vec![])
                            .build()?,
                    )
                    .build(),
            )
            .await
            .expect_err("direct unverified Catalog table writes must fail closed");
        assert!(error
            .to_string()
            .contains("verified exact-read and single-Head-CAS storage contract"));
        Ok(())
    }
}
