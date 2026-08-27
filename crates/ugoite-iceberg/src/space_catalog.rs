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
use ugoite_domain::checkpoint::{CheckpointTable, SpaceCheckpoint};
use ugoite_domain::id::{validate_asset_id, FormId, SpaceId};
use ugoite_domain::space_key::SpaceUri;
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
const MAX_DELETED_ASSET_BLOBS_PER_PASS: usize = 1_024;

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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CommandReceiptState {
    Pending,
    Publishing,
    Committed,
    Stale,
}

/// Exact-key idempotency evidence for one domain command. This is the online
/// command index; immutable publications remain the audit and crash-recovery
/// record, but are never searched for an ordinary new command.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct CommandReceiptRecord {
    command_id: String,
    command_kind: String,
    command_digest: String,
    state: CommandReceiptState,
    base_generation: Option<u64>,
    base_head_checksum: Option<String>,
    base_publication: Option<String>,
    intended_publication: Option<String>,
    catalog_generation: Option<u64>,
    snapshot_id: Option<i64>,
}

impl CommandReceiptRecord {
    fn pending(attempt: &PublicationAttempt, intended_publication: String) -> Self {
        Self {
            command_id: attempt.publication.command_id.clone(),
            command_kind: attempt.publication.command_kind.clone(),
            command_digest: attempt.publication.command_digest.clone(),
            state: CommandReceiptState::Pending,
            base_generation: attempt.expected_generation,
            base_head_checksum: attempt.expected_head_checksum.clone(),
            base_publication: attempt.expected_previous_publication.clone(),
            intended_publication: Some(intended_publication),
            catalog_generation: None,
            snapshot_id: None,
        }
    }

    fn matches(&self, publication: &PublicationContext) -> bool {
        self.command_id == publication.command_id
            && self.command_kind == publication.command_kind
            && self.command_digest == publication.command_digest
    }

    fn attempt(&self) -> PublicationAttempt {
        PublicationAttempt {
            publication: PublicationContext::with_command_digest(
                self.command_id.clone(),
                self.command_kind.clone(),
                self.command_digest.clone(),
            ),
            expected_generation: self.base_generation,
            expected_head_checksum: self.base_head_checksum.clone(),
            expected_previous_publication: self.base_publication.clone(),
            expected_head: None,
            expected_head_etag: None,
        }
    }

    fn publication_receipt(&self) -> Result<PublicationReceipt> {
        Ok(PublicationReceipt {
            command_id: self.command_id.clone(),
            catalog_generation: self.catalog_generation.ok_or_else(|| {
                Error::new(
                    ErrorKind::DataInvalid,
                    "committed command receipt has no Catalog generation",
                )
            })?,
            snapshot_id: self.snapshot_id,
        })
    }
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

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AssetLifecycleState {
    Pending,
    Publishing,
    Committed,
    Stale,
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
struct AssetLifecycleMarker {
    command_id: String,
    command_kind: String,
    command_digest: String,
    state: AssetLifecycleState,
    base_generation: Option<u64>,
    base_head_checksum: Option<String>,
    base_publication: Option<String>,
    intended_publication: Option<String>,
}

impl AssetLifecycleMarker {
    fn recovery_attempt(&self) -> PublicationAttempt {
        PublicationAttempt {
            publication: PublicationContext::with_command_digest(
                self.command_id.clone(),
                self.command_kind.clone(),
                self.command_digest.clone(),
            ),
            expected_generation: self.base_generation,
            expected_head_checksum: self.base_head_checksum.clone(),
            expected_previous_publication: self.base_publication.clone(),
            expected_head: None,
            expected_head_etag: None,
        }
    }

    fn matches_base(&self, head: &CatalogHead) -> bool {
        self.base_generation == Some(head.generation)
            && self.base_head_checksum.as_deref() == Some(head.checksum.as_str())
            && self.base_publication.as_deref() == head.publication_location.as_deref()
    }
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
        self.validate_head_publication(&head).await?;
        Ok(Some((head, exact)))
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

        let status = if publication_issue.is_some()
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
                issue: publication_issue.map(|code| health_issue(code, "publication")),
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

    /// Validates every immutable Iceberg coordinate before a checkpoint is
    /// made durable or returned to a caller. Publication evidence establishes
    /// which metadata locations belong to the Head; this verifies that each
    /// saved snapshot and schema coordinate still exactly matches that
    /// metadata rather than deferring discovery until query execution.
    pub(crate) async fn validate_checkpoint_tables(
        &self,
        checkpoint: &SpaceCheckpoint,
    ) -> anyhow::Result<()> {
        for coordinate in &checkpoint.tables {
            self.validate_checkpoint_table(coordinate).await?;
        }
        Ok(())
    }

    async fn validate_checkpoint_table(&self, coordinate: &CheckpointTable) -> anyhow::Result<()> {
        let metadata = TableMetadata::read_from(&self.file_io, &coordinate.metadata_location)
            .await
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
        Ok(())
    }

    pub(crate) async fn create_checkpoint(
        &self,
        name: &str,
        checkpoint: &SpaceCheckpoint,
    ) -> anyhow::Result<()> {
        let bytes = serde_json::to_vec(checkpoint)?;
        let permit = self.mutation_permit()?;
        self.store.create_checkpoint(&permit, name, bytes).await?;
        Ok(())
    }

    pub(crate) async fn read_checkpoint(&self, name: &str) -> anyhow::Result<SpaceCheckpoint> {
        let bytes = self
            .store
            .read_checkpoint(name)
            .await
            .map_err(checkpoint_target_error)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| crate::CheckpointIntegrityError::new(error.to_string()).into())
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

    /// Reads the exact-key command receipt. Immutable publication traversal is
    /// reserved for a receipt left Pending by an ambiguous crash; a missing,
    /// stale, or committed receipt is resolved without consulting history.
    pub(crate) async fn publication_receipt(
        &self,
        publication: &PublicationContext,
    ) -> Result<Option<PublicationReceipt>> {
        let Some((bytes, etag)) = self
            .store
            .read_command_receipt(&publication.command_id)
            .await
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        let record: CommandReceiptRecord = serde_json::from_slice(&bytes).map_err(json_error)?;
        if !record.matches(publication) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command id was reused with different command content",
            ));
        }
        match record.state {
            CommandReceiptState::Committed => Ok(Some(record.publication_receipt()?)),
            CommandReceiptState::Stale => Ok(None),
            CommandReceiptState::Pending | CommandReceiptState::Publishing => {
                self.resolve_pending_command_receipt(record, etag).await
            }
        }
    }

    /// Claims the exact command key for the immutable Head captured by this
    /// attempt. A competing owner either shares the same base or forces the
    /// coordinator to retry; neither path searches publication history.
    pub(crate) async fn claim_command_receipt(&self) -> Result<()> {
        let attempt = self.publication_attempt().await?;
        let intended = self.store.publication_path(
            attempt
                .expected_head
                .as_ref()
                .map_or(0, |head| head.generation + 1),
            &attempt.publication.command_id,
        );
        let pending = CommandReceiptRecord::pending(&attempt, intended);
        let bytes = serde_json::to_vec(&pending).map_err(json_error)?;
        let permit = self.mutation_permit().map_err(storage_error)?;
        let existing = self
            .store
            .read_command_receipt(&attempt.publication.command_id)
            .await
            .map_err(storage_error)?;
        let Some((existing_bytes, etag)) = existing else {
            match self
                .store
                .create_command_receipt(&permit, &attempt.publication.command_id, bytes)
                .await
            {
                Ok(()) => return Ok(()),
                Err(error) if is_condition_conflict(&error) => {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog Head changed while claiming the command receipt",
                    ));
                }
                Err(error) => return Err(storage_error(error)),
            }
        };
        let existing: CommandReceiptRecord =
            serde_json::from_slice(&existing_bytes).map_err(json_error)?;
        if !existing.matches(&attempt.publication) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command id was reused with different command content",
            ));
        }
        match existing.state {
            CommandReceiptState::Committed => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while claiming the command receipt",
            )),
            CommandReceiptState::Publishing
                if existing.base_generation == pending.base_generation
                    && existing.base_head_checksum == pending.base_head_checksum
                    && existing.base_publication == pending.base_publication
                    && existing.intended_publication == pending.intended_publication =>
            {
                // A process restart may resume the exact same immutable
                // attempt. The command identity and every publication-base
                // coordinate are already protected by the receipt key.
                Ok(())
            }
            CommandReceiptState::Publishing => Err(Error::new(
                ErrorKind::DataInvalid,
                "command publication is still in progress",
            )),
            CommandReceiptState::Pending
                if existing.base_generation == pending.base_generation
                    && existing.base_head_checksum == pending.base_head_checksum
                    && existing.base_publication == pending.base_publication
                    && existing.intended_publication == pending.intended_publication =>
            {
                Ok(())
            }
            CommandReceiptState::Pending => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while claiming the command receipt",
            )),
            CommandReceiptState::Stale => match self
                .store
                .replace_command_receipt(
                    &permit,
                    &attempt.publication.command_id,
                    etag.as_deref(),
                    serde_json::to_vec(&pending).map_err(json_error)?,
                )
                .await
            {
                Ok(()) => Ok(()),
                Err(error) if is_condition_conflict(&error) => Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog Head changed while claiming the command receipt",
                )),
                Err(error) => Err(storage_error(error)),
            },
        }
    }

    async fn resolve_pending_command_receipt(
        &self,
        record: CommandReceiptRecord,
        etag: Option<String>,
    ) -> Result<Option<PublicationReceipt>> {
        let publishing = record.state == CommandReceiptState::Publishing;
        let intended = record.intended_publication.as_deref().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "command receipt has no intended publication",
            )
        })?;
        let Some((head, exact)) = self.exact_head().await? else {
            return Ok(None);
        };
        let publication = match self.store.read_publication(intended).await {
            Ok(bytes) => Some(decode_publication(&bytes)?),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => None,
            Err(error) => return Err(storage_error(error)),
        };
        if head.publication_location.as_deref() == Some(intended) {
            let publication = publication.ok_or_else(|| {
                Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog Head points to a missing command publication",
                )
            })?;
            self.validate_receipt_publication(&record, &publication, &head)?;
            let committed = self
                .finalize_command_receipt(
                    record,
                    etag,
                    CommandReceiptState::Committed,
                    Some(publication.generation),
                    publication.new_snapshot_id,
                )
                .await?;
            let committed = if committed.state == CommandReceiptState::Committed {
                committed
            } else {
                self.reconcile_command_receipt_after_stale_race(committed)
                    .await?
            };
            return (committed.state == CommandReceiptState::Committed)
                .then(|| committed.publication_receipt())
                .transpose();
        }
        let base_is_current = record.base_generation == Some(head.generation)
            && record.base_head_checksum.as_deref() == Some(head.checksum.as_str())
            && record.base_publication.as_deref() == head.publication_location.as_deref();
        if base_is_current {
            if let Some(publication) = publication.as_ref() {
                self.adopt_existing_publication_head(&record, publication, &head, &exact)
                    .await?;
                let committed = self
                    .finalize_command_receipt(
                        record,
                        etag,
                        CommandReceiptState::Committed,
                        Some(publication.generation),
                        publication.new_snapshot_id,
                    )
                    .await?;
                if committed.state != CommandReceiptState::Committed {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "command receipt changed while adopting its immutable publication",
                    ));
                }
                self.finalize_asset_marker_for_publication(publication)
                    .await?;
                return Ok(Some(committed.publication_receipt()?));
            }
            if publishing {
                // Publishing is a durable ownership state, but an owner
                // crash is indistinguishable from a live owner while the
                // base Head remains current. A matching retry may resume;
                // it must not turn the state into Stale or report a commit.
                return Ok(None);
            }
            self.finalize_command_receipt_stale_if_head_unchanged(
                record,
                etag,
                &head,
                exact.etag.as_deref(),
            )
            .await?;
            return Ok(None);
        }

        let Some(publication) = publication else {
            let resolved = self
                .finalize_command_receipt_stale_if_head_advanced(record, etag, &head)
                .await?;
            return if resolved.state == CommandReceiptState::Committed {
                Ok(Some(resolved.publication_receipt()?))
            } else {
                Ok(None)
            };
        };

        // A receipt left Pending or Publishing while the Head advanced can
        // require the expensive immutable-chain recovery path. Publishing is
        // never made Stale while its base Head is still current: the writer
        // may have won the exact-key transition and be between that CAS and
        // writing its immutable publication.
        let committed = self
            .resolve_publication_from_head(head.clone(), &record.attempt())
            .await?;
        let resolved = if committed {
            self.finalize_command_receipt(
                record,
                etag,
                CommandReceiptState::Committed,
                Some(publication.generation),
                publication.new_snapshot_id,
            )
            .await?
        } else {
            self.finalize_command_receipt_stale_if_head_advanced(record, etag, &head)
                .await?
        };
        if resolved.state == CommandReceiptState::Committed {
            Ok(Some(resolved.publication_receipt()?))
        } else {
            Ok(None)
        }
    }

    fn validate_publication_for_base(
        &self,
        receipt: &CommandReceiptRecord,
        publication: &PublicationRecord,
        base: &CatalogHead,
    ) -> Result<()> {
        if publication.command_id != receipt.command_id
            || publication.command_kind != receipt.command_kind
            || publication.command_digest != receipt.command_digest
            || publication.previous_generation != receipt.base_generation
            || publication.previous_publication.as_deref() != receipt.base_publication.as_deref()
            || publication.previous_head_checksum.as_deref()
                != receipt.base_head_checksum.as_deref()
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt does not match its publication evidence",
            ));
        }
        let expected_generation = receipt
            .base_generation
            .map_or(0, |generation| generation.saturating_add(1));
        if publication.generation != expected_generation
            || publication.next_head.generation != publication.generation
            || publication.next_head_checksum != publication.next_head.checksum
            || head_checksum(&publication.next_head)? != publication.next_head_checksum
            || publication.next_head.space_id != base.space_id
            || publication.next_head.namespace != base.namespace
            || publication.next_head.publication_location.as_deref()
                != receipt.intended_publication.as_deref()
            || publication.next_head.publication_command_id.as_deref()
                != Some(receipt.command_id.as_str())
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "immutable publication does not describe the receipt's next Head",
            ));
        }
        Ok(())
    }

    /// Adopts a matching immutable publication without rerunning Iceberg's
    /// data/manifest/metadata writer. The exact current Head is reread under
    /// the single-process serializer where needed, then the embedded next
    /// Head is published with the same conditional boundary as a fresh write.
    async fn adopt_existing_publication_head(
        &self,
        receipt: &CommandReceiptRecord,
        publication: &PublicationRecord,
        observed_head: &CatalogHead,
        observed_exact: &ExactCatalogHead,
    ) -> Result<()> {
        let base_is_current = receipt.base_generation == Some(observed_head.generation)
            && receipt.base_head_checksum.as_deref() == Some(observed_head.checksum.as_str())
            && receipt.base_publication.as_deref() == observed_head.publication_location.as_deref();
        if !base_is_current {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while adopting an immutable publication",
            ));
        }
        self.validate_publication_for_base(receipt, publication, observed_head)?;
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
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
        self.validate_publication_for_base(receipt, publication, &head)?;
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

    async fn finalize_asset_marker_for_publication(
        &self,
        publication: &PublicationRecord,
    ) -> Result<()> {
        if publication.command_kind != "asset.delete" {
            return Ok(());
        }
        let Some(asset_id) = publication
            .affected_table
            .table
            .strip_prefix("_asset_delete_")
        else {
            return Ok(());
        };
        let Some((marker, etag)) = self.asset_marker(asset_id).await? else {
            return Ok(());
        };
        if marker.command_id != publication.command_id
            || marker.command_kind != publication.command_kind
            || marker.command_digest != publication.command_digest
            || marker.base_generation != publication.previous_generation
            || marker.base_head_checksum != publication.previous_head_checksum
            || marker.base_publication != publication.previous_publication
            || marker.intended_publication.as_deref()
                != Some(
                    publication
                        .next_head
                        .publication_location
                        .as_deref()
                        .unwrap_or_default(),
                )
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker does not match its committed publication",
            ));
        }
        if marker.state == AssetLifecycleState::Committed {
            return Ok(());
        }
        let resolved = self
            .finalize_asset_marker(asset_id, marker, etag, AssetLifecycleState::Committed)
            .await?;
        if resolved.state != AssetLifecycleState::Committed {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker was not committed with its publication",
            ));
        }
        Ok(())
    }

    fn validate_receipt_publication(
        &self,
        receipt: &CommandReceiptRecord,
        publication: &PublicationRecord,
        head: &CatalogHead,
    ) -> Result<()> {
        validate_publication_matches_head(publication, head)?;
        if publication.command_id != receipt.command_id
            || publication.command_kind != receipt.command_kind
            || publication.command_digest != receipt.command_digest
            || publication.previous_generation != receipt.base_generation
            || publication.previous_publication.as_deref() != receipt.base_publication.as_deref()
            || publication.previous_head_checksum.as_deref()
                != receipt.base_head_checksum.as_deref()
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt does not match its publication evidence",
            ));
        }
        Ok(())
    }

    async fn finalize_command_receipt(
        &self,
        mut record: CommandReceiptRecord,
        etag: Option<String>,
        state: CommandReceiptState,
        catalog_generation: Option<u64>,
        snapshot_id: Option<i64>,
    ) -> Result<CommandReceiptRecord> {
        record.state = state;
        record.catalog_generation = catalog_generation;
        record.snapshot_id = snapshot_id;
        let bytes = serde_json::to_vec(&record).map_err(json_error)?;
        let permit = self.mutation_permit().map_err(storage_error)?;
        match self
            .store
            .replace_command_receipt(&permit, &record.command_id, etag.as_deref(), bytes)
            .await
        {
            Ok(()) => Ok(record),
            Err(error) if is_condition_conflict(&error) => {
                let Some((bytes, _)) = self
                    .store
                    .read_command_receipt(&record.command_id)
                    .await
                    .map_err(storage_error)?
                else {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "command receipt disappeared during finalization",
                    ));
                };
                serde_json::from_slice(&bytes).map_err(json_error)
            }
            Err(error) => Err(storage_error(error)),
        }
    }

    /// A Pending receipt may be made terminal only if the exact Head used for
    /// the decision is still authoritative.  The receipt object and the Head
    /// are separate OpenDAL objects, so the Head check immediately before the
    /// receipt CAS is the validation boundary we can enforce here.  A second
    /// check repairs the only remaining race: a writer can win the Head CAS
    /// after the first check but before the receipt replacement.
    async fn finalize_command_receipt_stale_if_head_unchanged(
        &self,
        record: CommandReceiptRecord,
        _etag: Option<String>,
        observed_head: &CatalogHead,
        observed_head_etag: Option<&str>,
    ) -> Result<CommandReceiptRecord> {
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let Some((current_head, current_exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while resolving a pending command receipt",
            ));
        };
        if !exact_head_matches(
            observed_head,
            observed_head_etag,
            &current_head,
            &current_exact,
        ) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while resolving a pending command receipt",
            ));
        }

        // Re-read the exact-key state while holding the single-process
        // serializer. With backends that do not expose ETags, this is the
        // conditional transition point: a writer that already entered
        // Publishing must be observed as such and cannot be overwritten by
        // the stale resolver.
        let Some((bytes, current_etag)) = self
            .store
            .read_command_receipt(&record.command_id)
            .await
            .map_err(storage_error)?
        else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt disappeared during stale resolution",
            ));
        };
        let record = serde_json::from_slice::<CommandReceiptRecord>(&bytes).map_err(json_error)?;
        if record.state != CommandReceiptState::Pending {
            return Ok(record);
        }

        let resolved = self
            .finalize_command_receipt(record, current_etag, CommandReceiptState::Stale, None, None)
            .await?;
        if resolved.state != CommandReceiptState::Stale {
            return Ok(resolved);
        }

        let Some((after_head, after_exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while resolving a pending command receipt",
            ));
        };
        if exact_head_matches(observed_head, observed_head_etag, &after_head, &after_exact) {
            return Ok(resolved);
        }

        // The Head moved during terminalization.  If the command publication
        // won that race, upgrade the exact-key state before returning; if an
        // unrelated publication won, the Stale state remains correct.
        self.reconcile_command_receipt_after_stale_race(resolved)
            .await
    }

    /// A Publishing receipt can only be abandoned after the Head has moved
    /// away from the receipt's base. That proves the writer's captured Head
    /// CAS can no longer succeed, even if it writes its immutable publication
    /// object after this point. Unlike the Pending path, this is recovery from
    /// an already-owned publication attempt, not ordinary stale detection.
    async fn finalize_command_receipt_stale_if_head_advanced(
        &self,
        mut record: CommandReceiptRecord,
        _etag: Option<String>,
        _observed_head: &CatalogHead,
    ) -> Result<CommandReceiptRecord> {
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let Some((current_head, _)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while recovering a stale command publication",
            ));
        };
        let Some((bytes, current_etag)) = self
            .store
            .read_command_receipt(&record.command_id)
            .await
            .map_err(storage_error)?
        else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt disappeared during stale recovery",
            ));
        };
        let current_record =
            serde_json::from_slice::<CommandReceiptRecord>(&bytes).map_err(json_error)?;
        let same_attempt = current_record.command_id == record.command_id
            && current_record.command_kind == record.command_kind
            && current_record.command_digest == record.command_digest
            && current_record.base_generation == record.base_generation
            && current_record.base_head_checksum == record.base_head_checksum
            && current_record.base_publication == record.base_publication
            && current_record.intended_publication == record.intended_publication;
        if !same_attempt
            || !matches!(
                current_record.state,
                CommandReceiptState::Pending | CommandReceiptState::Publishing
            )
        {
            return Ok(current_record);
        }
        record = current_record;
        let etag = current_etag;
        let base_is_current = record.base_generation == Some(current_head.generation)
            && record.base_head_checksum.as_deref() == Some(current_head.checksum.as_str())
            && record.base_publication.as_deref() == current_head.publication_location.as_deref();
        if base_is_current {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command publication is still in progress",
            ));
        }
        record.state = CommandReceiptState::Stale;
        record.catalog_generation = None;
        record.snapshot_id = None;
        let bytes = serde_json::to_vec(&record).map_err(json_error)?;
        let permit = self.mutation_permit().map_err(storage_error)?;
        match self
            .store
            .replace_command_receipt(&permit, &record.command_id, etag.as_deref(), bytes)
            .await
        {
            Ok(()) => Ok(record),
            Err(error) if is_condition_conflict(&error) => {
                let Some((bytes, _)) = self
                    .store
                    .read_command_receipt(&record.command_id)
                    .await
                    .map_err(storage_error)?
                else {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "command receipt disappeared during stale recovery",
                    ));
                };
                serde_json::from_slice(&bytes).map_err(json_error)
            }
            Err(error) => Err(storage_error(error)),
        }
    }

    async fn reconcile_command_receipt_after_stale_race(
        &self,
        resolved: CommandReceiptRecord,
    ) -> Result<CommandReceiptRecord> {
        let Some((bytes, etag)) = self
            .store
            .read_command_receipt(&resolved.command_id)
            .await
            .map_err(storage_error)?
        else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt disappeared during Head race recovery",
            ));
        };
        let current: CommandReceiptRecord = serde_json::from_slice(&bytes).map_err(json_error)?;
        if current.state != CommandReceiptState::Stale {
            return Ok(current);
        }
        let Some(intended) = current.intended_publication.as_deref() else {
            return Ok(current);
        };
        let Some((head, _)) = self.exact_head().await? else {
            return Ok(current);
        };
        let publication = match self.store.read_publication(intended).await {
            Ok(bytes) => Some(decode_publication(&bytes)?),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => None,
            Err(error) => return Err(storage_error(error)),
        };
        let Some(publication) = publication else {
            return Ok(current);
        };
        let committed = if head.publication_location.as_deref() == Some(intended) {
            validate_publication_matches_head(&publication, &head)?;
            self.validate_receipt_publication(&current, &publication, &head)?;
            true
        } else if self
            .resolve_publication_from_head(head.clone(), &current.attempt())
            .await?
        {
            self.validate_receipt_publication(&current, &publication, &publication.next_head)?;
            true
        } else {
            false
        };
        if !committed {
            return Ok(current);
        }
        self.finalize_command_receipt(
            current,
            etag,
            CommandReceiptState::Committed,
            Some(publication.generation),
            publication.new_snapshot_id,
        )
        .await
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

    async fn asset_marker(
        &self,
        asset_id: &str,
    ) -> Result<Option<(AssetLifecycleMarker, Option<String>)>> {
        let Some((bytes, etag)) = self
            .store
            .read_asset_lifecycle_marker(asset_id)
            .await
            .map_err(storage_error)?
        else {
            return Ok(None);
        };
        Ok(Some((
            serde_json::from_slice(&bytes).map_err(json_error)?,
            etag,
        )))
    }

    fn pending_asset_marker(
        &self,
        attempt: &PublicationAttempt,
        intended_publication: String,
    ) -> AssetLifecycleMarker {
        AssetLifecycleMarker {
            command_id: attempt.publication.command_id.clone(),
            command_kind: attempt.publication.command_kind.clone(),
            command_digest: attempt.publication.command_digest.clone(),
            state: AssetLifecycleState::Pending,
            base_generation: attempt.expected_generation,
            base_head_checksum: attempt.expected_head_checksum.clone(),
            base_publication: attempt.expected_previous_publication.clone(),
            intended_publication: Some(intended_publication),
        }
    }

    async fn create_asset_marker(
        &self,
        asset_id: &str,
        marker: &AssetLifecycleMarker,
    ) -> Result<()> {
        let permit = self.mutation_permit().map_err(storage_error)?;
        match self
            .store
            .create_asset_lifecycle_marker(
                &permit,
                asset_id,
                serde_json::to_vec(marker).map_err(json_error)?,
            )
            .await
        {
            Ok(()) => Ok(()),
            Err(error) if is_condition_conflict(&error) => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while claiming the Asset lifecycle marker",
            )),
            Err(error) => Err(storage_error(error)),
        }
    }

    async fn finalize_asset_marker(
        &self,
        asset_id: &str,
        mut marker: AssetLifecycleMarker,
        etag: Option<String>,
        state: AssetLifecycleState,
    ) -> Result<AssetLifecycleMarker> {
        marker.state = state;
        let bytes = serde_json::to_vec(&marker).map_err(json_error)?;
        let permit = self.mutation_permit().map_err(storage_error)?;
        match self
            .store
            .replace_asset_lifecycle_marker(&permit, asset_id, etag.as_deref(), bytes)
            .await
        {
            Ok(()) => Ok(marker),
            Err(error) if is_condition_conflict(&error) => self
                .asset_marker(asset_id)
                .await?
                .map(|(marker, _)| marker)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::DataInvalid,
                        "Asset lifecycle marker disappeared during finalization",
                    )
                }),
            Err(error) => Err(storage_error(error)),
        }
    }

    async fn finalize_asset_marker_stale_if_head_unchanged(
        &self,
        asset_id: &str,
        _marker: AssetLifecycleMarker,
        _etag: Option<String>,
        observed_head: &CatalogHead,
        observed_head_etag: Option<&str>,
    ) -> Result<AssetLifecycleMarker> {
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let Some((current_head, current_exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while resolving a pending Asset lifecycle marker",
            ));
        };
        if !exact_head_matches(
            observed_head,
            observed_head_etag,
            &current_head,
            &current_exact,
        ) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while resolving a pending Asset lifecycle marker",
            ));
        }

        // The exact-key state is re-read inside the same single-process
        // serializer used by publication writers. A marker that already
        // crossed into Publishing must never be overwritten as Stale by a
        // resolver that still holds the old Pending snapshot.
        let Some((bytes, current_etag)) = self
            .store
            .read_asset_lifecycle_marker(asset_id)
            .await
            .map_err(storage_error)?
        else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker disappeared during stale resolution",
            ));
        };
        let marker = serde_json::from_slice::<AssetLifecycleMarker>(&bytes).map_err(json_error)?;
        if marker.state != AssetLifecycleState::Pending {
            return Ok(marker);
        }

        let resolved = self
            .finalize_asset_marker(asset_id, marker, current_etag, AssetLifecycleState::Stale)
            .await?;
        if resolved.state != AssetLifecycleState::Stale {
            return Ok(resolved);
        }
        let Some((after_head, after_exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while resolving a pending Asset lifecycle marker",
            ));
        };
        if exact_head_matches(observed_head, observed_head_etag, &after_head, &after_exact) {
            return Ok(resolved);
        }

        // The Head moved while the marker was being terminalized.  Reconcile
        // the marker against the authoritative Head before exposing the
        // terminal result to readers.
        self.reconcile_asset_marker_after_stale_race(asset_id).await
    }

    /// Publishing may be recovered as Stale only after the captured base Head
    /// is no longer current. The old writer's Head CAS can then no longer
    /// succeed, so a late immutable publication object cannot become
    /// authoritative.
    async fn finalize_asset_marker_stale_if_head_advanced(
        &self,
        asset_id: &str,
        mut marker: AssetLifecycleMarker,
        _etag: Option<String>,
        _observed_head: &CatalogHead,
    ) -> Result<AssetLifecycleMarker> {
        let _write_guard = if self.store.write_mode() == CatalogWriteMode::SingleProcess {
            Some(self.store.single_process_serializer().lock_owned().await)
        } else {
            None
        };
        let Some((current_head, _)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while recovering a stale Asset deletion",
            ));
        };
        let Some((bytes, current_etag)) = self
            .store
            .read_asset_lifecycle_marker(asset_id)
            .await
            .map_err(storage_error)?
        else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker disappeared during stale recovery",
            ));
        };
        let current_marker =
            serde_json::from_slice::<AssetLifecycleMarker>(&bytes).map_err(json_error)?;
        let same_attempt = current_marker.command_id == marker.command_id
            && current_marker.command_kind == marker.command_kind
            && current_marker.command_digest == marker.command_digest
            && current_marker.base_generation == marker.base_generation
            && current_marker.base_head_checksum == marker.base_head_checksum
            && current_marker.base_publication == marker.base_publication
            && current_marker.intended_publication == marker.intended_publication;
        if !same_attempt
            || !matches!(
                current_marker.state,
                AssetLifecycleState::Pending | AssetLifecycleState::Publishing
            )
        {
            return Ok(current_marker);
        }
        marker = current_marker;
        let etag = current_etag;
        if marker.matches_base(&current_head) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset deletion publication is still in progress",
            ));
        }
        marker.state = AssetLifecycleState::Stale;
        let bytes = serde_json::to_vec(&marker).map_err(json_error)?;
        let permit = self.mutation_permit().map_err(storage_error)?;
        match self
            .store
            .replace_asset_lifecycle_marker(&permit, asset_id, etag.as_deref(), bytes)
            .await
        {
            Ok(()) => Ok(marker),
            Err(error) if is_condition_conflict(&error) => self
                .asset_marker(asset_id)
                .await?
                .map(|(marker, _)| marker)
                .ok_or_else(|| {
                    Error::new(
                        ErrorKind::DataInvalid,
                        "Asset lifecycle marker disappeared during stale recovery",
                    )
                }),
            Err(error) => Err(storage_error(error)),
        }
    }

    async fn reconcile_asset_marker_after_stale_race(
        &self,
        asset_id: &str,
    ) -> Result<AssetLifecycleMarker> {
        let Some((bytes, etag)) = self
            .store
            .read_asset_lifecycle_marker(asset_id)
            .await
            .map_err(storage_error)?
        else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker disappeared during Head race recovery",
            ));
        };
        let current: AssetLifecycleMarker = serde_json::from_slice(&bytes).map_err(json_error)?;
        if current.state != AssetLifecycleState::Stale {
            return Ok(current);
        }
        let Some(intended) = current.intended_publication.as_deref() else {
            return Ok(current);
        };
        let Some((head, _)) = self.exact_head().await? else {
            return Ok(current);
        };
        let publication = match self.store.read_publication(intended).await {
            Ok(bytes) => Some(decode_publication(&bytes)?),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => None,
            Err(error) => return Err(storage_error(error)),
        };
        let Some(publication) = publication else {
            return Ok(current);
        };
        let committed = if head.publication_location.as_deref() == Some(intended) {
            validate_publication_matches_head(&publication, &head)?;
            self.validate_asset_publication(&current, &publication)?;
            true
        } else if self
            .resolve_publication_from_head(head.clone(), &current.recovery_attempt())
            .await?
        {
            self.validate_asset_publication(&current, &publication)?;
            true
        } else {
            false
        };
        if !committed {
            return Ok(current);
        }
        self.finalize_asset_marker(asset_id, current, etag, AssetLifecycleState::Committed)
            .await
    }

    fn validate_asset_publication(
        &self,
        marker: &AssetLifecycleMarker,
        publication: &PublicationRecord,
    ) -> Result<()> {
        if publication.command_id != marker.command_id
            || publication.command_kind != marker.command_kind
            || publication.command_digest != marker.command_digest
            || publication.previous_generation != marker.base_generation
            || publication.previous_publication.as_deref() != marker.base_publication.as_deref()
            || publication.previous_head_checksum.as_deref() != marker.base_head_checksum.as_deref()
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker does not match its publication evidence",
            ));
        }
        Ok(())
    }

    /// Claims the exact Asset lifecycle key immediately before writing the
    /// deletion publication. Pending-to-Publishing and Pending-to-Stale use
    /// the same conditional object transition, so a stale resolver that wins
    /// first prevents this writer from publishing at all.
    async fn begin_asset_marker_publication(
        &self,
        asset_id: &str,
        attempt: &PublicationAttempt,
        intended_publication: &str,
    ) -> Result<()> {
        let Some((mut marker, etag)) = self.asset_marker(asset_id).await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker disappeared before publication",
            ));
        };
        if marker.command_id != attempt.publication.command_id
            || marker.command_kind != attempt.publication.command_kind
            || marker.command_digest != attempt.publication.command_digest
            || marker.base_generation != attempt.expected_generation
            || marker.base_head_checksum != attempt.expected_head_checksum
            || marker.base_publication != attempt.expected_previous_publication
            || !matches!(
                marker.state,
                AssetLifecycleState::Pending | AssetLifecycleState::Publishing
            )
            || marker.intended_publication.as_deref() != Some(intended_publication)
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while publishing an Asset lifecycle marker",
            ));
        }
        match marker.state {
            AssetLifecycleState::Pending => {
                marker.state = AssetLifecycleState::Publishing;
                let bytes = serde_json::to_vec(&marker).map_err(json_error)?;
                let permit = self.mutation_permit().map_err(storage_error)?;
                match self
                    .store
                    .replace_asset_lifecycle_marker(&permit, asset_id, etag.as_deref(), bytes)
                    .await
                {
                    Ok(()) => Ok(()),
                    Err(error) if is_condition_conflict(&error) => Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog Head changed while starting Asset deletion publication",
                    )),
                    Err(error) => Err(storage_error(error)),
                }
            }
            AssetLifecycleState::Publishing => Ok(()),
            AssetLifecycleState::Committed | AssetLifecycleState::Stale => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while publishing a terminal Asset lifecycle marker",
            )),
        }
    }

    async fn finalize_asset_marker_after_publication(
        &self,
        asset_id: &str,
        attempt: &PublicationAttempt,
        intended_publication: &str,
    ) -> Result<()> {
        for _ in 0..3 {
            let Some((marker, etag)) = self.asset_marker(asset_id).await? else {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Asset lifecycle marker disappeared after publication",
                ));
            };
            if marker.command_id != attempt.publication.command_id
                || marker.command_kind != attempt.publication.command_kind
                || marker.command_digest != attempt.publication.command_digest
                || marker.intended_publication.as_deref() != Some(intended_publication)
            {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Asset lifecycle marker does not match the successful publication",
                ));
            }
            if marker.state == AssetLifecycleState::Committed {
                return Ok(());
            }
            if marker.state != AssetLifecycleState::Publishing {
                return Err(Error::new(
                    ErrorKind::DataInvalid,
                    "Asset lifecycle marker is not Publishing after the Catalog Head CAS",
                ));
            }
            let resolved = self
                .finalize_asset_marker(asset_id, marker, etag, AssetLifecycleState::Committed)
                .await?;
            if resolved.state == AssetLifecycleState::Committed {
                return Ok(());
            }
        }
        Err(Error::new(
            ErrorKind::DataInvalid,
            "Asset lifecycle marker changed during publication finalization",
        ))
    }

    /// Resolves Pending only when a process may have crashed during
    /// publication. Committed and Stale markers are terminal exact-key state,
    /// so ordinary reads never inspect the immutable publication chain.
    async fn resolve_pending_asset_marker(
        &self,
        asset_id: &str,
        marker: AssetLifecycleMarker,
        etag: Option<String>,
    ) -> Result<AssetLifecycleMarker> {
        let publishing = marker.state == AssetLifecycleState::Publishing;
        let intended = marker.intended_publication.as_deref().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker has no intended publication",
            )
        })?;
        let Some((head, exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while resolving a pending Asset lifecycle marker",
            ));
        };
        let publication = match self.store.read_publication(intended).await {
            Ok(bytes) => Some(decode_publication(&bytes)?),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => None,
            Err(error) => return Err(storage_error(error)),
        };
        if head.publication_location.as_deref() == Some(intended) {
            let publication = publication.ok_or_else(|| {
                Error::new(
                    ErrorKind::DataInvalid,
                    "Catalog Head points to a missing Asset lifecycle publication",
                )
            })?;
            validate_publication_matches_head(&publication, &head)?;
            self.validate_asset_publication(&marker, &publication)?;
            let resolved = self
                .finalize_asset_marker(asset_id, marker, etag, AssetLifecycleState::Committed)
                .await?;
            return if resolved.state == AssetLifecycleState::Committed {
                Ok(resolved)
            } else {
                self.reconcile_asset_marker_after_stale_race(asset_id).await
            };
        }
        let base_is_current = marker.matches_base(&head);
        if base_is_current {
            if publishing {
                // Keep the read barrier fail-closed, while allowing the
                // matching delete command to resume this durable attempt.
                return Ok(marker);
            }
            return self
                .finalize_asset_marker_stale_if_head_unchanged(
                    asset_id,
                    marker,
                    etag,
                    &head,
                    exact.etag.as_deref(),
                )
                .await;
        }

        if publication.is_none() {
            return self
                .finalize_asset_marker_stale_if_head_advanced(asset_id, marker, etag, &head)
                .await;
        }

        let committed = self
            .resolve_publication_from_head(head.clone(), &marker.recovery_attempt())
            .await?;
        if committed {
            self.validate_asset_publication(
                &marker,
                publication
                    .as_ref()
                    .expect("pending recovery has its intended publication"),
            )?;
        }
        if committed {
            let resolved = self
                .finalize_asset_marker(asset_id, marker, etag, AssetLifecycleState::Committed)
                .await?;
            if resolved.state == AssetLifecycleState::Committed {
                Ok(resolved)
            } else {
                self.reconcile_asset_marker_after_stale_race(asset_id).await
            }
        } else {
            self.finalize_asset_marker_stale_if_head_advanced(asset_id, marker, etag, &head)
                .await
        }
    }

    pub(crate) async fn asset_is_deleted(&self, asset_id: &str) -> Result<bool> {
        let Some((marker, etag)) = self.asset_marker(asset_id).await? else {
            return Ok(false);
        };
        match marker.state {
            AssetLifecycleState::Committed => Ok(true),
            AssetLifecycleState::Stale => Ok(false),
            AssetLifecycleState::Pending | AssetLifecycleState::Publishing => {
                let resolved = self
                    .resolve_pending_asset_marker(asset_id, marker, etag)
                    .await?;
                match resolved.state {
                    AssetLifecycleState::Committed => Ok(true),
                    AssetLifecycleState::Stale => Ok(false),
                    AssetLifecycleState::Pending | AssetLifecycleState::Publishing => {
                        Err(Error::new(
                            ErrorKind::DataInvalid,
                            "Asset deletion publication is still in progress",
                        ))
                    }
                }
            }
        }
    }

    /// Reclaims Asset bytes whose authoritative deletion publication already
    /// committed but whose physical delete was interrupted.  The lifecycle
    /// marker is retained as the durable tombstone, so a later pass can retry
    /// indefinitely without consulting or mutating Catalog history.
    pub(crate) async fn garbage_collect_deleted_asset_blobs(&self) -> Result<usize> {
        let markers = self
            .store
            .list_asset_lifecycle_markers()
            .await
            .map_err(storage_error)?;
        let mut deleted = 0usize;
        for (asset_id, bytes) in markers {
            if deleted >= MAX_DELETED_ASSET_BLOBS_PER_PASS {
                break;
            }
            if validate_asset_id(&asset_id).is_err() {
                continue;
            }
            let marker: AssetLifecycleMarker =
                serde_json::from_slice(&bytes).map_err(json_error)?;
            if marker.state != AssetLifecycleState::Committed {
                continue;
            }
            let permit = self.mutation_permit().map_err(storage_error)?;
            self.store
                .delete_asset_blob(&permit, &asset_id)
                .await
                .map_err(storage_error)?;
            deleted = deleted.saturating_add(1);
        }
        Ok(deleted)
    }

    async fn recover_existing_asset_publication(
        &self,
        asset_id: &str,
        marker: &AssetLifecycleMarker,
    ) -> Result<bool> {
        if marker.state != AssetLifecycleState::Publishing {
            return Ok(false);
        }
        let intended = marker.intended_publication.as_deref().ok_or_else(|| {
            Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker has no intended publication",
            )
        })?;
        let publication = match self.store.read_publication(intended).await {
            Ok(bytes) => decode_publication(&bytes)?,
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(storage_error(error)),
        };
        if publication.command_kind != "asset.delete"
            || publication
                .affected_table
                .table
                .strip_prefix("_asset_delete_")
                != Some(asset_id)
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Asset lifecycle marker does not match its publication target",
            ));
        }
        let Some((head, exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head disappeared while recovering Asset deletion",
            ));
        };
        if !marker.matches_base(&head) {
            if head.publication_location.as_deref() == Some(intended) {
                validate_publication_matches_head(&publication, &head)?;
                self.validate_asset_publication(marker, &publication)?;
                let attempt = marker.recovery_attempt();
                self.finalize_command_receipt_after_publication(
                    &attempt,
                    intended,
                    publication.generation,
                    publication.new_snapshot_id,
                )
                .await?;
                self.finalize_asset_marker_for_publication(&publication)
                    .await?;
                return Ok(true);
            }
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while recovering Asset deletion",
            ));
        }
        let receipt =
            CommandReceiptRecord::pending(&marker.recovery_attempt(), intended.to_string());
        self.adopt_existing_publication_head(&receipt, &publication, &head, &exact)
            .await?;
        let attempt = marker.recovery_attempt();
        self.finalize_command_receipt_after_publication(
            &attempt,
            intended,
            publication.generation,
            publication.new_snapshot_id,
        )
        .await?;
        self.finalize_asset_marker_for_publication(&publication)
            .await?;
        Ok(true)
    }

    /// Publishes an Asset lifecycle state transition through the same
    /// optimistic Catalog Head boundary as Form and Entry mutations. The
    /// physical blob is removed only after this marker wins the CAS.
    pub(crate) async fn mark_asset_deleted(&self, asset_id: &str) -> Result<()> {
        self.claim_mutation()?;

        // Resolve an existing in-flight marker before taking the local write
        // serializer. Recovery itself uses that serializer for the
        // Pending/Publishing -> Stale CAS; doing this first avoids recursively
        // locking the same process-local mutex from the recovery path.
        if let Some((marker, etag)) = self.asset_marker(asset_id).await? {
            match marker.state {
                AssetLifecycleState::Committed => return Ok(()),
                AssetLifecycleState::Pending | AssetLifecycleState::Publishing => {
                    let resolved = self
                        .resolve_pending_asset_marker(asset_id, marker, etag)
                        .await?;
                    if resolved.state == AssetLifecycleState::Committed {
                        return Ok(());
                    }
                }
                AssetLifecycleState::Stale => {}
            }
        }

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
        let intended_publication = self
            .store
            .publication_path(head.generation + 1, &attempt.publication.command_id);
        let marker = match self.asset_marker(asset_id).await? {
            None => {
                let pending = self.pending_asset_marker(&attempt, intended_publication.clone());
                self.create_asset_marker(asset_id, &pending).await?;
                self.asset_marker(asset_id).await?.ok_or_else(|| {
                    Error::new(
                        ErrorKind::DataInvalid,
                        "Asset lifecycle marker disappeared after creation",
                    )
                })?
            }
            Some((marker, etag)) => match marker.state {
                AssetLifecycleState::Committed => return Ok(()),
                AssetLifecycleState::Pending => {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Asset lifecycle marker changed while starting deletion",
                    ));
                }
                AssetLifecycleState::Publishing
                    if marker.command_id == attempt.publication.command_id
                        && marker.command_kind == attempt.publication.command_kind
                        && marker.command_digest == attempt.publication.command_digest
                        && marker.base_generation == attempt.expected_generation
                        && marker.base_head_checksum == attempt.expected_head_checksum
                        && marker.base_publication == attempt.expected_previous_publication
                        && marker.intended_publication.as_deref()
                            == Some(intended_publication.as_str()) =>
                {
                    // A restarted coordinator with the same command may
                    // resume an existing Publishing attempt. It never takes
                    // over a different command's lifecycle key.
                    (marker, etag)
                }
                AssetLifecycleState::Publishing => {
                    return Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Asset lifecycle marker changed while starting deletion",
                    ));
                }
                AssetLifecycleState::Stale => (marker, etag),
            },
        };

        if self
            .recover_existing_asset_publication(asset_id, &marker.0)
            .await?
        {
            return Ok(());
        }

        let pending = self.pending_asset_marker(&attempt, intended_publication.clone());
        let _marker_after_claim = match marker.1 {
            Some(etag) if marker.0.state == AssetLifecycleState::Stale => {
                let bytes = serde_json::to_vec(&pending).map_err(json_error)?;
                let permit = self.mutation_permit().map_err(storage_error)?;
                self.store
                    .replace_asset_lifecycle_marker(&permit, asset_id, Some(&etag), bytes)
                    .await
                    .map_err(|error| {
                        if is_condition_conflict(&error) {
                            Error::new(
                                ErrorKind::DataInvalid,
                                "Catalog Head changed while claiming the Asset lifecycle marker",
                            )
                        } else {
                            storage_error(error)
                        }
                    })?;
                self.asset_marker(asset_id).await?.ok_or_else(|| {
                    Error::new(
                        ErrorKind::DataInvalid,
                        "Asset lifecycle marker disappeared during takeover",
                    )
                })?
            }
            None if marker.0.state == AssetLifecycleState::Stale => {
                // Single-process backends do not provide an ETag. The
                // serializer still makes this replacement safe locally.
                let permit = self.mutation_permit().map_err(storage_error)?;
                self.store
                    .replace_asset_lifecycle_marker(
                        &permit,
                        asset_id,
                        None,
                        serde_json::to_vec(&pending).map_err(json_error)?,
                    )
                    .await
                    .map_err(storage_error)?;
                self.asset_marker(asset_id).await?.ok_or_else(|| {
                    Error::new(
                        ErrorKind::DataInvalid,
                        "Asset lifecycle marker disappeared during takeover",
                    )
                })?
            }
            current => (pending.clone(), current),
        };

        let next = head.next_generation();
        let publication = self
            .publish_asset_deletion(asset_id, &attempt, next, &intended_publication)
            .await;
        match publication {
            Ok(()) => {
                self.finalize_asset_marker_after_publication(
                    asset_id,
                    &attempt,
                    &intended_publication,
                )
                .await?;
                Ok(())
            }
            Err(error) if self.resolve_unknown_outcome(&attempt).await? => {
                if self.asset_is_deleted(asset_id).await? {
                    Ok(())
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    async fn publish_asset_deletion(
        &self,
        asset_id: &str,
        attempt: &PublicationAttempt,
        next: CatalogHead,
        intended_publication: &str,
    ) -> Result<()> {
        self.begin_asset_marker_publication(asset_id, attempt, intended_publication)
            .await?;
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

    /// Claims the exact-key receipt immediately before an immutable
    /// publication is written. This CAS is the publication start boundary:
    /// the resolver may change only Pending receipts to Stale, so a writer
    /// that owns Publishing cannot be invalidated between its check and the
    /// publication write.
    async fn begin_command_publication(
        &self,
        attempt: &PublicationAttempt,
        publication_path: &str,
    ) -> Result<()> {
        let Some((bytes, etag)) = self
            .store
            .read_command_receipt(&attempt.publication.command_id)
            .await
            .map_err(storage_error)?
        else {
            // Low-level catalog users and test fixtures may publish without
            // claiming a receipt. Coordinated mutations always have one.
            return Ok(());
        };
        let mut record: CommandReceiptRecord =
            serde_json::from_slice(&bytes).map_err(json_error)?;
        if !record.matches(&attempt.publication)
            || record.base_generation != attempt.expected_generation
            || record.base_head_checksum != attempt.expected_head_checksum
            || record.base_publication != attempt.expected_previous_publication
            || record.intended_publication.as_deref() != Some(publication_path)
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt does not match the publication attempt",
            ));
        }
        match record.state {
            CommandReceiptState::Pending => {
                record.state = CommandReceiptState::Publishing;
                let bytes = serde_json::to_vec(&record).map_err(json_error)?;
                let permit = self.mutation_permit().map_err(storage_error)?;
                match self
                    .store
                    .replace_command_receipt(
                        &permit,
                        &attempt.publication.command_id,
                        etag.as_deref(),
                        bytes,
                    )
                    .await
                {
                    Ok(()) => Ok(()),
                    Err(error) if is_condition_conflict(&error) => Err(Error::new(
                        ErrorKind::DataInvalid,
                        "Catalog Head changed while starting command publication",
                    )),
                    Err(error) => Err(storage_error(error)),
                }
            }
            // A retry of the same attempt may resume after the writer has
            // already claimed Publishing. It still cannot be reclaimed by a
            // different command because the exact key is already owned.
            CommandReceiptState::Publishing => Ok(()),
            CommandReceiptState::Committed | CommandReceiptState::Stale => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while publishing a terminal command receipt",
            )),
        }
    }

    async fn adopt_existing_publication_for_attempt(
        &self,
        attempt: &PublicationAttempt,
        publication_path: &str,
        publication: PublicationRecord,
    ) -> Result<()> {
        let Some((head, exact)) = self.exact_head().await? else {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head disappeared while adopting an immutable publication",
            ));
        };
        if attempt.expected_head.as_ref() != Some(&head) || attempt.expected_head_etag != exact.etag
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed while adopting an immutable publication",
            ));
        }
        let receipt = CommandReceiptRecord::pending(attempt, publication_path.to_string());
        self.adopt_existing_publication_head(&receipt, &publication, &head, &exact)
            .await?;
        self.finalize_command_receipt_after_publication(
            attempt,
            publication_path,
            publication.generation,
            publication.new_snapshot_id,
        )
        .await?;
        self.finalize_asset_marker_for_publication(&publication)
            .await?;
        Ok(())
    }

    async fn publish_new_head(
        &self,
        attempt: &PublicationAttempt,
        mut next: CatalogHead,
        update: PublicationUpdate,
    ) -> Result<()> {
        let new_snapshot_id = update.new_snapshot_id;
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
            affected_table: update.affected_table,
            base_metadata_location: update.base_metadata_location,
            new_metadata_location: update.new_metadata_location,
            base_snapshot_id: update.base_snapshot_id,
            base_schema_id: update.base_schema_id,
            new_snapshot_id,
            new_schema_id: update.new_schema_id,
            next_head_checksum: next.checksum.clone(),
            next_head: next.clone(),
            checksum: String::new(),
        };
        let mut publication = publication;
        publication.checksum = publication_checksum(&publication)?;
        self.begin_command_publication(attempt, &publication_path)
            .await?;
        if let Err(error) = self.write_publication(&publication).await {
            if error
                .to_string()
                .contains("publication path is already owned by another command")
            {
                let existing = decode_publication(
                    &self
                        .store
                        .read_publication(&publication_path)
                        .await
                        .map_err(storage_error)?,
                )?;
                self.adopt_existing_publication_for_attempt(attempt, &publication_path, existing)
                    .await?;
                return Ok(());
            }
            return Err(error);
        }
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
            Ok(()) => {
                self.finalize_command_receipt_after_publication(
                    attempt,
                    &publication_path,
                    next.generation,
                    new_snapshot_id,
                )
                .await?;
                Ok(())
            }
            Err(error) if is_condition_conflict(&error) => Err(Error::new(
                ErrorKind::DataInvalid,
                "Catalog Head changed before this publication could be committed",
            )),
            Err(error) => Err(storage_error(error)),
        }
    }

    async fn finalize_command_receipt_after_publication(
        &self,
        attempt: &PublicationAttempt,
        publication_path: &str,
        catalog_generation: u64,
        snapshot_id: Option<i64>,
    ) -> Result<()> {
        let Some((bytes, etag)) = self
            .store
            .read_command_receipt(&attempt.publication.command_id)
            .await
            .map_err(storage_error)?
        else {
            // Direct SpaceCatalog test fixtures and low-level catalog users do
            // not claim command receipts; coordinators always do.
            return Ok(());
        };
        let record: CommandReceiptRecord = serde_json::from_slice(&bytes).map_err(json_error)?;
        if !record.matches(&attempt.publication)
            || record.base_generation != attempt.expected_generation
            || record.base_head_checksum != attempt.expected_head_checksum
            || record.base_publication != attempt.expected_previous_publication
            || record.intended_publication.as_deref() != Some(publication_path)
        {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt does not match the successful publication",
            ));
        }
        if record.state == CommandReceiptState::Committed {
            return Ok(());
        }
        if !matches!(
            record.state,
            CommandReceiptState::Pending
                | CommandReceiptState::Publishing
                | CommandReceiptState::Stale
        ) {
            return Err(Error::new(
                ErrorKind::DataInvalid,
                "command receipt is not recoverable after the Catalog Head CAS",
            ));
        }
        self.finalize_command_receipt(
            record,
            etag,
            CommandReceiptState::Committed,
            Some(catalog_generation),
            snapshot_id,
        )
        .await?;
        Ok(())
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
    match error.kind() {
        opendal::ErrorKind::NotFound => crate::CheckpointUnavailable::new(error.to_string()).into(),
        _ => anyhow::Error::new(error).context("read checkpoint target"),
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

    fn asset_marker_for(
        catalog: &SpaceCatalog,
        publication: &PublicationContext,
        head: &CatalogHead,
    ) -> anyhow::Result<Vec<u8>> {
        Ok(serde_json::to_vec(&AssetLifecycleMarker {
            command_id: publication.command_id.clone(),
            command_kind: publication.command_kind.clone(),
            command_digest: publication.command_digest.clone(),
            state: AssetLifecycleState::Pending,
            base_generation: Some(head.generation),
            base_head_checksum: Some(head.checksum.clone()),
            base_publication: head.publication_location.clone(),
            intended_publication: Some(
                catalog
                    .store
                    .publication_path(head.generation + 1, &publication.command_id),
            ),
        })?)
    }

    async fn publish_test_generation(
        store: &SpaceCatalogStore,
        space_id: SpaceId,
        command_id: &str,
    ) -> AnyResult<()> {
        let catalog = SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(
            PublicationContext::with_command_digest(
                command_id,
                "test.generation",
                format!("{command_id}-digest"),
            ),
        );
        let attempt = catalog
            .publication_attempt()
            .await
            .map_err(|error| anyhow::anyhow!("{command_id}: {error}"))?;
        let head = attempt
            .expected_head
            .clone()
            .unwrap_or_else(|| CatalogHead::genesis(space_id, catalog.namespace()));
        let next = if attempt.expected_head.is_some() {
            head.next_generation()
        } else {
            head.clone()
        };
        catalog
            .publish_new_head(
                &attempt,
                next,
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: head.namespace.clone(),
                        table: "_test_generation".to_string(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: format!("test://generation/{command_id}"),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn exact_key_receipts_and_terminal_asset_markers_do_not_replay_history() -> AnyResult<()>
    {
        let operator = Operator::new(Memory::default())?;
        let (store, read_counter) =
            SpaceCatalogStore::new(operator, "spaces/online-index")?.with_read_counter();
        let permit = store.mutation_permit()?;
        let space_id = SpaceId::from(Uuid::from_u128(18_521));

        for generation in 0..64 {
            publish_test_generation(&store, space_id, &format!("generation-{generation}")).await?;
        }

        // Starting an unrelated mutation performs only exact Head and
        // command-receipt work, regardless of the length of the immutable
        // publication audit chain.
        let mutation = SpaceCatalog::new(store.clone(), space_id)?
            .with_publication_context(PublicationContext::with_command_digest(
                "unrelated-mutation",
                "test.unrelated",
                "unrelated-mutation-digest",
            ))
            .bind_exact_head()
            .await?;
        read_counter.store(0, Ordering::Relaxed);
        mutation.claim_command_receipt().await?;
        assert!(read_counter.load(Ordering::Relaxed) <= 3);

        // A normal new command is a single exact-key lookup, regardless of
        // the length of the immutable publication audit chain.
        read_counter.store(0, Ordering::Relaxed);
        let catalog = SpaceCatalog::new(store.clone(), space_id)?;
        let unrelated = PublicationContext::with_command_digest(
            "unrelated-command",
            "test.unrelated",
            "unrelated-digest",
        );
        assert!(catalog.publication_receipt(&unrelated).await?.is_none());
        assert!(read_counter.load(Ordering::Relaxed) <= 2);

        // A stale marker is terminal state. A live Asset therefore needs no
        // Head or publication-chain read to answer the normal byte/reference
        // availability check.
        let stale = AssetLifecycleMarker {
            command_id: "stale-asset-command".to_string(),
            command_kind: "asset.delete".to_string(),
            command_digest: "stale-asset-digest".to_string(),
            state: AssetLifecycleState::Stale,
            base_generation: Some(1),
            base_head_checksum: Some("old-head".to_string()),
            base_publication: Some("old-publication".to_string()),
            intended_publication: Some(store.publication_path(2, "stale-asset-command")),
        };
        store
            .create_asset_lifecycle_marker(
                &permit,
                "live-after-stale-marker",
                serde_json::to_vec(&stale)?,
            )
            .await?;
        read_counter.store(0, Ordering::Relaxed);
        assert!(!catalog.asset_is_deleted("live-after-stale-marker").await?);
        assert!(read_counter.load(Ordering::Relaxed) <= 2);

        // A receipt left Pending after the publication has reached Head is
        // only resolved through the exceptional crash-recovery path. Once
        // several later publications exist, that path is intentionally the
        // one case that may walk immutable history.
        let crash = PublicationContext::with_command_digest(
            "ambiguous-crash-command",
            "test.crash",
            "ambiguous-crash-digest",
        );
        let crash_catalog =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(crash.clone());
        let crash_attempt = crash_catalog.publication_attempt().await?;
        let crash_head = crash_attempt
            .expected_head
            .clone()
            .expect("history has a Catalog Head");
        crash_catalog
            .publish_new_head(
                &crash_attempt,
                crash_head.clone().next_generation(),
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: crash_head.namespace.clone(),
                        table: "_test_crash".to_string(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: "test://crash".to_string(),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await?;
        let intended = store.publication_path(crash_head.generation + 1, &crash.command_id);
        let pending = CommandReceiptRecord::pending(&crash_attempt, intended);
        store
            .create_command_receipt(&permit, &crash.command_id, serde_json::to_vec(&pending)?)
            .await?;
        for generation in 0..16 {
            publish_test_generation(&store, space_id, &format!("after-crash-{generation}")).await?;
        }
        read_counter.store(0, Ordering::Relaxed);
        assert!(catalog.publication_receipt(&crash).await?.is_some());
        assert!(read_counter.load(Ordering::Relaxed) > 4);
        Ok(())
    }

    #[tokio::test]
    async fn pending_command_receipt_head_race_cannot_end_as_stale() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?;
        let store = SpaceCatalogStore::new(operator, "spaces/receipt-race")?.single_process();
        let permit = store.mutation_permit()?;
        let space_id = SpaceId::from(Uuid::from_u128(18_523));
        publish_test_generation(&store, space_id, "receipt-race-base").await?;

        let command = PublicationContext::with_command_digest(
            "receipt-race-command",
            "test.receipt-race",
            "receipt-race-digest",
        );
        let writer =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(command.clone());
        let (observed_head, observed_exact) = writer.exact_head().await?.expect("base Head");
        let attempt = PublicationAttempt::from_exact(
            &command,
            Some((observed_head.clone(), observed_exact.clone())),
        );
        let intended = store.publication_path(observed_head.generation + 1, &command.command_id);
        let pending = CommandReceiptRecord::pending(&attempt, intended.clone());
        store
            .create_command_receipt(&permit, &command.command_id, serde_json::to_vec(&pending)?)
            .await?;
        let (_, pending_etag) = store
            .read_command_receipt(&command.command_id)
            .await?
            .expect("pending receipt");

        // Writer A atomically claims Publishing and pauses before creating
        // the immutable publication. Resolver B has already observed the
        // missing publication, but its Pending -> Stale CAS must now lose.
        writer
            .begin_command_publication(&attempt, &intended)
            .await?;
        let publishing = writer
            .finalize_command_receipt_stale_if_head_unchanged(
                pending.clone(),
                pending_etag.clone(),
                &observed_head,
                observed_exact.etag.as_deref(),
            )
            .await?;
        assert_eq!(publishing.state, CommandReceiptState::Publishing);

        // The paused writer resumes and is allowed to complete its
        // publication because it owns the exact-key Publishing state.
        writer
            .publish_new_head(
                &attempt,
                observed_head.next_generation(),
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: observed_head.namespace.clone(),
                        table: "_receipt_race".to_string(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: "test://receipt-race".to_string(),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await?;
        let (bytes, _) = store
            .read_command_receipt(&command.command_id)
            .await?
            .expect("receipt after race");
        let receipt: CommandReceiptRecord = serde_json::from_slice(&bytes)?;
        assert_eq!(receipt.state, CommandReceiptState::Committed);

        // Reverse ordering: a genuine stale finalization completes first.
        // The old writer is rejected before it can write a publication.
        let reverse = PublicationContext::with_command_digest(
            "receipt-race-reverse",
            "test.receipt-race",
            "receipt-race-reverse-digest",
        );
        let reverse_catalog =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(reverse.clone());
        let (reverse_head, reverse_exact) = reverse_catalog.exact_head().await?.expect("Head");
        let reverse_attempt = PublicationAttempt::from_exact(
            &reverse,
            Some((reverse_head.clone(), reverse_exact.clone())),
        );
        let reverse_intended =
            store.publication_path(reverse_head.generation + 1, &reverse.command_id);
        let reverse_pending = CommandReceiptRecord::pending(&reverse_attempt, reverse_intended);
        store
            .create_command_receipt(
                &permit,
                &reverse.command_id,
                serde_json::to_vec(&reverse_pending)?,
            )
            .await?;
        let (_, reverse_etag) = store
            .read_command_receipt(&reverse.command_id)
            .await?
            .expect("reverse receipt");
        let stale = reverse_catalog
            .finalize_command_receipt_stale_if_head_unchanged(
                reverse_pending,
                reverse_etag,
                &reverse_head,
                reverse_exact.etag.as_deref(),
            )
            .await?;
        assert_eq!(stale.state, CommandReceiptState::Stale);
        let error = reverse_catalog
            .publish_new_head(
                &reverse_attempt,
                reverse_head.next_generation(),
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: reverse_head.namespace.clone(),
                        table: "_receipt-race-reverse".to_string(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: "test://receipt-race-reverse".to_string(),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await
            .expect_err("a stale writer cannot publish");
        assert!(error.to_string().contains("Catalog Head changed"));
        let (head_after, _) = reverse_catalog.exact_head().await?.expect("Head");
        assert_eq!(head_after, reverse_head);
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

    #[tokio::test]
    async fn pending_asset_marker_head_race_cannot_end_as_stale() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?;
        let store = SpaceCatalogStore::new(operator, "spaces/asset-marker-race")?.single_process();
        let permit = store.mutation_permit()?;
        let space_id = SpaceId::from(Uuid::from_u128(18_524));
        publish_test_generation(&store, space_id, "asset-race-base").await?;

        let command = PublicationContext::with_command_digest(
            "asset-race-command",
            "asset.delete",
            "asset-race-digest",
        );
        let writer =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(command.clone());
        let (observed_head, observed_exact) = writer.exact_head().await?.expect("base Head");
        let attempt = PublicationAttempt::from_exact(
            &command,
            Some((observed_head.clone(), observed_exact.clone())),
        );
        let marker = AssetLifecycleMarker {
            command_id: command.command_id.clone(),
            command_kind: command.command_kind.clone(),
            command_digest: command.command_digest.clone(),
            state: AssetLifecycleState::Pending,
            base_generation: Some(observed_head.generation),
            base_head_checksum: Some(observed_head.checksum.clone()),
            base_publication: observed_head.publication_location.clone(),
            intended_publication: Some(
                store.publication_path(observed_head.generation + 1, &command.command_id),
            ),
        };
        store
            .create_asset_lifecycle_marker(&permit, "race-asset", serde_json::to_vec(&marker)?)
            .await?;
        let (_, marker_etag) = store
            .read_asset_lifecycle_marker("race-asset")
            .await?
            .expect("pending marker");

        // Writer A claims Publishing and pauses before creating its immutable
        // deletion publication. Resolver B must not convert that owned marker
        // to Stale.
        let intended = marker
            .intended_publication
            .as_deref()
            .expect("intended publication")
            .to_string();
        writer
            .begin_asset_marker_publication("race-asset", &attempt, &intended)
            .await?;
        let publishing = writer
            .finalize_asset_marker_stale_if_head_unchanged(
                "race-asset",
                marker.clone(),
                marker_etag.clone(),
                &observed_head,
                observed_exact.etag.as_deref(),
            )
            .await?;
        assert_eq!(publishing.state, AssetLifecycleState::Publishing);

        // The paused writer resumes and publishes because it owns the exact
        // lifecycle key.
        writer
            .publish_asset_deletion(
                "race-asset",
                &attempt,
                observed_head.next_generation(),
                &intended,
            )
            .await?;
        let (bytes, _) = store
            .read_asset_lifecycle_marker("race-asset")
            .await?
            .expect("marker after race");
        let current: AssetLifecycleMarker = serde_json::from_slice(&bytes)?;
        assert_eq!(current.state, AssetLifecycleState::Publishing);
        // Before the writer's explicit marker-finalization step, a reader
        // must still treat the authoritative deletion Head as deleted. This
        // prevents a reference publication from treating the Asset as live.
        assert!(writer.asset_is_deleted("race-asset").await?);
        writer
            .finalize_asset_marker_after_publication("race-asset", &attempt, &intended)
            .await?;
        let (bytes, _) = store
            .read_asset_lifecycle_marker("race-asset")
            .await?
            .expect("committed marker");
        let current: AssetLifecycleMarker = serde_json::from_slice(&bytes)?;
        assert_eq!(current.state, AssetLifecycleState::Committed);

        // Reverse ordering: once the marker is genuinely Stale, the old
        // deletion attempt cannot pass the marker guard and publish.
        let reverse = PublicationContext::with_command_digest(
            "asset-race-reverse",
            "asset.delete",
            "asset-race-reverse-digest",
        );
        let reverse_catalog =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(reverse.clone());
        let (reverse_head, reverse_exact) = reverse_catalog.exact_head().await?.expect("Head");
        let reverse_attempt = PublicationAttempt::from_exact(
            &reverse,
            Some((reverse_head.clone(), reverse_exact.clone())),
        );
        let reverse_marker = AssetLifecycleMarker {
            command_id: reverse.command_id.clone(),
            command_kind: reverse.command_kind.clone(),
            command_digest: reverse.command_digest.clone(),
            state: AssetLifecycleState::Pending,
            base_generation: Some(reverse_head.generation),
            base_head_checksum: Some(reverse_head.checksum.clone()),
            base_publication: reverse_head.publication_location.clone(),
            intended_publication: Some(
                store.publication_path(reverse_head.generation + 1, &reverse.command_id),
            ),
        };
        store
            .create_asset_lifecycle_marker(
                &permit,
                "race-asset-reverse",
                serde_json::to_vec(&reverse_marker)?,
            )
            .await?;
        let (_, reverse_etag) = store
            .read_asset_lifecycle_marker("race-asset-reverse")
            .await?
            .expect("reverse marker");
        let stale = reverse_catalog
            .finalize_asset_marker_stale_if_head_unchanged(
                "race-asset-reverse",
                reverse_marker,
                reverse_etag,
                &reverse_head,
                reverse_exact.etag.as_deref(),
            )
            .await?;
        assert_eq!(stale.state, AssetLifecycleState::Stale);
        let error = reverse_catalog
            .publish_asset_deletion(
                "race-asset-reverse",
                &reverse_attempt,
                reverse_head.next_generation(),
                stale.intended_publication.as_deref().expect("intended"),
            )
            .await
            .expect_err("a stale asset writer cannot publish");
        assert!(error.to_string().contains("Catalog Head changed"));
        let (head_after, _) = reverse_catalog.exact_head().await?.expect("Head");
        assert_eq!(head_after, reverse_head);
        Ok(())
    }

    #[tokio::test]
    async fn publishing_command_receipt_can_resume_after_restart() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?;
        let store = SpaceCatalogStore::new(operator, "spaces/receipt-restart")?.single_process();
        let permit = store.mutation_permit()?;
        let space_id = SpaceId::from(Uuid::from_u128(18_525));
        publish_test_generation(&store, space_id, "receipt-restart-base").await?;

        let command = PublicationContext::with_command_digest(
            "receipt-restart-absent",
            "test.receipt-restart",
            "receipt-restart-absent-digest",
        );
        let writer =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(command.clone());
        let (base_head, base_exact) = writer.exact_head().await?.expect("base Head");
        let attempt =
            PublicationAttempt::from_exact(&command, Some((base_head.clone(), base_exact.clone())));
        let intended = store.publication_path(base_head.generation + 1, &command.command_id);
        store
            .create_command_receipt(
                &permit,
                &command.command_id,
                serde_json::to_vec(&CommandReceiptRecord::pending(&attempt, intended.clone()))?,
            )
            .await?;
        writer
            .begin_command_publication(&attempt, &intended)
            .await?;

        // A different command body cannot steal the live exact-key owner.
        let intruder = SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(
            PublicationContext::with_command_digest(
                command.command_id.clone(),
                command.command_kind.clone(),
                "different-digest",
            ),
        );
        assert!(intruder.claim_command_receipt().await.is_err());

        // The original writer is gone. A fresh coordinator sees the durable
        // Publishing attempt, claims the same exact identity, and resumes it
        // without requiring an unrelated Head mutation.
        drop(writer);
        let restarted =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(command.clone());
        assert!(restarted.publication_receipt(&command).await?.is_none());
        restarted.claim_command_receipt().await?;
        let restarted_attempt = restarted.publication_attempt().await?;
        let restarted_head = restarted_attempt.expected_head.clone().expect("base Head");
        restarted
            .publish_new_head(
                &restarted_attempt,
                restarted_head.next_generation(),
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: restarted_head.namespace.clone(),
                        table: "_receipt_restart_absent".to_string(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: "test://receipt-restart-absent".to_string(),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await?;
        assert!(restarted.publication_receipt(&command).await?.is_some());

        // A second restart covers the case where the immutable publication
        // already exists but its Head CAS was interrupted. The resumed writer
        // must validate and reuse the exact same bytes rather than create a
        // different publication at the same command path.
        let existing = PublicationContext::with_command_digest(
            "receipt-restart-existing",
            "test.receipt-restart",
            "receipt-restart-existing-digest",
        );
        let existing_writer =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(existing.clone());
        let existing_attempt = existing_writer.publication_attempt().await?;
        existing_writer.claim_command_receipt().await?;
        let existing_head = existing_attempt.expected_head.clone().expect("Head");
        let existing_intended =
            store.publication_path(existing_head.generation + 1, &existing.command_id);
        existing_writer
            .begin_command_publication(&existing_attempt, &existing_intended)
            .await?;
        write_asset_publication_without_head_cas(
            &existing_writer,
            &existing_attempt,
            "receipt-restart-existing",
        )
        .await?;
        drop(existing_writer);

        let restarted_existing =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(existing.clone());
        let recovered = restarted_existing
            .publication_receipt(&existing)
            .await?
            .expect("existing immutable publication must be adopted");
        assert_eq!(recovered.catalog_generation, existing_head.generation + 1);
        Ok(())
    }

    #[tokio::test]
    async fn publishing_asset_marker_can_resume_after_restart_with_partial_receipt() -> AnyResult<()>
    {
        let operator = Operator::new(Memory::default())?;
        let store = SpaceCatalogStore::new(operator, "spaces/asset-restart")?.single_process();
        let space_id = SpaceId::from(Uuid::from_u128(18_526));
        publish_test_generation(&store, space_id, "asset-restart-base").await?;

        let pending_receipt_command = PublicationContext::with_command_digest(
            "asset-restart-pending-receipt",
            "asset.delete",
            "asset-restart-pending-receipt-digest",
        );
        let writer = SpaceCatalog::new(store.clone(), space_id)?
            .with_publication_context(pending_receipt_command.clone());
        let attempt = writer.publication_attempt().await?;
        writer.claim_command_receipt().await?;
        let head = attempt.expected_head.clone().expect("base Head");
        let intended =
            store.publication_path(head.generation + 1, &pending_receipt_command.command_id);
        let marker = writer.pending_asset_marker(&attempt, intended.clone());
        writer
            .create_asset_marker("asset-restart-pending", &marker)
            .await?;
        writer
            .begin_asset_marker_publication("asset-restart-pending", &attempt, &intended)
            .await?;
        drop(writer);

        let restarted = SpaceCatalog::new(store.clone(), space_id)?
            .with_publication_context(pending_receipt_command.clone());
        assert!(restarted
            .asset_is_deleted("asset-restart-pending")
            .await
            .expect_err("Publishing remains a fail-closed read barrier")
            .to_string()
            .contains("still in progress"));
        restarted.claim_command_receipt().await?;
        restarted
            .mark_asset_deleted("asset-restart-pending")
            .await?;
        assert!(restarted.asset_is_deleted("asset-restart-pending").await?);

        let stale_receipt_command = PublicationContext::with_command_digest(
            "asset-restart-stale-receipt",
            "asset.delete",
            "asset-restart-stale-receipt-digest",
        );
        let writer = SpaceCatalog::new(store.clone(), space_id)?
            .with_publication_context(stale_receipt_command.clone());
        let attempt = writer.publication_attempt().await?;
        writer.claim_command_receipt().await?;
        let head = attempt.expected_head.clone().expect("base Head");
        let intended =
            store.publication_path(head.generation + 1, &stale_receipt_command.command_id);
        let marker = writer.pending_asset_marker(&attempt, intended.clone());
        writer
            .create_asset_marker("asset-restart-stale", &marker)
            .await?;
        writer
            .begin_asset_marker_publication("asset-restart-stale", &attempt, &intended)
            .await?;
        let (bytes, etag) = store
            .read_command_receipt(&stale_receipt_command.command_id)
            .await?
            .expect("Pending command receipt");
        let receipt: CommandReceiptRecord = serde_json::from_slice(&bytes)?;
        writer
            .finalize_command_receipt(receipt, etag, CommandReceiptState::Stale, None, None)
            .await?;
        drop(writer);

        // The marker is still Publishing, but the command receipt was only
        // partially claimed and became Stale. A fresh coordinator reclaims
        // the receipt and resumes the marker without an unrelated mutation.
        let restarted = SpaceCatalog::new(store.clone(), space_id)?
            .with_publication_context(stale_receipt_command.clone());
        assert!(restarted
            .asset_is_deleted("asset-restart-stale")
            .await
            .expect_err("Publishing remains a fail-closed read barrier")
            .to_string()
            .contains("still in progress"));
        restarted.claim_command_receipt().await?;
        restarted.mark_asset_deleted("asset-restart-stale").await?;
        assert!(restarted.asset_is_deleted("asset-restart-stale").await?);
        Ok(())
    }

    async fn write_asset_publication_without_head_cas(
        catalog: &SpaceCatalog,
        attempt: &PublicationAttempt,
        asset_id: &str,
    ) -> AnyResult<()> {
        let head = attempt.expected_head.as_ref().expect("test Head");
        let mut next = head.next_generation();
        let publication_path = catalog
            .store
            .publication_path(next.generation, &attempt.publication.command_id);
        next.publication_location = Some(publication_path);
        next.publication_command_id = Some(attempt.publication.command_id.clone());
        next.checksum = head_checksum(&next)?;
        let mut publication = PublicationRecord {
            generation: next.generation,
            previous_generation: attempt.expected_generation,
            previous_publication: attempt.expected_previous_publication.clone(),
            previous_head_checksum: attempt.expected_head_checksum.clone(),
            command_id: attempt.publication.command_id.clone(),
            command_kind: attempt.publication.command_kind.clone(),
            command_digest: attempt.publication.command_digest.clone(),
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
            next_head_checksum: next.checksum.clone(),
            next_head: next,
            checksum: String::new(),
        };
        publication.checksum = publication_checksum(&publication)?;
        catalog.write_publication(&publication).await?;
        Ok(())
    }

    #[tokio::test]
    async fn asset_lifecycle_recovery_is_subordinate_to_the_authoritative_head() -> AnyResult<()> {
        let operator = Operator::new(Memory::default())?;
        let store = SpaceCatalogStore::new(operator, "spaces/asset-recovery")?.single_process();
        let permit = store.mutation_permit()?;
        let space_id = SpaceId::from(Uuid::from_u128(18_520));
        let catalog = SpaceCatalog::new(store.clone(), space_id)?;
        let namespace = catalog.namespace().clone();
        catalog
            .create_table(
                &namespace,
                TableCreation::builder()
                    .name("form_asset_recovery".to_string())
                    .location(logical_test_location(space_id, "forms/form"))
                    .schema(
                        iceberg::spec::Schema::builder()
                            .with_fields(vec![])
                            .build()?,
                    )
                    .build(),
            )
            .await?;

        // Marker exists before a publication is written: recovery keeps the
        // bytes and references available because the Head still proves the
        // old state.
        let base = SpaceCatalog::new(store.clone(), space_id)?;
        let (base_head, base_exact) = base.exact_head().await?.expect("base Head");
        let before_publication = PublicationContext::with_command_digest(
            "asset-recovery-before-publication",
            "asset.delete",
            "asset-recovery-before-publication-digest",
        );
        base.store
            .create_asset_lifecycle_marker(
                &permit,
                "before-publication",
                asset_marker_for(&base, &before_publication, &base_head)?,
            )
            .await?;
        let reopened = SpaceCatalog::new(store.clone(), space_id)?;
        assert!(!reopened.asset_is_deleted("before-publication").await?);

        // A publication object without its Head CAS is also pending, not a
        // deletion authority.
        let before_head_cas = PublicationContext::with_command_digest(
            "asset-recovery-before-head-cas",
            "asset.delete",
            "asset-recovery-before-head-cas-digest",
        );
        base.store
            .create_asset_lifecycle_marker(
                &permit,
                "before-head-cas",
                asset_marker_for(&base, &before_head_cas, &base_head)?,
            )
            .await?;
        let publication_catalog = SpaceCatalog::new(store.clone(), space_id)?;
        let publication_attempt = PublicationAttempt::from_exact(
            &before_head_cas,
            Some((base_head.clone(), base_exact.clone())),
        );
        write_asset_publication_without_head_cas(
            &publication_catalog,
            &publication_attempt,
            "before-head-cas",
        )
        .await?;
        assert!(!reopened.asset_is_deleted("before-head-cas").await?);

        // Once the deletion publication reaches Head, a restart can still
        // observe the committed lifecycle state and finish physical cleanup.
        let committed = PublicationContext::with_command_digest(
            "asset-recovery-committed",
            "asset.delete",
            "asset-recovery-committed-digest",
        );
        base.store
            .create_asset_lifecycle_marker(
                &permit,
                "committed",
                asset_marker_for(&base, &committed, &base_head)?,
            )
            .await?;
        let committed_catalog =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(committed);
        committed_catalog.mark_asset_deleted("committed").await?;
        let restarted = SpaceCatalog::new(store.clone(), space_id)?;
        assert!(restarted.asset_is_deleted("committed").await?);

        // A competing reference/publication winning the Head CAS makes the
        // deletion marker stale. The next reader logically reclaims it and
        // keeps bytes available instead of inheriting a second authority;
        // the inert marker slot remains so reclaim never races a new claim.
        let competing = PublicationContext::with_command_digest(
            "asset-recovery-reference-wins",
            "entry.append",
            "asset-recovery-reference-wins-digest",
        );
        let reference_base_catalog = SpaceCatalog::new(store.clone(), space_id)?;
        let (reference_base, _) = reference_base_catalog
            .exact_head()
            .await?
            .expect("reference race base Head");
        base.store
            .create_asset_lifecycle_marker(
                &permit,
                "reference-wins",
                asset_marker_for(
                    &reference_base_catalog,
                    &before_publication,
                    &reference_base,
                )?,
            )
            .await?;
        let winner =
            SpaceCatalog::new(store.clone(), space_id)?.with_publication_context(competing.clone());
        let winner_attempt = winner.publication_attempt().await?;
        winner
            .publish_new_head(
                &winner_attempt,
                reference_base.next_generation(),
                PublicationUpdate {
                    affected_table: TableCoordinates {
                        namespace: reference_base.namespace.clone(),
                        table: "form_asset_recovery".to_string(),
                    },
                    base_metadata_location: None,
                    new_metadata_location: "asset://reference-wins".to_string(),
                    base_snapshot_id: None,
                    base_schema_id: None,
                    new_snapshot_id: None,
                    new_schema_id: 0,
                },
            )
            .await?;
        assert!(!restarted.asset_is_deleted("reference-wins").await?);
        assert!(restarted.asset_marker("reference-wins").await?.is_some());
        Ok(())
    }
}
