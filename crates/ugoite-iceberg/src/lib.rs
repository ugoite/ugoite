//! Iceberg-native persistence and query boundary.
//!
//! One [`IcebergWorkspace`] represents one Ugoite Space namespace. Production
//! callers inject a durable Catalog; every built-in test workspace uses the
//! same OpenDAL-backed SpaceCatalog boundary as production.

#![recursion_limit = "512"]

pub mod derived_relation;
mod logical_storage;
mod read_schema_provider;
mod space_catalog;

pub mod asset;
pub mod audit;
pub mod authorization;
pub mod entry;
pub mod form;
pub mod health;
pub mod iceberg_store;
pub mod index;
pub mod integrity;
pub mod preferences;
pub mod query_context;
pub mod sample_data;
pub mod saved_sql;
pub mod search;
pub mod service;
pub mod space;
pub mod sql_session;

pub use health::SpaceHealthReport;
use space_catalog::SpaceCatalog;
pub use space_catalog::{PublicationContext, PublishedChange};
pub use ugoite_domain::checkpoint::{
    CheckpointChange, CheckpointChangeKind, CheckpointDiff, CheckpointTable, SpaceCheckpoint,
};
pub use ugoite_domain::publication_ref::PublicationRef;

use anyhow::{anyhow, bail, Context, Result};
use arrow_array::builder::{
    BooleanBuilder, Date32Builder, FixedSizeBinaryBuilder, Float32Builder, Float64Builder,
    Int32Builder, Int64Builder, LargeBinaryBuilder, ListBuilder, StringBuilder, StructBuilder,
    Time64MicrosecondBuilder, TimestampMicrosecondBuilder, TimestampNanosecondBuilder,
};
use arrow_array::{
    Array, ArrayRef, BooleanArray, Date32Array, FixedSizeBinaryArray, Float32Array, Float64Array,
    Int32Array, Int64Array, LargeBinaryArray, ListArray, RecordBatch, StringArray, StructArray,
    Time64MicrosecondArray, TimestampMicrosecondArray, TimestampNanosecondArray,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use datafusion::execution::context::SessionContext;
use datafusion::logical_expr::expr_fn::ident;
use datafusion::prelude::{col, lit};
use iceberg::spec::DataFileFormat;
use iceberg::spec::{
    ListType, NestedField, PrimitiveType, Schema, SortOrder, StructType, Type, UnboundPartitionSpec,
};
use iceberg::transaction::{AddColumn, ApplyTransactionAction, Transaction};
use iceberg::writer::base_writer::data_file_writer::DataFileWriterBuilder;
use iceberg::writer::file_writer::location_generator::{
    DefaultFileNameGenerator, DefaultLocationGenerator,
};
use iceberg::writer::file_writer::rolling_writer::RollingFileWriterBuilder;
use iceberg::writer::file_writer::ParquetWriterBuilder;
use iceberg::writer::{IcebergWriter, IcebergWriterBuilder};
use iceberg::{Catalog, NamespaceIdent, TableCreation, TableIdent};
use iceberg_datafusion::{IcebergCatalogProvider, IcebergStaticTableProvider};
use opendal::options::ReadOptions;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Duration;
#[cfg(debug_assertions)]
use tokio::sync::Notify;
use tokio::sync::Semaphore;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
};
use ugoite_domain::change::{
    selective_inverse_with_form_schema, ChangeCommand, ChangeDescriptor, RevertFieldAction,
};
use ugoite_domain::entry::{
    AssetReference, EntryIntegrity, EntryMetadata, EntryOperation, EntryRevision, FieldValue,
    RevisionError,
};
use ugoite_domain::form::{
    sql_column_name, sql_relation_name, Compatibility, FieldType, FormChange, FormChangeSet,
    FormDefinition, FormField, ListItemDefinition,
};
use ugoite_domain::id::{validate_checkpoint_name, FormId, RevisionId, SpaceId};

use crate::logical_storage::{logical_space_uid, logical_uri};
use ugoite_storage::{is_local_operator, operator_from_uri, SpaceCatalogStore};
use uuid::Uuid;

pub(crate) fn is_shared_backend(operator: &Operator) -> bool {
    !is_local_operator(operator)
}

/// Read an object against the exact version observed by `stat`.
///
/// Shared object stores must never fall back to an unconditional read after a
/// missing ETag: the caller would otherwise be able to combine metadata from
/// different revisions while believing it read one snapshot.
pub(crate) async fn read_object_exact_optional(
    operator: &Operator,
    path: &str,
) -> Result<Option<Vec<u8>>> {
    Ok(read_object_exact_optional_with_etag(operator, path)
        .await?
        .map(|(bytes, _)| bytes))
}

pub(crate) async fn read_object_exact_optional_with_etag(
    operator: &Operator,
    path: &str,
) -> Result<Option<(Vec<u8>, Option<String>)>> {
    let metadata = match operator.stat(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == opendal::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let etag = metadata
        .etag()
        .filter(|etag| !etag.is_empty())
        .map(str::to_owned);
    let bytes = match etag.as_deref() {
        Some(etag) => {
            operator
                .read_options(
                    path,
                    ReadOptions {
                        if_match: Some(etag.to_string()),
                        ..Default::default()
                    },
                )
                .await?
        }
        None if !is_shared_backend(operator) => operator.read(path).await?,
        None => return Err(anyhow!("exact read requires an ETag: {path}")),
    };
    Ok(Some((bytes.to_vec(), etag)))
}

pub(crate) async fn read_object_exact(operator: &Operator, path: &str) -> Result<Vec<u8>> {
    read_object_exact_optional(operator, path)
        .await?
        .ok_or_else(|| anyhow!("object not found: {path}"))
}

const FORM_DEFINITION_PROPERTY: &str = "ugoite.form.definition.v1";
const FORM_HISTORY_PROPERTY: &str = "ugoite.form.history.v1";
const FORM_ID_PROPERTY: &str = "ugoite.form.id";
pub(crate) const FORM_NAME_PROPERTY: &str = "ugoite.form.name";
const FORM_VERSION_PROPERTY: &str = "ugoite.form.version";
const TARGET_FILE_SIZE_PROPERTY: &str = "write.target-file-size-bytes";
const FIRST_FORM_FIELD_ID: i32 = 100;
const NESTED_FIELD_ID_BASE: i32 = 1_000_000;

fn unsupported_form_field_type_change(
    current: &FormDefinition,
    changes: &FormChangeSet,
) -> Result<AppError> {
    changes
        .changes
        .iter()
        .find_map(|change| {
            let FormChange::ChangeFieldType {
                field_id,
                field_type,
            } = change
            else {
                return None;
            };
            let source = current.fields.iter().find(|field| field.id == *field_id)?;
            Some(AppError::form_field_type_change_not_supported(
                &source.name,
                source.field_type.as_str(),
                field_type.as_str(),
            ))
        })
        .context("breaking Form compatibility did not contain a field type change")
}

fn invalid_revision_input(message: impl Into<String>) -> anyhow::Error {
    AppError::invalid_input(ErrorCode::InvalidInput, message).into()
}

fn validate_revision_payload(form: &FormDefinition, revision: &EntryRevision) -> Result<()> {
    revision.validate_payload(form).map_err(|error| {
        let field_id = match error {
            RevisionError::RequiredField(field_id)
            | RevisionError::UnknownField(field_id)
            | RevisionError::WrongType(field_id)
            | RevisionError::InvalidAssetReference(field_id)
            | RevisionError::DuplicateAssetReference(field_id) => Some(field_id),
            _ => None,
        };
        let field = field_id.and_then(|field_id| {
            form.fields
                .iter()
                .find(|candidate| candidate.id == field_id)
                .map(|candidate| candidate.name.clone())
        });
        let message = match error {
            RevisionError::RequiredField(_) => "Required field is missing",
            RevisionError::UnknownField(_) => "Field is not defined on this Form",
            RevisionError::WrongType(_) => "Value has the wrong type for this field",
            RevisionError::InvalidAssetReference(_) => "Asset reference metadata is invalid",
            RevisionError::DuplicateAssetReference(_) => {
                "The same asset is referenced more than once in this list"
            }
            RevisionError::TooManyAssetReferences => "Entry contains too many AssetReferences",
            RevisionError::AssetReferenceMetadataTooLarge => {
                "Entry AssetReference metadata exceeds the size limit"
            }
            _ => "Entry revision payload is not valid for this Form",
        };
        invalid_revision_input(format!(
            "Form validation failed: {}",
            serde_json::json!([{"field": field, "message": message}])
        ))
    })
}

/// A durable checkpoint or one of its immutable targets cannot be resolved.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CheckpointUnavailable {
    target: String,
}

impl CheckpointUnavailable {
    fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
        }
    }
}

impl std::fmt::Display for CheckpointUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "checkpoint unavailable: {}", self.target)
    }
}

impl std::error::Error for CheckpointUnavailable {}

/// A checkpoint's coordinate checksum or immutable metadata does not match.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CheckpointIntegrityError {
    detail: String,
}

impl CheckpointIntegrityError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for CheckpointIntegrityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "checkpoint integrity error: {}", self.detail)
    }
}

impl std::error::Error for CheckpointIntegrityError {}

#[derive(Debug, Clone)]
pub struct IcebergWorkspace {
    catalog: Arc<dyn Catalog>,
    space_catalog: Option<Arc<SpaceCatalog>>,
    namespace: NamespaceIdent,
    space_id: SpaceId,
    logical_space_uid: uuid::Uuid,
    warehouse: String,
    write: WriteConfig,
}

impl IcebergWorkspace {
    /// Read the exact active Pin set owned by the Space Catalog Head.
    pub async fn list_pins(
        &self,
    ) -> Result<std::collections::BTreeMap<String, ugoite_domain::pin::PinEntry>> {
        self.space_catalog
            .as_ref()
            .context("Pin operations require the OpenDAL-backed SpaceCatalog")?
            .list_pins()
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    /// Returns the exact current publication coordinate.  The coordinate is
    /// portable and may be retained by a caller or stored in a Pin; it is not
    /// a copy of the Catalog Head or of any table metadata.
    pub async fn current_publication(&self) -> Result<PublicationRef> {
        self.space_catalog
            .as_ref()
            .context("Publication selection requires the OpenDAL-backed SpaceCatalog")?
            .current_publication()
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    /// Resolves an active Head-owned Pin to its immutable publication
    /// coordinate.  Publication resolution subsequently verifies that the
    /// coordinate is still reachable from the current Head.
    pub async fn resolve_pin(&self, name: &str) -> Result<PublicationRef> {
        let pin = match self
            .space_catalog
            .as_ref()
            .context("Pin selection requires the OpenDAL-backed SpaceCatalog")?
            .get_pin(name)
            .await
        {
            Ok(pin) => pin,
            Err(error) => {
                let detail = error.to_string();
                if detail.contains("pin not found")
                    || detail.contains("publication target unavailable")
                    || detail.to_ascii_lowercase().contains("not found")
                {
                    return Err(CheckpointUnavailable::new("named Pin").into());
                }
                if error.kind() == iceberg::ErrorKind::DataInvalid {
                    return Err(CheckpointIntegrityError::new(detail).into());
                }
                return Err(anyhow!(detail));
            }
        };
        Ok(pin.coordinate)
    }

    /// Resolves a PublicationRef to an in-memory immutable view.  This value
    /// is deliberately not persisted; it is an adapter-local description used
    /// while the publication-selected read is executing.
    pub async fn resolve_publication(
        &self,
        publication: &PublicationRef,
    ) -> Result<SpaceCheckpoint> {
        self.space_catalog
            .as_ref()
            .context("Publication selection requires the OpenDAL-backed SpaceCatalog")?
            .resolve_publication_checkpoint(publication)
            .await
    }

    /// Reads an immutable revision view selected by a PublicationRef.  The
    /// reference is resolved through the current authoritative chain before
    /// any Iceberg metadata is opened.
    pub async fn read_revision_view_at_publication(
        &self,
        publication: &PublicationRef,
        form_id: FormId,
        view: RevisionView,
    ) -> Result<Vec<EntryRevision>> {
        let checkpoint = self.resolve_publication(publication).await?;
        self.read_revision_view_at_checkpoint(&checkpoint, form_id, view)
            .await
    }

    pub async fn read_revision_view_at_publication_with_scope(
        &self,
        publication: &PublicationRef,
        form_id: FormId,
        entry_scope: EntryScope,
        view: RevisionView,
    ) -> Result<Vec<EntryRevision>> {
        let checkpoint = self.resolve_publication(publication).await?;
        self.read_revision_view_at_checkpoint_with_scope(&checkpoint, form_id, entry_scope, view)
            .await
    }

    pub async fn forms_at_publication(
        &self,
        publication: &PublicationRef,
    ) -> Result<Vec<FormDefinition>> {
        let checkpoint = self.resolve_publication(publication).await?;
        self.forms_at_checkpoint(&checkpoint).await
    }

    pub async fn form_at_publication(
        &self,
        publication: &PublicationRef,
        relation: &str,
    ) -> Result<FormDefinition> {
        let checkpoint = self.resolve_publication(publication).await?;
        self.form_at_checkpoint(&checkpoint, relation).await
    }

    pub async fn form_history_at_publication(
        &self,
        publication: &PublicationRef,
        form_id: FormId,
    ) -> Result<Vec<FormDefinition>> {
        let checkpoint = self.resolve_publication(publication).await?;
        self.form_history_at_checkpoint(&checkpoint, form_id).await
    }

    /// Compares logical Entry revisions selected by two immutable publication
    /// coordinates.  Neither coordinate is allowed to select a detached or
    /// corrupt publication.
    pub async fn diff_publications(
        &self,
        from: &PublicationRef,
        to: &PublicationRef,
    ) -> Result<CheckpointDiff> {
        self.diff_publications_with_scopes(from, to, None).await
    }

    pub async fn diff_publications_with_scopes(
        &self,
        from: &PublicationRef,
        to: &PublicationRef,
        form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
    ) -> Result<CheckpointDiff> {
        let from = self.resolve_publication(from).await?;
        let to = self.resolve_publication(to).await?;
        self.diff_checkpoints_with_scopes(&from, &to, form_scopes)
            .await
    }

    pub async fn list_changes(&self) -> Result<Vec<space_catalog::PublishedChange>> {
        self.space_catalog
            .as_ref()
            .context("Change history requires the OpenDAL-backed SpaceCatalog")?
            .list_changes()
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    /// Create a maintenance Pin by publishing a Head-only transition.
    pub async fn create_pin(
        &self,
        name: &str,
        created_by_principal_id: &str,
        created_at_micros: i64,
        command_id: &str,
    ) -> Result<ugoite_domain::pin::PinEntry> {
        self.space_catalog
            .as_ref()
            .context("Pin operations require the OpenDAL-backed SpaceCatalog")?
            .new_attempt()
            .create_pin(name, created_by_principal_id, created_at_micros, command_id)
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }

    pub async fn delete_pin(&self, name: &str, command_id: &str) -> Result<()> {
        self.space_catalog
            .as_ref()
            .context("Pin operations require the OpenDAL-backed SpaceCatalog")?
            .new_attempt()
            .delete_pin(name, command_id)
            .await
            .map_err(|error| anyhow!(error.to_string()))
    }
}

/// Query permits are process-wide per Space coordinate. A request creates a
/// short-lived authorization context, but it must not thereby create a fresh
/// production concurrency budget.
static SPACE_QUERY_PERMITS: LazyLock<Mutex<HashMap<String, Weak<Semaphore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Derived rebuilds have their own maintenance budget. They may be expensive,
/// but they must not consume the permit reserved for interactive authorized
/// reads.
static SPACE_MAINTENANCE_QUERY_PERMITS: LazyLock<Mutex<HashMap<String, Weak<Semaphore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

const MAX_SPACE_PERMIT_KEYS: usize = 1024;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SchemaCommitCapability {
    MetadataOnly,
    AtomicSchemaEvolution,
}

/// The reusable logical views over one append-only Form revision table.
/// `LatestIncludingTombstones` deliberately retains delete revisions so a
/// caller can distinguish an absent Entry from a deleted one.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RevisionView {
    All,
    LatestIncludingTombstones,
    Current,
}

/// Normal current-state reads are deliberately bounded. History remains an
/// explicit, separate operation and may materialize its complete revision
/// stream.
pub const MAX_NORMAL_READ_ROWS: usize = 10_000;
const DERIVED_REVISION_PAGE_SIZE: usize = 2_048;

#[derive(Debug, Clone, Copy)]
pub struct WriteConfig {
    pub target_file_size_bytes: u64,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            target_file_size_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct CommitReceipt {
    pub command_id: String,
    pub catalog_generation: u64,
    pub snapshot_id: i64,
    pub committed_revision_ids: Vec<RevisionId>,
    pub committed_at_micros: i64,
    pub data_file_count: usize,
}

/// The only production entry point for changes that publish a Space Catalog
/// Head.  A coordinator owns one immutable domain command identity.  It
/// creates a fresh, short-lived `SpaceCatalog` only for the command attempt;
/// no publication state is ambient or shared between commands.
#[derive(Debug, Clone)]
pub struct SpaceCommitCoordinator {
    workspace: IcebergWorkspace,
    publication: PublicationContext,
    #[cfg(debug_assertions)]
    validation_gate: Option<Arc<TestValidationGate>>,
}

/// Debug-only synchronization used by the deterministic publication race
/// tests. It is absent from release builds and cannot affect production
/// scheduling or persistence.
#[cfg(debug_assertions)]
#[doc(hidden)]
#[derive(Debug)]
pub struct TestValidationGate {
    reached: std::sync::atomic::AtomicBool,
    entered: Notify,
    release: Notify,
}

#[cfg(debug_assertions)]
impl TestValidationGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            reached: std::sync::atomic::AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        })
    }

    pub async fn wait_until_entered(&self) {
        while !self.reached.load(std::sync::atomic::Ordering::Acquire) {
            self.entered.notified().await;
        }
    }

    pub fn release(&self) {
        self.release.notify_one();
    }

    async fn pause(&self) {
        self.reached
            .store(true, std::sync::atomic::Ordering::Release);
        self.entered.notify_waiters();
        self.release.notified().await;
    }
}

#[cfg(debug_assertions)]
static TEST_VALIDATION_GATE: LazyLock<Mutex<Option<Arc<TestValidationGate>>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn install_test_validation_gate(gate: Arc<TestValidationGate>) {
    *TEST_VALIDATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(gate);
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn clear_test_validation_gate() {
    *TEST_VALIDATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(debug_assertions)]
fn current_test_validation_gate() -> Option<Arc<TestValidationGate>> {
    TEST_VALIDATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Debug-only synchronization for proving recovery after an immutable
/// publication has been written but before its Catalog Head CAS. It is not
/// compiled into release builds and exposes no production mutation bypass.
#[cfg(debug_assertions)]
#[doc(hidden)]
#[derive(Debug)]
pub struct TestPublicationGate {
    reached: std::sync::atomic::AtomicBool,
    entered: Notify,
    release: Notify,
}

#[cfg(debug_assertions)]
impl TestPublicationGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            reached: std::sync::atomic::AtomicBool::new(false),
            entered: Notify::new(),
            release: Notify::new(),
        })
    }

    pub async fn wait_until_entered(&self) {
        while !self.reached.load(std::sync::atomic::Ordering::Acquire) {
            self.entered.notified().await;
        }
    }

    pub fn release(&self) {
        self.release.notify_one();
    }

    async fn pause(&self) {
        self.reached
            .store(true, std::sync::atomic::Ordering::Release);
        self.entered.notify_waiters();
        self.release.notified().await;
    }
}

#[cfg(debug_assertions)]
static TEST_PUBLICATION_GATE: LazyLock<Mutex<Option<Arc<TestPublicationGate>>>> =
    LazyLock::new(|| Mutex::new(None));

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn install_test_publication_gate(gate: Arc<TestPublicationGate>) {
    *TEST_PUBLICATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(gate);
}

#[cfg(debug_assertions)]
#[doc(hidden)]
pub fn clear_test_publication_gate() {
    *TEST_PUBLICATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

#[cfg(debug_assertions)]
fn current_test_publication_gate() -> Option<Arc<TestPublicationGate>> {
    TEST_PUBLICATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

const MAX_PUBLICATION_ATTEMPTS: usize = 3;

/// Builds the immutable identity carried by one domain command. Callers pass
/// domain values only; canonical JSON is hashed before any physical Iceberg
/// conversion happens.
pub fn publication_context<T: Serialize>(
    command_id: impl Into<String>,
    command_kind: impl Into<String>,
    command: &T,
) -> Result<PublicationContext> {
    let digest = hex::encode(Sha256::digest(serde_json::to_vec(command)?));
    Ok(PublicationContext::with_command_digest(
        command_id,
        command_kind,
        digest,
    ))
}

/// Build a publication context from a validated semantic Change. The
/// publication command identity is derived from the Change ID in one place,
/// so adapters cannot accidentally create two identities for one mutation.
pub fn publication_context_for_change<T: Serialize>(
    command: &ChangeCommand,
    command_kind: impl Into<String>,
    payload: &T,
) -> Result<PublicationContext> {
    command.validate()?;
    publication_context(command.change_id.clone(), command_kind, payload)?
        .with_change_descriptor(command.descriptor())
        .map_err(|error| anyhow!(error.to_string()))
}

pub(crate) fn system_publication_context<T: Serialize>(
    command_id: impl Into<String>,
    command_kind: impl Into<String>,
    payload: &T,
) -> Result<PublicationContext> {
    let context = publication_context(command_id, command_kind, payload)?;
    context
        .with_change_descriptor(ChangeDescriptor {
            run_id: None,
            actor_principal_id: "system".to_owned(),
            message: None,
            reverts_change_id: None,
            created_at_micros: chrono::Utc::now().timestamp_micros(),
        })
        .map_err(|error| anyhow!(error.to_string()))
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct MaintenancePlan {
    pub form_id: FormId,
    pub small_file_count: usize,
    pub rewrite_data_files: bool,
    pub rewrite_manifests: bool,
    pub expire_snapshots: bool,
    pub remove_orphans: bool,
    pub refresh_statistics: bool,
}

impl IcebergWorkspace {
    pub(crate) fn shared_query_permits(&self, max_concurrency: usize) -> Arc<Semaphore> {
        let key = format!("{}:{}", self.warehouse, self.space_id);
        let mut permits = SPACE_QUERY_PERMITS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        permits.retain(|_, permit| permit.strong_count() > 0);
        if let Some(permit) = permits.get(&key).and_then(Weak::upgrade) {
            return permit;
        }
        let permit = Arc::new(Semaphore::new(max_concurrency));
        // Weak entries avoid retaining a semaphore forever, while this cap
        // also bounds dead coordinate keys when a process sees many Spaces
        // and does not subsequently revisit them.
        if permits.len() < MAX_SPACE_PERMIT_KEYS {
            permits.insert(key, Arc::downgrade(&permit));
        }
        permit
    }

    pub(crate) fn maintenance_query_permits(&self, max_concurrency: usize) -> Arc<Semaphore> {
        let key = format!("{}:{}", self.warehouse, self.space_id);
        let mut permits = SPACE_MAINTENANCE_QUERY_PERMITS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        permits.retain(|_, permit| permit.strong_count() > 0);
        if let Some(permit) = permits.get(&key).and_then(Weak::upgrade) {
            return permit;
        }
        let permit = Arc::new(Semaphore::new(max_concurrency));
        if permits.len() < MAX_SPACE_PERMIT_KEYS {
            permits.insert(key, Arc::downgrade(&permit));
        }
        permit
    }

    pub async fn open_space(
        store: SpaceCatalogStore,
        space_id: SpaceId,
        write: WriteConfig,
    ) -> Result<Self> {
        let warehouse = store.warehouse_uri();
        Self::new_space_catalog(
            Arc::new(SpaceCatalog::new(store, space_id)?),
            space_id,
            warehouse,
            write,
        )
        .await
    }

    async fn new_space_catalog(
        catalog: Arc<SpaceCatalog>,
        space_id: SpaceId,
        warehouse: impl Into<String>,
        write: WriteConfig,
    ) -> Result<Self> {
        let namespace = namespace_for_space(space_id);
        if !catalog.namespace_exists(&namespace).await? {
            catalog.ensure_authoritative_mutation_contract()?;
            catalog.create_namespace(&namespace, HashMap::new()).await?;
        }
        Ok(Self {
            catalog: catalog.clone(),
            space_catalog: Some(catalog),
            namespace,
            space_id,
            logical_space_uid: logical_space_uid(space_id),
            warehouse: warehouse.into(),
            write,
        })
    }

    pub async fn memory_for_tests(space_id: SpaceId, warehouse: impl Into<String>) -> Result<Self> {
        let warehouse = warehouse.into();
        let store = SpaceCatalogStore::new(
            operator_from_uri(&warehouse)?,
            format!("test/space_{}", space_id.as_uuid().simple()),
        )?
        .single_process();
        let storage_warehouse = store.warehouse_uri();
        Self::new_space_catalog(
            Arc::new(SpaceCatalog::new(store, space_id)?),
            space_id,
            storage_warehouse,
            WriteConfig::default(),
        )
        .await
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn namespace_for_testing(&self) -> &NamespaceIdent {
        &self.namespace
    }

    /// Test-only physical inspection. Release builds expose no Catalog handle,
    /// so application code cannot bypass `SpaceCommitCoordinator` to commit.
    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn catalog_for_testing(&self) -> Arc<dyn Catalog> {
        self.catalog.clone()
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn clone_for_testing(&self) -> Self {
        self.clone()
    }

    fn mutation_catalog(&self) -> Arc<dyn Catalog> {
        self.catalog.clone()
    }

    /// Binds one stable domain command identity to a coordinator.  The
    /// coordinator is intentionally the only public mutation API; read APIs
    /// remain on `IcebergWorkspace`.
    pub fn commit(&self, publication: PublicationContext) -> Result<SpaceCommitCoordinator> {
        self.space_catalog
            .as_ref()
            .context("SpaceCommitCoordinator requires the OpenDAL-backed SpaceCatalog")?
            .ensure_authoritative_mutation_contract()?;
        publication
            .validate()
            .context("invalid publication Change contract")?;
        if self.space_catalog.is_none() {
            return Err(anyhow!(
                "SpaceCommitCoordinator requires the OpenDAL-backed SpaceCatalog"
            ));
        }
        Ok(SpaceCommitCoordinator {
            workspace: self.clone(),
            publication,
            #[cfg(debug_assertions)]
            validation_gate: current_test_validation_gate(),
        })
    }

    /// Captures one exact, checksum-protected Catalog Head and the immutable
    /// Iceberg coordinates reachable from it. This is read-only and never
    /// acquires a writer lock or starts a transaction.
    pub async fn capture_checkpoint(&self) -> Result<SpaceCheckpoint> {
        self.space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .capture_checkpoint()
            .await
    }

    /// Collects read-only evidence from the exact Catalog Head and the Iceberg
    /// metadata it references. It never scans table rows, lists objects, or
    /// performs a Catalog mutation.
    pub async fn health_report(&self, checkpoint_names: &[String]) -> Result<SpaceHealthReport> {
        for name in checkpoint_names {
            validate_checkpoint_name(name)?;
        }
        self.space_catalog
            .as_ref()
            .context("Space health requires the OpenDAL-backed SpaceCatalog")?
            .health_report(checkpoint_names)
            .await
    }

    /// Reads a revision view from checkpoint-recorded immutable metadata.
    /// Snapshot-bearing tables use Iceberg's static snapshot provider; a table
    /// with no snapshots still uses Iceberg's static metadata provider.
    pub async fn read_revision_view_at_checkpoint(
        &self,
        checkpoint: &SpaceCheckpoint,
        form_id: FormId,
        view: RevisionView,
    ) -> Result<Vec<EntryRevision>> {
        self.read_revision_view_at_checkpoint_with_scope(
            checkpoint,
            form_id,
            EntryScope::AllCurrent,
            view,
        )
        .await
    }

    /// Reads a checkpoint-pinned revision view after applying the trusted
    /// provider-side Entry scope. Full history is allowed here only because
    /// the caller supplies the scope before rows leave DataFusion.
    pub async fn read_revision_view_at_checkpoint_with_scope(
        &self,
        checkpoint: &SpaceCheckpoint,
        form_id: FormId,
        entry_scope: EntryScope,
        view: RevisionView,
    ) -> Result<Vec<EntryRevision>> {
        self.validate_checkpoint(checkpoint)?;
        let coordinate = checkpoint
            .tables
            .iter()
            .find(|coordinate| coordinate.form_id == form_id)
            .ok_or_else(|| CheckpointUnavailable::new(format!("Form {form_id}")))?;
        let table = self
            .space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .load_checkpoint_table(checkpoint, coordinate)
            .await?;
        let form = form_from_table(&table, form_id)?;
        self.read_revision_view_from_table(
            &form,
            table,
            entry_scope,
            view,
            coordinate.snapshot_id,
            Some(MAX_NORMAL_READ_ROWS),
        )
        .await
        .map_err(checkpoint_query_error)
    }

    /// Compares the latest logical revision at two immutable checkpoints.
    /// Iceberg revision IDs and payload rows define the result; manifest or
    /// data-file differences are intentionally not presented as domain events.
    pub async fn diff_checkpoints(
        &self,
        from: &SpaceCheckpoint,
        to: &SpaceCheckpoint,
    ) -> Result<CheckpointDiff> {
        self.diff_checkpoints_with_scopes(from, to, None).await
    }

    /// Authorized variant of [`Self::diff_checkpoints`]. The map is keyed by
    /// stable Form ID so a display-name rename cannot widen or lose the scope.
    pub async fn diff_checkpoints_with_scopes(
        &self,
        from: &SpaceCheckpoint,
        to: &SpaceCheckpoint,
        form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
    ) -> Result<CheckpointDiff> {
        self.validate_checkpoint(from)?;
        self.validate_checkpoint(to)?;
        let catalog = self
            .space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?;
        catalog.validate_checkpoint_evidence(from).await?;
        catalog.validate_checkpoint_evidence(to).await?;

        let form_ids = from
            .tables
            .iter()
            .chain(to.tables.iter())
            .map(|table| table.form_id)
            .collect::<BTreeSet<_>>();
        let mut changes = Vec::new();
        for form_id in form_ids {
            let scope = form_scopes
                .map(|scopes| {
                    scopes
                        .get(&form_id)
                        .cloned()
                        .unwrap_or_else(|| EntryScope::Only(BTreeSet::new()))
                })
                .unwrap_or(EntryScope::AllCurrent);
            let before = self
                .read_checkpoint_view_if_present(from, form_id, scope.clone())
                .await?;
            let after = self
                .read_checkpoint_view_if_present(to, form_id, scope)
                .await?;
            let before = before
                .into_iter()
                .map(|revision| (revision.entry_id, revision))
                .collect::<BTreeMap<_, _>>();
            let after = after
                .into_iter()
                .map(|revision| (revision.entry_id, revision))
                .collect::<BTreeMap<_, _>>();
            let entry_ids = before
                .keys()
                .chain(after.keys())
                .copied()
                .collect::<BTreeSet<_>>();
            for entry_id in entry_ids {
                let from_revision = before.get(&entry_id).cloned();
                let to_revision = after.get(&entry_id).cloned();
                if from_revision
                    .as_ref()
                    .zip(to_revision.as_ref())
                    .is_some_and(|(left, right)| left.revision_id == right.revision_id)
                {
                    continue;
                }
                let kind = match (&from_revision, &to_revision) {
                    (None, None) => unreachable!("entry ID came from one of the checkpoint views"),
                    (None, Some(revision)) if revision.operation == EntryOperation::Delete => {
                        CheckpointChangeKind::Deleted
                    }
                    (None, Some(_)) => CheckpointChangeKind::Added,
                    (Some(_), None) => CheckpointChangeKind::Deleted,
                    (Some(previous), Some(revision))
                        if revision.operation == EntryOperation::Delete =>
                    {
                        let _ = previous;
                        CheckpointChangeKind::Deleted
                    }
                    (Some(previous), Some(revision))
                        if revision.operation == EntryOperation::Restore
                            || (previous.operation == EntryOperation::Delete
                                && !revision.entry.deleted) =>
                    {
                        CheckpointChangeKind::Restored
                    }
                    (Some(_), Some(_)) => CheckpointChangeKind::Updated,
                };
                changes.push(CheckpointChange {
                    form_id,
                    entry_id,
                    kind,
                    from_revision_id: from_revision.as_ref().map(|revision| revision.revision_id),
                    to_revision_id: to_revision.as_ref().map(|revision| revision.revision_id),
                    from: from_revision,
                    to: to_revision,
                });
            }
        }
        Ok(CheckpointDiff {
            from_coordinate_checksum: from.coordinate_checksum.clone(),
            to_coordinate_checksum: to.coordinate_checksum.clone(),
            changes,
        })
    }

    async fn read_checkpoint_view_if_present(
        &self,
        checkpoint: &SpaceCheckpoint,
        form_id: FormId,
        entry_scope: EntryScope,
    ) -> Result<Vec<EntryRevision>> {
        if !checkpoint
            .tables
            .iter()
            .any(|coordinate| coordinate.form_id == form_id)
        {
            return Ok(Vec::new());
        }
        self.read_revision_view_at_checkpoint_with_scope(
            checkpoint,
            form_id,
            entry_scope,
            RevisionView::LatestIncludingTombstones,
        )
        .await
    }

    /// Loads Form definitions from the immutable tables named by one
    /// checkpoint. This deliberately never consults the live Form registry:
    /// callers that retain a checkpoint must retain its relation names and
    /// column surface as well.
    pub async fn forms_at_checkpoint(
        &self,
        checkpoint: &SpaceCheckpoint,
    ) -> Result<Vec<FormDefinition>> {
        self.validate_checkpoint(checkpoint)?;
        let catalog = self
            .space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?;
        catalog.validate_checkpoint_evidence(checkpoint).await?;
        let mut forms = Vec::with_capacity(checkpoint.tables.len());
        for coordinate in &checkpoint.tables {
            let table = catalog
                .load_checkpoint_table(checkpoint, coordinate)
                .await?;
            forms.push(form_from_table(&table, coordinate.form_id)?);
        }
        forms.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(forms)
    }

    /// Resolves exactly one Form from immutable checkpoint metadata. SQL
    /// session creation uses this after parsing the relation, rather than
    /// loading every Form definition or any Entry rows.
    pub async fn form_at_checkpoint(
        &self,
        checkpoint: &SpaceCheckpoint,
        relation: &str,
    ) -> Result<FormDefinition> {
        self.validate_checkpoint(checkpoint)?;
        let relation = relation.to_ascii_lowercase();
        let mut matches = checkpoint
            .tables
            .iter()
            .filter(|coordinate| sql_relation_name(coordinate.form_id) == relation);
        let coordinate = matches
            .next()
            .ok_or_else(|| CheckpointUnavailable::new(format!("Form relation {relation}")))?;
        if matches.next().is_some() {
            return Err(CheckpointIntegrityError::new(format!(
                "checkpoint contains multiple Forms for relation {relation}"
            ))
            .into());
        }
        let table = self
            .space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .load_checkpoint_table(checkpoint, coordinate)
            .await?;
        let form = form_from_table(&table, coordinate.form_id)?;
        if sql_relation_name(coordinate.form_id) != relation || coordinate.form_id != form.id {
            return Err(CheckpointIntegrityError::new(
                "checkpoint Form ID does not match immutable Iceberg metadata",
            )
            .into());
        }
        Ok(form)
    }

    pub async fn form_history_at_checkpoint(
        &self,
        checkpoint: &SpaceCheckpoint,
        form_id: FormId,
    ) -> Result<Vec<FormDefinition>> {
        self.validate_checkpoint(checkpoint)?;
        let coordinate = checkpoint
            .tables
            .iter()
            .find(|coordinate| coordinate.form_id == form_id)
            .ok_or_else(|| CheckpointUnavailable::new(format!("Form {form_id}")))?;
        let table = self
            .space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .load_checkpoint_table(checkpoint, coordinate)
            .await?;
        form_history_from_table(&table, form_id)
    }

    fn validate_checkpoint(&self, checkpoint: &SpaceCheckpoint) -> Result<()> {
        if checkpoint.space_id != self.space_id {
            return Err(
                CheckpointIntegrityError::new("Space ID does not match this workspace").into(),
            );
        }
        if !checkpoint.validate_coordinate_checksum() {
            return Err(CheckpointIntegrityError::new(
                "coordinate checksum or format version does not match",
            )
            .into());
        }
        checkpoint
            .validate_structure()
            .map_err(CheckpointIntegrityError::new)?;
        Ok(())
    }

    pub fn schema_commit_capability(&self) -> SchemaCommitCapability {
        SchemaCommitCapability::AtomicSchemaEvolution
    }

    async fn create_form(&self, form: &FormDefinition) -> Result<()> {
        form.validate()?;
        validate_field_ids(form)?;
        let ident = self.form_ident(form.id);
        if self.catalog.table_exists(&ident).await? {
            return Err(anyhow!("form already exists: {}", form.id));
        }
        let creation = TableCreation::builder()
            .name(ident.name().to_string())
            .location(self.form_location(form.id))
            .schema(form_schema(form)?)
            .partition_spec(UnboundPartitionSpec::default())
            .sort_order(SortOrder::unsorted_order())
            .properties(form_properties(form, self.write)?)
            .build();
        self.mutation_catalog()
            .create_table(&self.namespace, creation)
            .await?;
        Ok(())
    }

    pub async fn load_form(&self, form_id: FormId) -> Result<FormDefinition> {
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        form_from_table(&table, form_id)
    }

    pub async fn form_history(&self, form_id: FormId) -> Result<Vec<FormDefinition>> {
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        form_history_from_table(&table, form_id)
    }

    /// Returns whether the authoritative Catalog Head currently contains this
    /// Form table. This is a domain read and intentionally exposes neither a
    /// physical table handle nor a mutation path.
    pub async fn has_form(&self, form_id: FormId) -> Result<bool> {
        Ok(self.catalog.table_exists(&self.form_ident(form_id)).await?)
    }

    pub async fn list_forms(&self) -> Result<Vec<FormDefinition>> {
        self.list_forms_bounded(usize::MAX, usize::MAX).await
    }

    /// Append a selective inverse for one committed Change. The operation is
    /// intentionally scoped to one Form publication: cross-Form atomicity is
    /// a separate capability and must not be implied by the public API.
    pub async fn revert_change(
        &self,
        target_change_id: &str,
        command: &ChangeCommand,
    ) -> Result<CommitReceipt> {
        command
            .validate()
            .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
        if command.reverts_change_id.as_deref() != Some(target_change_id) {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                "revert command must identify the target Change explicitly",
            )
            .into());
        }
        let now = command.created_at_micros;
        let mut target_form = None;
        let mut inverse_revisions = Vec::new();
        for form in self.list_forms().await? {
            let revisions = self.read_revisions(form.id).await?;
            let targets = revisions
                .iter()
                .filter(|revision| revision.change_id == target_change_id)
                .collect::<Vec<_>>();
            if targets.is_empty() {
                continue;
            }
            if target_form.replace(form.id).is_some() {
                return Err(AppError::conflict(
                    ErrorCode::RevisionConflict,
                    "reverting a Change spanning multiple Forms is not supported",
                )
                .into());
            }
            let schema = form
                .fields
                .iter()
                .map(|field| (field.id, field.clone()))
                .collect::<BTreeMap<_, _>>();
            let mut target_entries = BTreeSet::new();
            for target in targets {
                if !target_entries.insert(target.entry_id) {
                    return Err(AppError::conflict(
                        ErrorCode::RevisionConflict,
                        "one Change contains multiple revisions for the same Entry",
                    )
                    .into());
                }
                let current = revisions
                    .iter()
                    .filter(|revision| revision.entry_id == target.entry_id)
                    .max_by_key(|revision| revision.entry_version)
                    .ok_or_else(|| {
                        AppError::conflict(
                            ErrorCode::RevisionConflict,
                            "target Change has no current Entry revision",
                        )
                    })?;
                let previous = revisions
                    .iter()
                    .filter(|revision| {
                        revision.entry_id == target.entry_id
                            && revision.entry_version < target.entry_version
                    })
                    .max_by_key(|revision| revision.entry_version);
                let empty_before = BTreeMap::new();
                let before = previous
                    .map(|revision| &revision.values)
                    .unwrap_or(&empty_before);
                let plan = selective_inverse_with_form_schema(
                    target_change_id,
                    before,
                    &target.values,
                    &current.values,
                    &schema,
                )
                .map_err(|conflict| {
                    AppError::conflict(
                        ErrorCode::RevisionConflict,
                        format!("cannot revert {target_change_id}: {conflict}"),
                    )
                })?;
                let mut values = current.values.clone();
                for (field_id, action) in plan.fields {
                    if let RevertFieldAction::Restore { value } = action {
                        match value {
                            Some(value) => {
                                values.insert(field_id, value);
                            }
                            None => {
                                values.remove(&field_id);
                            }
                        }
                    }
                }
                let restoring_existing = previous.is_some();
                let mut entry = current.entry.clone();
                entry.updated_at_micros = now;
                entry.updated_by = command.actor_principal_id.clone();
                entry.deleted = !restoring_existing;
                entry.deleted_at_micros = (!restoring_existing).then_some(now);
                entry.deleted_by =
                    (!restoring_existing).then(|| command.actor_principal_id.clone());
                if !restoring_existing {
                    values.clear();
                }
                inverse_revisions.push(EntryRevision {
                    form_id: current.form_id,
                    entry_id: current.entry_id,
                    revision_id: RevisionId::from(Uuid::new_v4()),
                    parent_revision_id: Some(current.revision_id),
                    entry_version: current.entry_version.saturating_add(1),
                    change_id: command.change_id.clone(),
                    expected_version: Some(current.entry_version),
                    operation: if restoring_existing {
                        EntryOperation::Upsert
                    } else {
                        EntryOperation::Delete
                    },
                    committed_at_micros: now,
                    author_id: current.author_id.clone(),
                    form_version: current.form_version,
                    source_kind: current.source_kind.clone(),
                    source_id: current.source_id.clone(),
                    entry,
                    values,
                    extra_attributes: current.extra_attributes.clone(),
                    extension_metadata: current.extension_metadata.clone(),
                });
            }
            break;
        }
        let form_id = target_form.ok_or_else(|| {
            AppError::not_found(
                ErrorCode::RevisionNotFound,
                format!("target Change was not found: {target_change_id}"),
            )
        })?;
        let publication =
            publication_context_for_change(command, "change.revert", &inverse_revisions)?;
        self.commit(publication)?
            .append_revisions(form_id, inverse_revisions)
            .await
    }

    /// Loads Form definitions with explicit count and serialized-size bounds.
    /// Catalog implementations may return table identifiers as a Vec, so the
    /// count is checked before any table metadata is retained. Definitions are
    /// then loaded one at a time and the cumulative persisted representation is
    /// bounded before the returned collection can grow without limit.
    pub async fn list_forms_bounded(
        &self,
        max_forms: usize,
        max_serialized_bytes: usize,
    ) -> Result<Vec<FormDefinition>> {
        let identifiers = if let Some(catalog) = &self.space_catalog {
            catalog
                .list_tables_bounded(&self.namespace, max_forms)
                .await?
        } else {
            let identifiers = self.catalog.list_tables(&self.namespace).await?;
            if identifiers.len() > max_forms {
                return Err(anyhow!("Form catalog exceeds its configured count limit"));
            }
            identifiers
        };
        let mut forms = Vec::new();
        let mut serialized_bytes = 0usize;
        for ident in identifiers {
            let table = self.catalog.load_table(&ident).await?;
            if let Some(raw) = table.metadata().properties().get(FORM_DEFINITION_PROPERTY) {
                serialized_bytes = serialized_bytes
                    .checked_add(raw.len())
                    .context("Form definition size overflow")?;
                if serialized_bytes > max_serialized_bytes {
                    return Err(anyhow!(
                        "Form definitions exceed their configured serialized-size limit"
                    ));
                }
                let form: FormDefinition = serde_json::from_str(raw)?;
                forms.push(form_from_table(&table, form.id)?);
            }
        }
        forms.sort_by(|left: &FormDefinition, right: &FormDefinition| left.name.cmp(&right.name));
        Ok(forms)
    }

    async fn evolve_form(&self, changes: &FormChangeSet) -> Result<FormDefinition> {
        let current = self.load_form(changes.form_id).await?;
        if changes.expected_version != Some(current.version) {
            return Err(anyhow!("Form version conflict"));
        }
        match changes.compatibility(&current)? {
            Compatibility::Breaking => {
                return Err(unsupported_form_field_type_change(&current, changes)?.into())
            }
            Compatibility::Compatible => {}
        }
        let evolved = current.apply(changes)?;
        let table = self
            .catalog
            .load_table(&self.form_ident(current.id))
            .await?;
        let mut form_history = form_history_from_table(&table, current.id)?;
        if form_history
            .last()
            .is_none_or(|form| form.version != current.version)
        {
            form_history.push(current.clone());
        }
        let current_schema = table.metadata().current_schema();
        let additions = evolved
            .fields
            .iter()
            .filter(|field| current_schema.field_by_id(field.id.get()).is_none())
            .collect::<Vec<_>>();
        for field in &current.fields {
            let evolved_field = evolved
                .fields
                .iter()
                .find(|candidate| candidate.id == field.id)
                .context("Form evolution removed a stable field ID")?;
            let physical = current_schema
                .field_by_id(field.id.get())
                .context("Iceberg schema is missing a stable Form field ID")?;
            if physical.field_type.as_ref()
                != &iceberg_type(
                    &evolved_field.field_type,
                    field.id.get(),
                    evolved_field.list_item.as_ref(),
                )
            {
                return Err(anyhow!(
                    "Iceberg schema is inconsistent with the existing Form definition"
                ));
            }
        }
        if let Some(space_catalog) = &self.space_catalog {
            let mut fields = current_schema
                .as_struct()
                .fields()
                .iter()
                .map(|field| {
                    let mut field = (**field).clone();
                    if let Some(evolved_field) = evolved
                        .fields
                        .iter()
                        .find(|candidate| candidate.id.get() == field.id)
                    {
                        field.name = evolved_field.name.clone();
                    }
                    Arc::new(field)
                })
                .collect::<Vec<_>>();
            fields.extend(additions.iter().map(|field| {
                Arc::new(NestedField::new(
                    field.id.get(),
                    field.name.clone(),
                    iceberg_type(&field.field_type, field.id.get(), field.list_item.as_ref()),
                    false,
                ))
            }));
            let schema = Schema::builder()
                .with_fields(fields)
                .with_identifier_field_ids(current_schema.identifier_field_ids())
                .build()?;
            let mut metadata_builder = table
                .metadata()
                .clone()
                .into_builder(Some(table.metadata_location_result()?.to_string()));
            if schema.calc_min_compatible_format() == iceberg::spec::FormatVersion::V3 {
                metadata_builder =
                    metadata_builder.upgrade_format_version(iceberg::spec::FormatVersion::V3)?;
            }
            form_history.push(evolved.clone());
            let metadata = metadata_builder
                .add_current_schema(schema)?
                .set_properties(form_properties_with_history(
                    &evolved,
                    self.write,
                    &form_history,
                )?)?
                .build()?
                .metadata;
            space_catalog
                .replace_table_metadata(table.identifier(), metadata)
                .await?;
            // The attempt remains bound to the pre-publication Head.  Its
            // static providers must not be refreshed behind that boundary;
            // the value we just derived is the authoritative post-commit
            // Form returned to the caller.
            return Ok(evolved);
        }
        if additions.is_empty() {
            form_history.push(evolved.clone());
            let tx = Transaction::new(&table);
            let mut action = tx.update_table_properties();
            for (key, value) in form_properties_with_history(&evolved, self.write, &form_history)? {
                action = action.set(key, value);
            }
            let catalog = self.mutation_catalog();
            action.apply(tx)?.commit(catalog.as_ref()).await?;
            return Ok(evolved);
        }
        form_history.push(evolved.clone());
        let tx = Transaction::new(&table);
        let mut schema_action = tx.update_schema();
        for field in additions {
            schema_action = schema_action.add_column(AddColumn::optional(
                &field.name,
                iceberg_type(&field.field_type, field.id.get(), field.list_item.as_ref()),
            ));
        }
        let transaction = schema_action.apply(tx)?;
        let mut properties = transaction.update_table_properties();
        for (key, value) in form_properties_with_history(&evolved, self.write, &form_history)? {
            properties = properties.set(key, value);
        }
        let catalog = self.mutation_catalog();
        properties
            .apply(transaction)?
            .commit(catalog.as_ref())
            .await?;
        self.load_form(changes.form_id).await
    }

    async fn validate_row_reference_targets(
        &self,
        form_id: FormId,
        revisions: &[EntryRevision],
        relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    ) -> Result<()> {
        let form = self.load_form(form_id).await?;
        let target_forms = self
            .list_forms()
            .await?
            .into_iter()
            .map(|form| (form.id, form))
            .collect::<HashMap<_, _>>();
        let pending_entry_ids = revisions
            .iter()
            .filter(|revision| {
                !matches!(
                    revision.operation,
                    EntryOperation::Delete | EntryOperation::Restore
                )
            })
            .flat_map(|revision| {
                [
                    (!revision.entry.external_id.is_empty())
                        .then(|| revision.entry.external_id.clone()),
                    Some(revision.entry_id.to_string()),
                ]
                .into_iter()
                .flatten()
                .map(move |entry_id| (revision.form_id, entry_id))
            })
            .collect::<BTreeSet<_>>();
        let mut references = BTreeSet::<(FormId, String)>::new();
        let mut reference_fields = HashMap::<(FormId, String), String>::new();
        for revision in revisions {
            if matches!(
                revision.operation,
                EntryOperation::Delete | EntryOperation::Restore
            ) {
                continue;
            }
            for field in &form.fields {
                let Some(value) = revision.values.get(&field.id) else {
                    continue;
                };
                match (&field.field_type, value) {
                    (FieldType::RowReference, FieldValue::String(entry_id)) => {
                        let target_form = field.reference_form.ok_or_else(|| {
                            invalid_revision_input(format!(
                                "Form validation failed: {}",
                                serde_json::json!([{
                                    "field": field.name,
                                    "message": "This row reference has no target Form"
                                }])
                            ))
                        })?;
                        reference_fields
                            .insert((target_form, entry_id.clone()), field.name.clone());
                        references.insert((target_form, entry_id.clone()));
                    }
                    (FieldType::List, FieldValue::List(values))
                        if field
                            .list_item
                            .as_ref()
                            .is_some_and(|item| item.field_type == FieldType::RowReference) =>
                    {
                        let target_form = field
                            .list_item
                            .as_ref()
                            .and_then(|item| item.reference_form)
                            .ok_or_else(|| {
                                invalid_revision_input(format!(
                                    "Form validation failed: {}",
                                    serde_json::json!([{
                                        "field": field.name,
                                        "message": "This row reference has no target Form"
                                    }])
                                ))
                            })?;
                        for value in values {
                            if let FieldValue::String(entry_id) = value {
                                reference_fields
                                    .insert((target_form, entry_id.clone()), field.name.clone());
                                references.insert((target_form, entry_id.clone()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if references.is_empty() {
            return Ok(());
        }

        let mut authorized_forms = BTreeMap::new();
        for (target_form_id, entry_id) in &references {
            let target_form = target_forms.get(target_form_id).ok_or_else(|| {
                invalid_revision_input(format!(
                    "Form validation failed: {}",
                    serde_json::json!([{
                        "field": reference_fields.get(&(*target_form_id, entry_id.clone())),
                        "message": format!("Referenced Form '{target_form_id}' does not exist")
                    }])
                ))
            })?;
            let entry_scope = relation_scopes
                .and_then(|scopes| scopes.get(&target_form.name.to_ascii_lowercase()).cloned())
                .unwrap_or_else(|| {
                    if relation_scopes.is_some() {
                        EntryScope::Only(BTreeSet::new())
                    } else {
                        EntryScope::AllCurrent
                    }
                });
            authorized_forms.insert(
                target_form.id,
                AuthorizedQueryForm {
                    relation: sql_relation_name(target_form.id),
                    entry_scope,
                    columns: target_form
                        .fields
                        .iter()
                        .map(|field| sql_column_name(field.id))
                        .collect(),
                    system_columns: BTreeSet::from([QuerySystemColumn::ExternalId]),
                },
            );
        }
        let context = self
            .authorized_query_context(AuthorizedQueryPolicy {
                forms: authorized_forms,
                checkpoint: None,
                limits: QueryLimits {
                    max_memory_bytes: 64 * 1024 * 1024,
                    max_rows: 1,
                    timeout: Duration::from_secs(30),
                    max_concurrency: 1,
                    allowed_functions: BTreeSet::new(),
                },
            })
            .await?;

        for (target_form_id, entry_id) in references {
            if pending_entry_ids.contains(&(target_form_id, entry_id.clone())) {
                continue;
            }
            let target_form = target_forms.get(&target_form_id).ok_or_else(|| {
                invalid_revision_input(format!(
                    "Form validation failed: {}",
                    serde_json::json!([{
                        "field": reference_fields.get(&(target_form_id, entry_id.clone())),
                        "message": format!("Referenced Form '{target_form_id}' does not exist")
                    }])
                ))
            })?;
            let relation_name = sql_relation_name(target_form.id);
            let relation = format!("\"{}\"", relation_name.replace('"', "\"\""));
            let literal = format!("'{}'", entry_id.replace('\'', "''"));
            let rows = context
                .execute(&format!(
                    "SELECT 1 FROM {relation} WHERE _ugoite_id = {literal} LIMIT 1"
                ))
                .await?;
            if rows.iter().map(|batch| batch.num_rows()).sum::<usize>() == 0 {
                let field = reference_fields
                    .get(&(target_form_id, entry_id.clone()))
                    .cloned();
                return Err(invalid_revision_input(format!(
                    "Form validation failed: {}",
                    serde_json::json!([{
                        "field": field,
                        "message": format!(
                            "Referenced Entry '{}' does not belong to Form '{}'",
                            entry_id, target_form.name
                        )
                    }])
                )));
            }
        }
        Ok(())
    }

    async fn validate_asset_references_not_deleted(
        &self,
        form_id: FormId,
        revisions: &[EntryRevision],
    ) -> Result<()> {
        let form = self.load_form(form_id).await?;
        for revision in revisions {
            if matches!(
                revision.operation,
                EntryOperation::Delete | EntryOperation::Restore
            ) {
                continue;
            }
            for field in &form.fields {
                let Some(value) = revision.values.get(&field.id) else {
                    continue;
                };
                let asset_references = match (&field.field_type, value) {
                    (FieldType::AssetReference, FieldValue::AssetReference(reference)) => {
                        vec![(reference.asset_id.as_str(), field.name.as_str())]
                    }
                    (FieldType::List, FieldValue::List(values))
                        if field
                            .list_item
                            .as_ref()
                            .is_some_and(|item| item.field_type == FieldType::AssetReference) =>
                    {
                        values
                            .iter()
                            .filter_map(|value| match value {
                                FieldValue::AssetReference(reference) => {
                                    Some((reference.asset_id.as_str(), field.name.as_str()))
                                }
                                _ => None,
                            })
                            .collect()
                    }
                    _ => Vec::new(),
                };
                for (asset_id, field_name) in asset_references {
                    if self.asset_is_deleted(asset_id).await? {
                        return Err(invalid_revision_input(format!(
                            "Form validation failed: {}",
                            serde_json::json!([{
                                "field": field_name,
                                "message": format!(
                                    "Asset '{}' is unavailable because it was deleted",
                                    asset_id
                                )
                            }])
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn validate_revision_batches_authorized(
        &self,
        batches: &[(FormId, Vec<EntryRevision>)],
        relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    ) -> Result<()> {
        let requested_entry_ids = batches
            .iter()
            .flat_map(|(_, revisions)| revisions)
            .filter(|revision| {
                revision.entry_version == 1
                    && revision.expected_version.is_none()
                    && revision.parent_revision_id.is_none()
            })
            .map(|revision| revision.entry.external_id.clone())
            .collect::<Vec<_>>();
        let existing_entry_ids = self
            .existing_entry_external_ids(&requested_entry_ids)
            .await?;
        if let Some(entry_id) = requested_entry_ids
            .iter()
            .find(|entry_id| existing_entry_ids.contains(*entry_id))
        {
            return Err(invalid_revision_input(format!(
                "Entry ID '{entry_id}' is already in use"
            )));
        }
        for (form_id, revisions) in batches {
            let form = self.load_form(*form_id).await?;
            if let Some(scopes) = relation_scopes {
                if !scopes.contains_key(&form.name.to_ascii_lowercase()) {
                    return Err(AppError::forbidden("Form is not readable").into());
                }
            }
            for revision in revisions {
                validate_revision_payload(&form, revision)?;
            }
            let table = self.catalog.load_table(&self.form_ident(*form_id)).await?;
            revision_batch_from_values(&form, table.metadata().current_schema(), revisions)
                .map_err(|error| {
                    invalid_revision_input(format!("Form validation failed: {error:#}"))
                })?;
            self.validate_asset_references_not_deleted(*form_id, revisions)
                .await?;
            self.validate_row_reference_targets(*form_id, revisions, relation_scopes)
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn asset_is_deleted(&self, asset_id: &str) -> Result<bool> {
        self.space_catalog
            .as_ref()
            .context("Asset lifecycle requires the OpenDAL-backed SpaceCatalog")?
            .asset_is_deleted(asset_id)
            .await
            .map_err(Into::into)
    }

    async fn mark_asset_deleted(&self, asset_id: &str) -> Result<()> {
        self.space_catalog
            .as_ref()
            .context("Asset lifecycle requires the OpenDAL-backed SpaceCatalog")?
            .mark_asset_deleted(asset_id)
            .await
            .map_err(Into::into)
    }

    async fn recover_existing_publication(
        &self,
    ) -> Result<Option<space_catalog::PublicationOutcome>> {
        self.space_catalog
            .as_ref()
            .context("publication recovery requires the OpenDAL-backed SpaceCatalog")?
            .recover_existing_publication()
            .await
            .map_err(Into::into)
    }

    pub(crate) async fn append_record_batches(
        &self,
        form_id: FormId,
        batches: Vec<RecordBatch>,
        revisions: &[EntryRevision],
    ) -> Result<CommitReceipt> {
        self.append_record_batches_inner(form_id, batches, revisions, true)
            .await
    }

    async fn append_record_batches_inner(
        &self,
        form_id: FormId,
        batches: Vec<RecordBatch>,
        revisions: &[EntryRevision],
        validate_revision_chain: bool,
    ) -> Result<CommitReceipt> {
        if batches.is_empty() || revisions.is_empty() {
            return Err(anyhow!("append batch must not be empty"));
        }
        let row_count: usize = batches.iter().map(RecordBatch::num_rows).sum();
        if row_count != revisions.len() {
            return Err(anyhow!(
                "record batch row count ({row_count}) does not match revisions ({})",
                revisions.len()
            ));
        }
        let form = self.load_form(form_id).await?;
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        let entry_ids = revisions
            .iter()
            .map(|revision| revision.entry_id)
            .collect::<Vec<_>>();
        let mut current = if validate_revision_chain {
            self.read_latest_revisions_for_entries(form_id, &entry_ids)
                .await?
                .into_iter()
                .map(|revision| (revision.entry_id, revision))
                .collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };
        let mut seen = std::collections::HashSet::new();
        for revision in revisions {
            if revision.form_id != form.id || revision.form_version != form.version {
                return Err(anyhow!(
                    "revision does not belong to the current Form version"
                ));
            }
            if !seen.insert(revision.revision_id) {
                return Err(anyhow!("duplicate revision ID in append batch"));
            }
            if validate_revision_chain {
                let previous = current.get(&revision.entry_id);
                if let Some(previous) = previous {
                    if revision.author_id != previous.author_id {
                        return Err(anyhow!("entry author cannot change across revisions"));
                    }
                    if revision.expected_version != Some(previous.entry_version)
                        || revision.parent_revision_id != Some(previous.revision_id)
                        || revision.entry_version
                            != previous
                                .entry_version
                                .checked_add(1)
                                .ok_or_else(|| anyhow!("entry version overflow"))?
                    {
                        return Err(anyhow!("entry revision conflict"));
                    }
                } else if revision.expected_version.is_some()
                    || revision.parent_revision_id.is_some()
                    || revision.entry_version != 1
                {
                    return Err(anyhow!("entry revision conflict"));
                }
            }
            validate_revision_payload(&form, revision)?;
            if validate_revision_chain {
                current.insert(revision.entry_id, revision.clone());
            }
        }
        validate_batch_revision_metadata(&batches, revisions)?;
        let table_arrow_schema = Arc::new(iceberg::arrow::schema_to_arrow_schema(
            table.metadata().current_schema(),
        )?);
        let batches = batches
            .into_iter()
            .map(|batch| {
                RecordBatch::try_new(table_arrow_schema.clone(), batch.columns().to_vec())
                    .map_err(|error| anyhow!("record batch schema does not match table: {error}"))
            })
            .collect::<Result<Vec<_>>>()?;
        let table_properties = table.metadata().table_properties()?;
        let parquet_writer = ParquetWriterBuilder::from_table_properties(
            &table_properties,
            table.metadata().current_schema().clone(),
        )?;
        let location_generator = DefaultLocationGenerator::new(table.metadata())?;
        let file_name_generator = DefaultFileNameGenerator::new(
            Uuid::now_v7().to_string(),
            None,
            DataFileFormat::Parquet,
        );
        let rolling_writer = RollingFileWriterBuilder::new(
            parquet_writer,
            table_properties.write_target_file_size_bytes,
            table.file_io().clone(),
            location_generator,
            file_name_generator,
        );
        let mut writer = DataFileWriterBuilder::new(rolling_writer)
            .build(None)
            .await?;
        for batch in batches {
            writer.write(batch).await?;
        }
        let data_files = writer.close().await?;
        let committed_at_micros = revisions
            .iter()
            .map(|revision| revision.committed_at_micros)
            .max()
            .unwrap_or_default();
        let ids = revisions
            .iter()
            .map(|revision| revision.revision_id)
            .collect::<Vec<_>>();
        let summary = HashMap::from([
            ("ugoite.form-id".into(), form_id.to_string()),
            ("ugoite.revision-count".into(), revisions.len().to_string()),
            (
                "ugoite.authors".into(),
                revisions
                    .iter()
                    .map(|revision| revision.author_id.as_str())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        ]);
        let tx = Transaction::new(&table);
        let action = tx
            .fast_append()
            .add_data_files(data_files.clone())
            .set_snapshot_properties(summary);
        let catalog = self.mutation_catalog();
        let updated = action.apply(tx)?.commit(catalog.as_ref()).await?;
        let snapshot_id = updated
            .metadata()
            .current_snapshot()
            .context("append commit did not create a snapshot")?
            .snapshot_id();
        Ok(CommitReceipt {
            command_id: String::new(),
            catalog_generation: 0,
            snapshot_id,
            committed_revision_ids: ids,
            committed_at_micros,
            data_file_count: data_files.len(),
        })
    }

    async fn append_revisions(
        &self,
        form_id: FormId,
        revisions: Vec<EntryRevision>,
    ) -> Result<CommitReceipt> {
        if revisions.is_empty() {
            return Err(anyhow!("append batch must not be empty"));
        }
        let form = self.load_form(form_id).await?;
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        let batch =
            revision_batch_from_values(&form, table.metadata().current_schema(), &revisions)
                .map_err(|error| {
                    invalid_revision_input(format!("Form validation failed: {error:#}"))
                })?;
        self.append_record_batches(form_id, vec![batch], &revisions)
            .await
    }

    /// Reads canonical revisions through Iceberg's Arrow projection. Physical
    /// column decoding lives in this adapter; callers receive only domain
    /// revisions and never Arrow arrays or Iceberg tables.
    pub async fn read_revisions(&self, form_id: FormId) -> Result<Vec<EntryRevision>> {
        self.read_revision_view(form_id, RevisionView::All).await
    }

    /// Reads one of the canonical revision views through the same DataFusion
    /// logical-plan builder used for live and future checkpoint providers.
    /// A duplicate maximum entry version is invariant corruption, never a
    /// condition resolved by timestamp, revision ID, or file order.
    pub async fn read_revision_view(
        &self,
        form_id: FormId,
        view: RevisionView,
    ) -> Result<Vec<EntryRevision>> {
        if view == RevisionView::All {
            let form = self.load_form(form_id).await?;
            let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
            return self
                .read_revision_view_from_table(
                    &form,
                    table,
                    EntryScope::AllCurrent,
                    view,
                    None,
                    None,
                )
                .await;
        }
        self.read_revision_view_with_scope(form_id, EntryScope::AllCurrent, view)
            .await
    }

    /// Reads a revision view through a provider-side Entry scope. The scope is
    /// part of the trusted DataFusion plan, so unauthorized rows never cross
    /// the query boundary into domain decoding.
    pub async fn read_revision_view_with_scope(
        &self,
        form_id: FormId,
        entry_scope: EntryScope,
        view: RevisionView,
    ) -> Result<Vec<EntryRevision>> {
        if view == RevisionView::All {
            bail!("scoped revision views do not expose full history");
        }
        let form = self.load_form(form_id).await?;
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        self.read_revision_view_from_table(&form, table, entry_scope, view, None, None)
            .await
    }

    /// Reads a canonical revision view from one immutable Iceberg snapshot.
    /// The latest-state plan is identical to the live provider; only the
    /// upstream Iceberg table provider is pinned.
    pub async fn read_revision_view_at_snapshot(
        &self,
        form_id: FormId,
        view: RevisionView,
        snapshot_id: i64,
    ) -> Result<Vec<EntryRevision>> {
        self.read_revision_view_with_snapshot(form_id, view, Some(snapshot_id))
            .await
    }

    async fn read_revision_view_with_snapshot(
        &self,
        form_id: FormId,
        view: RevisionView,
        snapshot_id: Option<i64>,
    ) -> Result<Vec<EntryRevision>> {
        let form = self.load_form(form_id).await?;
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        self.read_revision_view_from_table(
            &form,
            table,
            EntryScope::AllCurrent,
            view,
            snapshot_id,
            None,
        )
        .await
    }

    async fn read_revision_view_from_table(
        &self,
        form: &FormDefinition,
        table: iceberg::table::Table,
        entry_scope: EntryScope,
        view: RevisionView,
        snapshot_id: Option<i64>,
        checkpoint_history_limit: Option<usize>,
    ) -> Result<Vec<EntryRevision>> {
        let batches = match view {
            RevisionView::All if entry_scope == EntryScope::AllCurrent => {
                self.read_all_revision_batches(&table, snapshot_id).await?
            }
            RevisionView::All => {
                self.read_scoped_revision_batches(
                    &table,
                    &entry_scope,
                    snapshot_id,
                    checkpoint_history_limit.map_or(usize::MAX, |limit| limit.saturating_add(1)),
                )
                .await?
            }
            RevisionView::LatestIncludingTombstones | RevisionView::Current => {
                self.read_latest_revision_batches(
                    &table,
                    &entry_scope,
                    snapshot_id,
                    view,
                    Some(MAX_NORMAL_READ_ROWS),
                )
                .await?
            }
        };
        let schema = table.metadata().current_schema().clone();
        let mut revisions = Vec::new();
        for batch in &batches {
            revisions.extend(revisions_from_batch(batch, form, &schema)?);
        }
        if let Some(limit) = checkpoint_history_limit {
            if matches!(view, RevisionView::All) && revisions.len() > limit {
                return Err(anyhow!(
                    "checkpoint history exceeds the configured {limit}-revision response limit"
                ));
            }
        }
        Ok(revisions)
    }

    /// Point lookup variant of the latest plan. The entry predicate is applied
    /// before the max-version aggregation, preserving latest-state semantics
    /// while avoiding a full Form scan.
    pub async fn read_latest_revisions_for_entry(
        &self,
        form_id: FormId,
        entry_id: ugoite_domain::id::EntryId,
    ) -> Result<Vec<EntryRevision>> {
        self.read_latest_revisions_for_entries(form_id, &[entry_id])
            .await
    }

    async fn read_latest_revisions_for_entries(
        &self,
        form_id: FormId,
        entry_ids: &[ugoite_domain::id::EntryId],
    ) -> Result<Vec<EntryRevision>> {
        let form = self.load_form(form_id).await?;
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        let schema = table.metadata().current_schema().clone();
        let batches = self
            .read_latest_revision_batches(
                &table,
                &EntryScope::Only(entry_ids.iter().copied().collect()),
                None,
                RevisionView::LatestIncludingTombstones,
                Some(MAX_NORMAL_READ_ROWS),
            )
            .await?;
        let mut revisions = Vec::new();
        for batch in &batches {
            revisions.extend(revisions_from_batch(batch, &form, &schema)?);
        }
        Ok(revisions)
    }

    /// Checks external IDs against every Form's latest head, including
    /// tombstones. This is a point query per Form and request ID, rather than
    /// a caller-scoped current-state scan. The commit coordinator calls it on
    /// the exact publication attempt so a retry rechecks the winning head.
    pub(crate) async fn existing_entry_external_ids(
        &self,
        external_ids: &[String],
    ) -> Result<HashSet<String>> {
        if external_ids.is_empty() {
            return Ok(HashSet::new());
        }
        let entry_ids = external_ids
            .iter()
            .map(|external_id| {
                Uuid::parse_str(external_id)
                    .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, external_id.as_bytes()))
                    .into()
            })
            .collect::<Vec<ugoite_domain::id::EntryId>>();
        let mut existing = HashSet::new();
        for form in self.list_forms().await? {
            existing.extend(
                self.read_latest_revisions_for_entries(form.id, &entry_ids)
                    .await?
                    .into_iter()
                    .map(|revision| revision.entry.external_id),
            );
        }
        Ok(existing)
    }

    async fn revision_provider(
        &self,
        table: &iceberg::table::Table,
        snapshot_id: Option<i64>,
    ) -> Result<(Arc<dyn datafusion::datasource::TableProvider>, Option<i64>)> {
        let (provider, query_snapshot_id): (Arc<dyn datafusion::datasource::TableProvider>, _) =
            if let Some(snapshot_id) = snapshot_id {
                (
                    Arc::new(
                        read_schema_provider::CurrentSchemaTableProvider::try_new(
                            table.clone(),
                            snapshot_id,
                        )
                        .await?,
                    ),
                    Some(snapshot_id),
                )
            } else if let Some(snapshot_id) = table.metadata().current_snapshot_id() {
                (
                    Arc::new(
                        read_schema_provider::CurrentSchemaTableProvider::try_new(
                            table.clone(),
                            snapshot_id,
                        )
                        .await?,
                    ),
                    Some(snapshot_id),
                )
            } else {
                (
                    Arc::new(IcebergStaticTableProvider::try_new_from_table(table.clone()).await?),
                    None,
                )
            };
        Ok((provider, query_snapshot_id))
    }

    async fn read_all_revision_batches(
        &self,
        table: &iceberg::table::Table,
        snapshot_id: Option<i64>,
    ) -> Result<Vec<RecordBatch>> {
        // History is an explicit audit operation, not a normal current-state
        // read. Keep its schema-evolution-aware Iceberg stream separate from
        // the bounded DataFusion latest-state path above.
        let scan = match snapshot_id {
            Some(snapshot_id) => table.scan().snapshot_id(snapshot_id),
            None => table.scan(),
        };
        let mut stream = scan.build()?.to_arrow().await?;
        let mut batches = Vec::new();
        while let Some(batch) = futures::TryStreamExt::try_next(&mut stream).await? {
            batches.push(batch);
        }
        Ok(batches)
    }

    async fn read_scoped_revision_batches(
        &self,
        table: &iceberg::table::Table,
        entry_scope: &EntryScope,
        snapshot_id: Option<i64>,
        max_rows: usize,
    ) -> Result<Vec<RecordBatch>> {
        let (provider, query_snapshot_id) = self.revision_provider(table, snapshot_id).await?;
        let context = self
            .authorized_revision_query_context(
                provider,
                table.metadata().uuid().to_string(),
                query_snapshot_id,
                entry_scope,
                QueryLimits {
                    max_memory_bytes: 64 * 1024 * 1024,
                    max_rows,
                    timeout: Duration::from_secs(30),
                    max_concurrency: 1,
                    allowed_functions: BTreeSet::new(),
                },
            )
            .await?;
        let predicate = match entry_scope {
            EntryScope::AllCurrent => None,
            EntryScope::Only(entry_ids) if entry_ids.is_empty() => Some(lit(false)),
            EntryScope::Only(entry_ids) => Some(
                col("entry_id").in_list(
                    entry_ids
                        .iter()
                        .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                        .collect(),
                    false,
                ),
            ),
            EntryScope::AllExcept(entry_ids) => Some(
                col("entry_id").in_list(
                    entry_ids
                        .iter()
                        .map(|entry_id| lit(entry_id.as_uuid().as_bytes().to_vec()))
                        .collect(),
                    true,
                ),
            ),
        };
        let projection = table
            .metadata()
            .current_schema()
            .as_struct()
            .fields()
            .iter()
            .map(|field| ident(&field.name))
            .collect();
        context
            .execute_relation_plan(
                "revisions",
                &[],
                predicate.into_iter().collect(),
                projection,
                Vec::new(),
                false,
                false,
                max_rows,
            )
            .await
    }

    async fn read_latest_revision_batches(
        &self,
        table: &iceberg::table::Table,
        entry_scope: &EntryScope,
        snapshot_id: Option<i64>,
        view: RevisionView,
        max_rows: Option<usize>,
    ) -> Result<Vec<RecordBatch>> {
        self.read_latest_revision_batches_with_permits(
            table,
            entry_scope,
            snapshot_id,
            view,
            max_rows,
            self.shared_query_permits(1),
        )
        .await
    }

    async fn read_latest_revision_batches_with_permits(
        &self,
        table: &iceberg::table::Table,
        entry_scope: &EntryScope,
        snapshot_id: Option<i64>,
        view: RevisionView,
        max_rows: Option<usize>,
        permits: Arc<Semaphore>,
    ) -> Result<Vec<RecordBatch>> {
        let (provider, query_snapshot_id) = self.revision_provider(table, snapshot_id).await?;
        let context = self
            .authorized_revision_query_context_with_permits(
                provider,
                table.metadata().uuid().to_string(),
                query_snapshot_id,
                entry_scope,
                QueryLimits {
                    max_memory_bytes: 64 * 1024 * 1024,
                    max_rows: max_rows.unwrap_or(i64::MAX as usize / 2),
                    timeout: Duration::from_secs(30),
                    max_concurrency: 1,
                    allowed_functions: BTreeSet::new(),
                },
                permits,
            )
            .await?;
        let ids = context
            .execute_latest_revision_plan(entry_scope, view)
            .await?;
        let mut revision_ids = Vec::<ugoite_domain::id::EntryId>::new();
        let mut entry_ids = std::collections::BTreeSet::new();
        for batch in ids {
            let entry_values = batch
                .column_by_name("entry_id")
                .context("latest revision plan is missing entry_id")?;
            let revision_values = batch
                .column_by_name("revision_id")
                .context("latest revision plan is missing revision_id")?;
            for row in 0..batch.num_rows() {
                if !entry_ids.insert(uuid_at(entry_values, row)?) {
                    return Err(anyhow!(
                        "entry revision invariant failed: multiple revisions share a maximum entry_version"
                    ));
                }
                revision_ids.push(uuid_at(revision_values, row)?);
            }
        }
        if max_rows.is_some_and(|max_rows| entry_ids.len() > max_rows) {
            return Err(anyhow!(
                "normal Entry reads are limited to {MAX_NORMAL_READ_ROWS} current rows"
            ));
        }
        if revision_ids.is_empty() {
            return Ok(Vec::new());
        }
        let revision_literals = revision_ids
            .iter()
            .map(|revision_id| lit(revision_id.as_uuid().as_bytes().to_vec()))
            .collect::<Vec<_>>();
        let projection = table
            .metadata()
            .current_schema()
            .as_struct()
            .fields()
            .iter()
            .map(|field| ident(&field.name))
            .collect::<Vec<_>>();
        context
            .execute_relation_plan(
                "revisions",
                &[],
                vec![col("revision_id").in_list(revision_literals, false)],
                projection,
                Vec::new(),
                false,
                false,
                revision_ids.len(),
            )
            .await
    }

    /// Visits every current Entry in bounded keyset pages. Derived rebuilds
    /// intentionally bypass the API response ceiling, but never bypass the
    /// authorized latest-state plan or materialize a whole Form through one
    /// giant revision-id list.
    pub(crate) async fn visit_current_revision_view_for_derived<F>(
        &self,
        form_id: FormId,
        mut visit: F,
    ) -> Result<()>
    where
        F: FnMut(EntryRevision) -> Result<()>,
    {
        let form = self.load_form(form_id).await?;
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        let (provider, query_snapshot_id) = self.revision_provider(&table, None).await?;
        let context = self
            .authorized_revision_query_context_with_permits(
                provider,
                table.metadata().uuid().to_string(),
                query_snapshot_id,
                &EntryScope::AllCurrent,
                QueryLimits {
                    max_memory_bytes: 64 * 1024 * 1024,
                    max_rows: DERIVED_REVISION_PAGE_SIZE,
                    timeout: Duration::from_secs(30),
                    max_concurrency: 1,
                    allowed_functions: BTreeSet::new(),
                },
                self.maintenance_query_permits(1),
            )
            .await?;
        let schema = table.metadata().current_schema().clone();
        let mut after_entry_id = None;
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let batches = context
                    .execute_latest_revision_page(
                        &EntryScope::AllCurrent,
                        RevisionView::LatestIncludingTombstones,
                        after_entry_id.as_deref(),
                        DERIVED_REVISION_PAGE_SIZE,
                    )
                    .await?;
                let mut page_rows = 0usize;
                for batch in batches {
                    let revisions = revisions_from_batch(&batch, &form, &schema)?;
                    page_rows = page_rows.saturating_add(revisions.len());
                    for revision in revisions {
                        after_entry_id = Some(revision.entry_id.as_uuid().as_bytes().to_vec());
                        visit(revision)?;
                    }
                }
                if page_rows < DERIVED_REVISION_PAGE_SIZE {
                    break;
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .map_err(|_| anyhow!("derived current Entry stream timed out"))??;
        Ok(())
    }

    pub async fn query(&self, sql: &str) -> Result<Vec<RecordBatch>> {
        let context = SessionContext::new();
        let provider = IcebergCatalogProvider::try_new(self.catalog.clone()).await?;
        context.register_catalog("ugoite", Arc::new(provider));
        Ok(context.sql(sql).await?.collect().await?)
    }

    pub async fn maintenance_plan(
        &self,
        form_id: FormId,
        small_file_bytes: u64,
    ) -> Result<MaintenancePlan> {
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        let small_file_count = table
            .metadata()
            .current_snapshot()
            .and_then(|snapshot| {
                snapshot
                    .summary()
                    .additional_properties
                    .get("added-data-files")
                    .and_then(|value| value.parse().ok())
            })
            .unwrap_or(0);
        Ok(MaintenancePlan {
            form_id,
            small_file_count,
            rewrite_data_files: small_file_count > 1
                && small_file_bytes < self.write.target_file_size_bytes,
            rewrite_manifests: true,
            expire_snapshots: true,
            remove_orphans: true,
            refresh_statistics: true,
        })
    }

    fn form_ident(&self, form_id: FormId) -> TableIdent {
        TableIdent::new(self.namespace.clone(), physical_form_name(form_id))
    }
    fn form_location(&self, form_id: FormId) -> String {
        logical_uri(
            self.logical_space_uid,
            &format!("forms/{}", physical_form_name(form_id)),
        )
        .expect("workspace logical Form location must be canonical")
    }
}

/// Converts missing immutable files discovered while scanning a checkpoint to
/// the stable checkpoint API error. Planning and execution can reach manifest
/// lists, manifests, and data files after the metadata coordinate was loaded.
/// Those are checkpoint targets too, even though DataFusion/Iceberg own the
/// actual reads.
fn checkpoint_query_error(error: anyhow::Error) -> anyhow::Error {
    if error
        .chain()
        .any(space_catalog::error_chain_contains_not_found)
    {
        CheckpointUnavailable::new("checkpoint immutable data").into()
    } else {
        error
    }
}

impl SpaceCommitCoordinator {
    fn ensure_authoritative_mutation_contract(&self) -> Result<()> {
        self.workspace
            .space_catalog
            .as_ref()
            .context("coordinator is missing its SpaceCatalog")?
            .ensure_authoritative_mutation_contract()
    }

    async fn attempt_workspace(&self) -> Result<IcebergWorkspace> {
        let catalog = self
            .workspace
            .space_catalog
            .as_ref()
            .context("coordinator is missing its SpaceCatalog")?;
        let catalog = catalog
            .new_attempt()
            .with_publication_context(self.publication.clone())
            .bind_exact_head()
            .await?;
        let catalog = Arc::new(catalog);
        Ok(IcebergWorkspace {
            catalog: catalog.clone(),
            space_catalog: Some(catalog),
            namespace: self.workspace.namespace.clone(),
            space_id: self.workspace.space_id,
            logical_space_uid: self.workspace.logical_space_uid,
            warehouse: self.workspace.warehouse.clone(),
            write: self.workspace.write,
        })
    }

    async fn publication_outcome(&self) -> Result<Option<space_catalog::PublicationOutcome>> {
        let catalog = self
            .workspace
            .space_catalog
            .as_ref()
            .context("coordinator is missing its SpaceCatalog")?;
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            match catalog.publication_outcome(&self.publication).await {
                Ok(receipt) => return Ok(receipt),
                Err(error) if error.to_string().contains("Catalog Head changed") => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(anyhow!(
            "Catalog Head changed while resolving the command outcome"
        ))
    }

    pub async fn create_form(&self, form: &FormDefinition) -> Result<()> {
        self.ensure_authoritative_mutation_contract()?;
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            if self.publication_outcome().await?.is_some() {
                return Ok(());
            }
            let attempt = self.attempt_workspace().await?;
            match attempt.recover_existing_publication().await {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => {}
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
            match attempt.create_form(form).await {
                Ok(()) => return Ok(()),
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(anyhow!("Catalog Head changed during every create attempt"))
    }

    pub async fn evolve_form(&self, changes: &FormChangeSet) -> Result<FormDefinition> {
        self.ensure_authoritative_mutation_contract()?;
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            if self.publication_outcome().await?.is_some() {
                return self.workspace.load_form(changes.form_id).await;
            }
            let attempt = self.attempt_workspace().await?;
            match attempt.recover_existing_publication().await {
                Ok(Some(_)) => return self.workspace.load_form(changes.form_id).await,
                Ok(None) => {}
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
            match attempt.evolve_form(changes).await {
                Ok(form) => return Ok(form),
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(anyhow!(
            "Catalog Head changed during every form evolution attempt"
        ))
    }

    pub async fn append_revisions(
        &self,
        form_id: FormId,
        revisions: Vec<EntryRevision>,
    ) -> Result<CommitReceipt> {
        self.append_revisions_authorized(form_id, revisions, None)
            .await
    }

    pub async fn append_revisions_authorized(
        &self,
        form_id: FormId,
        revisions: Vec<EntryRevision>,
        relation_scopes: Option<&BTreeMap<String, EntryScope>>,
    ) -> Result<CommitReceipt> {
        self.ensure_authoritative_mutation_contract()?;
        if self.publication.change.is_some()
            && revisions
                .iter()
                .any(|revision| revision.change_id != self.publication.command_id)
        {
            return Err(anyhow!(
                "Change ID must equal the publication command ID for every revision"
            ));
        }
        if let Some(receipt) = self.publication_outcome().await? {
            return Ok(CommitReceipt {
                command_id: receipt.command_id,
                catalog_generation: receipt.catalog_generation,
                snapshot_id: receipt
                    .snapshot_id
                    .context("revision publication did not create an Iceberg snapshot")?,
                committed_revision_ids: revisions
                    .iter()
                    .map(|revision| revision.revision_id)
                    .collect(),
                committed_at_micros: revisions
                    .iter()
                    .map(|revision| revision.committed_at_micros)
                    .max()
                    .unwrap_or_default(),
                data_file_count: 0,
            });
        }
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            if let Some(receipt) = self.publication_outcome().await? {
                return Ok(CommitReceipt {
                    command_id: receipt.command_id,
                    catalog_generation: receipt.catalog_generation,
                    snapshot_id: receipt
                        .snapshot_id
                        .context("revision publication did not create an Iceberg snapshot")?,
                    committed_revision_ids: revisions
                        .iter()
                        .map(|revision| revision.revision_id)
                        .collect(),
                    committed_at_micros: revisions
                        .iter()
                        .map(|revision| revision.committed_at_micros)
                        .max()
                        .unwrap_or_default(),
                    data_file_count: 0,
                });
            }
            let attempt = self.attempt_workspace().await?;
            let recovered = match attempt.recover_existing_publication().await {
                Ok(recovered) => recovered,
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            };
            if let Some(publication) = recovered {
                return Ok(CommitReceipt {
                    command_id: publication.command_id,
                    catalog_generation: publication.catalog_generation,
                    snapshot_id: publication
                        .snapshot_id
                        .context("revision publication did not create an Iceberg snapshot")?,
                    committed_revision_ids: revisions
                        .iter()
                        .map(|revision| revision.revision_id)
                        .collect(),
                    committed_at_micros: revisions
                        .iter()
                        .map(|revision| revision.committed_at_micros)
                        .max()
                        .unwrap_or_default(),
                    data_file_count: 0,
                });
            }
            let new_entry_ids = revisions
                .iter()
                .filter(|revision| {
                    revision.entry_version == 1
                        && revision.expected_version.is_none()
                        && revision.parent_revision_id.is_none()
                })
                .map(|revision| revision.entry.external_id.clone())
                .collect::<Vec<_>>();
            let existing_entry_ids = attempt.existing_entry_external_ids(&new_entry_ids).await?;
            if new_entry_ids
                .iter()
                .any(|entry_id| existing_entry_ids.contains(entry_id))
            {
                return Err(invalid_revision_input("Entry ID is already in use"));
            }
            attempt
                .validate_asset_references_not_deleted(form_id, &revisions)
                .await?;
            attempt
                .validate_row_reference_targets(form_id, &revisions, relation_scopes)
                .await?;
            #[cfg(debug_assertions)]
            if let Some(gate) = &self.validation_gate {
                gate.pause().await;
            }
            let mut receipt = match attempt.append_revisions(form_id, revisions.clone()).await {
                Ok(receipt) => receipt,
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            };
            let publication = self
                .publication_outcome()
                .await?
                .context("successful append is missing its Catalog publication")?;
            receipt.command_id = publication.command_id;
            receipt.catalog_generation = publication.catalog_generation;
            if publication.snapshot_id != Some(receipt.snapshot_id) {
                return Err(anyhow!(
                    "Catalog publication snapshot does not match the append receipt"
                ));
            }
            return Ok(receipt);
        }
        Err(anyhow!("Catalog Head changed during every append attempt"))
    }

    pub async fn delete_asset(
        &self,
        asset_id: &str,
        relation_scopes: &BTreeMap<String, ugoite_core::query::EntryScope>,
    ) -> Result<()> {
        self.ensure_authoritative_mutation_contract()?;
        if self.publication_outcome().await?.is_some() {
            return Ok(());
        }
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            if self.publication_outcome().await?.is_some() {
                return Ok(());
            }
            let attempt = self.attempt_workspace().await?;
            match attempt.asset_is_deleted(asset_id).await {
                Ok(true) => {
                    return Err(anyhow!("Asset '{}' is already unavailable", asset_id));
                }
                Ok(false) => {}
                Err(error) => return Err(error),
            }
            let all_current_scopes = attempt
                .list_forms()
                .await?
                .into_iter()
                .map(|form| {
                    (
                        form.name.to_ascii_lowercase(),
                        ugoite_core::query::EntryScope::AllCurrent,
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let referenced_anywhere = crate::asset::current_asset_reference_exists_in_workspace(
                &attempt,
                asset_id,
                &all_current_scopes,
            )
            .await?;
            if referenced_anywhere
                && crate::asset::current_asset_reference_exists_in_workspace(
                    &attempt,
                    asset_id,
                    relation_scopes,
                )
                .await?
            {
                return Err(anyhow::Error::new(
                    crate::asset::AssetDeleteConflict::Visible,
                ));
            }
            if referenced_anywhere {
                // The reference is deliberately not named: the caller is not
                // authorized to learn which Entry protects the bytes. The
                // all-current query above still makes deletion fail closed.
                return Err(anyhow::Error::new(
                    crate::asset::AssetDeleteConflict::Hidden,
                ));
            }
            #[cfg(debug_assertions)]
            if let Some(gate) = &self.validation_gate {
                gate.pause().await;
            }
            match attempt.mark_asset_deleted(asset_id).await {
                Ok(()) => return Ok(()),
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(anyhow!(
            "Catalog Head changed during every asset delete attempt"
        ))
    }
}

fn is_publication_conflict(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("Catalog Head changed"))
}

pub fn physical_form_name(form_id: FormId) -> String {
    sql_relation_name(form_id)
}

pub fn namespace_for_space(space_id: SpaceId) -> NamespaceIdent {
    NamespaceIdent::new(format!("space_{}", space_id.as_uuid().simple()))
}

/// Returns the Arrow schema used by the revision table. This is also the
/// canonical schema callers must use when constructing append batches.
pub fn arrow_schema_for_form(form: &FormDefinition) -> Result<arrow_schema::Schema> {
    Ok(iceberg::arrow::schema_to_arrow_schema(&form_schema(form)?)?)
}

fn validate_field_ids(form: &FormDefinition) -> Result<()> {
    if let Some(field) = form
        .fields
        .iter()
        .find(|field| field.id.get() < FIRST_FORM_FIELD_ID)
    {
        return Err(anyhow!(
            "field {} uses reserved Iceberg field ID {}",
            field.name,
            field.id.get()
        ));
    }
    Ok(())
}

fn physical_field_id(field: &ugoite_domain::form::FormField) -> i32 {
    field.id.get()
}

fn form_from_table(table: &iceberg::table::Table, form_id: FormId) -> Result<FormDefinition> {
    validate_attribution_schema(table)?;
    let raw = table
        .metadata()
        .properties()
        .get(FORM_DEFINITION_PROPERTY)
        .context("Iceberg table is missing Ugoite Form metadata")?;
    let form: FormDefinition = serde_json::from_str(raw)?;
    if form.id != form_id {
        return Err(anyhow!(
            "Form ID property does not match physical table identity"
        ));
    }
    for field in &form.fields {
        let Some(physical) = table
            .metadata()
            .current_schema()
            .field_by_id(field.id.get())
        else {
            return Err(anyhow!(
                "Iceberg schema is missing Form field ID {}",
                field.id.get()
            ));
        };
        if physical.field_type.as_ref()
            != &iceberg_type(&field.field_type, field.id.get(), field.list_item.as_ref())
        {
            return Err(anyhow!(
                "Iceberg field ID {} does not match the Form field type",
                field.id.get()
            ));
        }
    }
    Ok(form)
}

fn form_history_from_table(
    table: &iceberg::table::Table,
    form_id: FormId,
) -> Result<Vec<FormDefinition>> {
    let current = form_from_table(table, form_id)?;
    let Some(raw) = table.metadata().properties().get(FORM_HISTORY_PROPERTY) else {
        return Ok(vec![current]);
    };
    let history: Vec<FormDefinition> =
        serde_json::from_str(raw).context("Iceberg Form history metadata is malformed")?;
    if history.is_empty() {
        return Err(anyhow!("Iceberg Form history metadata is empty"));
    }
    for form in &history {
        if form.id != form_id || form.version.get() == 0 {
            return Err(anyhow!("Iceberg Form history identity is invalid"));
        }
    }
    if history
        .iter()
        .any(|form| form.version == current.version && form != &current)
    {
        return Err(anyhow!(
            "Iceberg Form history conflicts with current definition"
        ));
    }
    if history.iter().any(|form| form.version == current.version) {
        Ok(history)
    } else {
        let mut history = history;
        history.push(current);
        Ok(history)
    }
}

/// Attribution is part of the v1-pre physical Form contract. Existing tables
/// with the former schema are rejected explicitly instead of failing later
/// with an Arrow column-count or missing-column error; pre-v1 Spaces must be
/// recreated after this breaking schema change.
fn validate_attribution_schema(table: &iceberg::table::Table) -> Result<()> {
    for (id, name, kind, required) in [
        (24, "ugoite_entry_updated_by", PrimitiveType::String, true),
        (25, "ugoite_entry_deleted_by", PrimitiveType::String, false),
    ] {
        let field = table
            .metadata()
            .current_schema()
            .field_by_id(id)
            .with_context(|| {
                format!(
                    "Iceberg Form table is missing required Entry attribution column {name}; recreate this pre-v1 Space"
                )
            })?;
        if field.name != name
            || field.required != required
            || field.field_type.as_ref() != &Type::Primitive(kind)
        {
            return Err(anyhow!(
                "Iceberg Form table has incompatible Entry attribution column {name}; recreate this pre-v1 Space"
            ));
        }
    }
    Ok(())
}

fn form_schema(form: &FormDefinition) -> Result<Schema> {
    let mut fields = vec![
        required(1, "entry_id", PrimitiveType::Uuid),
        required(2, "revision_id", PrimitiveType::Uuid),
        optional(3, "parent_revision_id", PrimitiveType::Uuid),
        required(4, "entry_version", PrimitiveType::Long),
        required(5, "operation", PrimitiveType::String),
        required(6, "committed_at", PrimitiveType::Timestamptz),
        required(7, "author_id", PrimitiveType::String),
        required(8, "form_version", PrimitiveType::Int),
        required(9, "source_kind", PrimitiveType::String),
        optional(10, "source_id", PrimitiveType::String),
        optional(11, "extension_metadata", PrimitiveType::String),
        optional(12, "extra_attributes", PrimitiveType::String),
        required(13, "ugoite_entry_title", PrimitiveType::String),
        required_type(
            14,
            "ugoite_entry_tags",
            Type::List(ListType::new(Arc::new(NestedField::new(
                nested_field_id(14, 0),
                "element",
                Type::Primitive(PrimitiveType::String),
                false,
            )))),
        ),
        required(16, "ugoite_entry_created_at", PrimitiveType::Timestamptz),
        required(17, "ugoite_entry_updated_at", PrimitiveType::Timestamptz),
        required_type(
            19,
            "ugoite_entry_integrity",
            Type::Struct(StructType::new(vec![
                optional(nested_field_id(19, 1), "checksum", PrimitiveType::String),
                optional(nested_field_id(19, 2), "signature", PrimitiveType::String),
            ])),
        ),
        required(20, "ugoite_entry_deleted", PrimitiveType::Boolean),
        optional(21, "ugoite_entry_deleted_at", PrimitiveType::Timestamptz),
        optional(22, "ugoite_entry_restored_from", PrimitiveType::Uuid),
        required(23, "ugoite_entry_external_id", PrimitiveType::String),
        required(24, "ugoite_entry_updated_by", PrimitiveType::String),
        optional(25, "ugoite_entry_deleted_by", PrimitiveType::String),
        required(26, "change_id", PrimitiveType::String),
    ];
    for field in &form.fields {
        fields.push(Arc::new(NestedField::new(
            physical_field_id(field),
            field.name.clone(),
            iceberg_type(
                &field.field_type,
                physical_field_id(field),
                field.list_item.as_ref(),
            ),
            // Revision tables also contain tombstones. Requiredness is enforced
            // by EntryRevision validation and Form metadata; physical columns
            // must remain nullable so a delete can carry no value payload.
            false,
        )));
    }
    Ok(Schema::builder().with_fields(fields).build()?)
}

fn required(id: i32, name: &str, kind: PrimitiveType) -> Arc<NestedField> {
    Arc::new(NestedField::new(id, name, Type::Primitive(kind), true))
}
fn optional(id: i32, name: &str, kind: PrimitiveType) -> Arc<NestedField> {
    Arc::new(NestedField::new(id, name, Type::Primitive(kind), false))
}
fn required_type(id: i32, name: &str, kind: Type) -> Arc<NestedField> {
    Arc::new(NestedField::new(id, name, kind, true))
}

fn iceberg_type(kind: &FieldType, parent_id: i32, list_item: Option<&ListItemDefinition>) -> Type {
    match kind {
        FieldType::Boolean => Type::Primitive(PrimitiveType::Boolean),
        FieldType::Integer => Type::Primitive(PrimitiveType::Int),
        FieldType::Long => Type::Primitive(PrimitiveType::Long),
        FieldType::Float => Type::Primitive(PrimitiveType::Float),
        FieldType::Double => Type::Primitive(PrimitiveType::Double),
        FieldType::Date => Type::Primitive(PrimitiveType::Date),
        FieldType::Time => Type::Primitive(PrimitiveType::Time),
        FieldType::Timestamp => Type::Primitive(PrimitiveType::Timestamp),
        FieldType::TimestampTz => Type::Primitive(PrimitiveType::Timestamptz),
        FieldType::TimestampNs => Type::Primitive(PrimitiveType::TimestampNs),
        FieldType::TimestampTzNs => Type::Primitive(PrimitiveType::TimestamptzNs),
        FieldType::Uuid => Type::Primitive(PrimitiveType::Uuid),
        FieldType::Binary => Type::Primitive(PrimitiveType::Binary),
        FieldType::String | FieldType::Markdown | FieldType::Sql | FieldType::RowReference => {
            Type::Primitive(PrimitiveType::String)
        }
        FieldType::AssetReference => asset_reference_type(parent_id),
        FieldType::List => {
            let item_kind = list_item
                .map(|item| &item.field_type)
                .unwrap_or(&FieldType::String);
            Type::List(ListType::new(Arc::new(NestedField::new(
                nested_field_id(parent_id, 0),
                "element",
                iceberg_type(item_kind, nested_field_id(parent_id, 0), None),
                false,
            ))))
        }
        FieldType::ObjectList => {
            let fields = vec![
                Arc::new(NestedField::new(
                    nested_field_id(parent_id, 1),
                    "type",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
                Arc::new(NestedField::new(
                    nested_field_id(parent_id, 2),
                    "name",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
                Arc::new(NestedField::new(
                    nested_field_id(parent_id, 3),
                    "description",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
            ];
            Type::List(ListType::new(Arc::new(NestedField::new(
                nested_field_id(parent_id, 0),
                "element",
                Type::Struct(StructType::new(fields)),
                false,
            ))))
        }
    }
}

fn asset_reference_type(parent_id: i32) -> Type {
    Type::Struct(StructType::new(vec![
        optional(
            nested_field_id(parent_id, 1),
            "asset_id",
            PrimitiveType::String,
        ),
        optional(nested_field_id(parent_id, 2), "name", PrimitiveType::String),
        optional(
            nested_field_id(parent_id, 3),
            "media_type",
            PrimitiveType::String,
        ),
        optional(
            nested_field_id(parent_id, 4),
            "size_bytes",
            PrimitiveType::Long,
        ),
        optional(
            nested_field_id(parent_id, 5),
            "sha256",
            PrimitiveType::String,
        ),
    ]))
}

fn nested_field_id(parent_id: i32, offset: i32) -> i32 {
    NESTED_FIELD_ID_BASE + parent_id * 10 + offset
}

fn form_properties(form: &FormDefinition, write: WriteConfig) -> Result<HashMap<String, String>> {
    form_properties_with_history(form, write, std::slice::from_ref(form))
}

fn form_properties_with_history(
    form: &FormDefinition,
    write: WriteConfig,
    history: &[FormDefinition],
) -> Result<HashMap<String, String>> {
    Ok(HashMap::from([
        (
            FORM_DEFINITION_PROPERTY.into(),
            serde_json::to_string(form)?,
        ),
        (
            FORM_HISTORY_PROPERTY.into(),
            serde_json::to_string(history)?,
        ),
        (FORM_ID_PROPERTY.into(), form.id.to_string()),
        (FORM_NAME_PROPERTY.into(), form.name.clone()),
        (FORM_VERSION_PROPERTY.into(), form.version.get().to_string()),
        (
            TARGET_FILE_SIZE_PROPERTY.into(),
            write.target_file_size_bytes.to_string(),
        ),
    ]))
}

fn revision_batch_from_values(
    form: &FormDefinition,
    table_schema: &iceberg::spec::Schema,
    revisions: &[EntryRevision],
) -> Result<RecordBatch> {
    let schema = Arc::new(iceberg::arrow::schema_to_arrow_schema(table_schema)?);
    let mut entry_ids = FixedSizeBinaryBuilder::with_capacity(revisions.len(), 16);
    let mut revision_ids = FixedSizeBinaryBuilder::with_capacity(revisions.len(), 16);
    let mut parents = FixedSizeBinaryBuilder::with_capacity(revisions.len(), 16);
    for revision in revisions {
        entry_ids.append_value(revision.entry_id.as_uuid().as_bytes())?;
        revision_ids.append_value(revision.revision_id.as_uuid().as_bytes())?;
        match revision.parent_revision_id {
            Some(parent) => parents.append_value(parent.as_uuid().as_bytes())?,
            None => parents.append_null(),
        }
    }
    let mut arrays: Vec<ArrayRef> = vec![
        Arc::new(entry_ids.finish()),
        Arc::new(revision_ids.finish()),
        Arc::new(parents.finish()),
        Arc::new(Int64Array::from(
            revisions
                .iter()
                .map(|revision| i64::try_from(revision.entry_version))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| match revision.operation {
                    EntryOperation::Upsert => "upsert",
                    EntryOperation::Delete => "delete",
                    EntryOperation::Restore => "restore",
                })
                .collect::<Vec<_>>(),
        )),
        Arc::new(
            TimestampMicrosecondArray::from(
                revisions
                    .iter()
                    .map(|revision| revision.committed_at_micros)
                    .collect::<Vec<_>>(),
            )
            .with_timezone("+00:00"),
        ),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.author_id.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(Int32Array::from(
            revisions
                .iter()
                .map(|revision| i32::try_from(revision.form_version.get()))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.source_kind.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.source_id.as_deref())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| serde_json::to_string(&revision.extension_metadata))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| serde_json::to_string(&revision.extra_attributes))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.entry.title.as_str())
                .collect::<Vec<_>>(),
        )),
        string_list_array(
            schema
                .field_with_name("ugoite_entry_tags")
                .context("missing tags metadata field")?,
            revisions,
            |revision| &revision.entry.tags,
        )?,
        Arc::new(
            TimestampMicrosecondArray::from(
                revisions
                    .iter()
                    .map(|revision| revision.entry.created_at_micros)
                    .collect::<Vec<_>>(),
            )
            .with_timezone("+00:00"),
        ),
        Arc::new(
            TimestampMicrosecondArray::from(
                revisions
                    .iter()
                    .map(|revision| revision.entry.updated_at_micros)
                    .collect::<Vec<_>>(),
            )
            .with_timezone("+00:00"),
        ),
        integrity_array(
            schema
                .field_with_name("ugoite_entry_integrity")
                .context("missing integrity metadata field")?,
            revisions,
        )?,
        Arc::new(BooleanArray::from(
            revisions
                .iter()
                .map(|revision| revision.entry.deleted)
                .collect::<Vec<_>>(),
        )),
        Arc::new(
            TimestampMicrosecondArray::from(
                revisions
                    .iter()
                    .map(|revision| revision.entry.deleted_at_micros)
                    .collect::<Vec<_>>(),
            )
            .with_timezone("+00:00"),
        ),
        revision_id_array(
            revisions
                .iter()
                .map(|revision| revision.entry.restored_from)
                .collect::<Vec<_>>(),
        )?,
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.entry.external_id.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.entry.updated_by.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.entry.deleted_by.as_deref())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.change_id.as_str())
                .collect::<Vec<_>>(),
        )),
    ];
    for field in &form.fields {
        arrays.push(field_array(
            field,
            schema
                .field_with_name(&field.name)
                .map_err(|error| anyhow!("missing form field schema: {error}"))?,
            revisions,
        )?);
    }
    Ok(RecordBatch::try_new(schema, arrays)?)
}

fn string_list_array(
    arrow_field: &arrow_schema::Field,
    revisions: &[EntryRevision],
    values: impl Fn(&EntryRevision) -> &Vec<String>,
) -> Result<ArrayRef> {
    let element_field = match arrow_field.data_type() {
        arrow_schema::DataType::List(element) => element.clone(),
        kind => return Err(anyhow!("metadata list has invalid Arrow type: {kind:?}")),
    };
    let mut builder = ListBuilder::new(StringBuilder::new()).with_field(element_field);
    for revision in revisions {
        for value in values(revision) {
            builder.values().append_value(value);
        }
        builder.append(true);
    }
    Ok(Arc::new(builder.finish()))
}

fn integrity_array(
    arrow_field: &arrow_schema::Field,
    revisions: &[EntryRevision],
) -> Result<ArrayRef> {
    let fields = match arrow_field.data_type() {
        arrow_schema::DataType::Struct(fields) => fields.clone(),
        kind => return Err(anyhow!("integrity has invalid Arrow type: {kind:?}")),
    };
    let arrays: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.entry.integrity.checksum.as_str())
                .collect::<Vec<_>>(),
        )),
        Arc::new(StringArray::from(
            revisions
                .iter()
                .map(|revision| revision.entry.integrity.signature.as_str())
                .collect::<Vec<_>>(),
        )),
    ];
    Ok(Arc::new(StructArray::try_new(fields, arrays, None)?))
}

fn revision_id_array(revisions: Vec<Option<RevisionId>>) -> Result<ArrayRef> {
    let mut builder = FixedSizeBinaryBuilder::with_capacity(revisions.len(), 16);
    for revision in revisions {
        match revision {
            Some(revision) => builder.append_value(revision.as_uuid().as_bytes())?,
            None => builder.append_null(),
        }
    }
    Ok(Arc::new(builder.finish()))
}

fn typed_list_array(
    field: &FormField,
    arrow_field: &arrow_schema::Field,
    values: Vec<Option<&FieldValue>>,
) -> Result<ArrayRef> {
    let element_field = match arrow_field.data_type() {
        arrow_schema::DataType::List(element) => element.clone(),
        other => return Err(anyhow!("list field has invalid Arrow type: {other:?}")),
    };
    let item_kind = field
        .list_item
        .as_ref()
        .map(|item| &item.field_type)
        .unwrap_or(&FieldType::String);
    if matches!(item_kind, FieldType::AssetReference) {
        let fields = match element_field.data_type() {
            arrow_schema::DataType::Struct(fields) => fields.clone(),
            other => {
                return Err(anyhow!(
                    "asset reference list has invalid element type: {other:?}"
                ))
            }
        };
        let mut builder = ListBuilder::new(StructBuilder::from_fields(fields, values.len()))
            .with_field(element_field);
        for value in values {
            if let Some(FieldValue::List(items)) = value {
                for item in items {
                    if matches!(item, FieldValue::Null) {
                        append_null_asset_reference(builder.values())?;
                    } else {
                        append_asset_reference(builder.values(), item)?;
                    }
                }
                builder.append(true);
            } else {
                builder.append(false);
            }
        }
        return Ok(Arc::new(builder.finish()));
    }

    macro_rules! build_list {
        ($builder:expr, $handler:expr) => {{
            let mut builder = ListBuilder::new($builder).with_field(element_field.clone());
            for value in values {
                if let Some(FieldValue::List(items)) = value {
                    for item in items {
                        if matches!(item, FieldValue::Null) {
                            builder.values().append_null();
                        } else {
                            ($handler)(builder.values(), item)?;
                        }
                    }
                    builder.append(true);
                } else {
                    builder.append(false);
                }
            }
            return Ok(Arc::new(builder.finish()));
        }};
    }

    match item_kind {
        FieldType::String | FieldType::Markdown | FieldType::Sql | FieldType::RowReference => {
            build_list!(
                StringBuilder::new(),
                |builder: &mut StringBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid string item",
                            field.name
                        ));
                    };
                    builder.append_value(value);
                    Ok(())
                }
            );
        }
        FieldType::Boolean => {
            build_list!(
                BooleanBuilder::new(),
                |builder: &mut BooleanBuilder, item: &FieldValue| {
                    let FieldValue::Boolean(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid boolean item",
                            field.name
                        ));
                    };
                    builder.append_value(*value);
                    Ok(())
                }
            );
        }
        FieldType::Integer => {
            build_list!(
                Int32Builder::new(),
                |builder: &mut Int32Builder, item: &FieldValue| {
                    let FieldValue::Integer(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid integer item",
                            field.name
                        ));
                    };
                    builder.append_value(i32::try_from(*value)?);
                    Ok(())
                }
            );
        }
        FieldType::Long => {
            build_list!(
                Int64Builder::new(),
                |builder: &mut Int64Builder, item: &FieldValue| {
                    let FieldValue::Integer(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid long item",
                            field.name
                        ));
                    };
                    builder.append_value(*value);
                    Ok(())
                }
            );
        }
        FieldType::Float => {
            build_list!(
                Float32Builder::new(),
                |builder: &mut Float32Builder, item: &FieldValue| {
                    let value = match item {
                        FieldValue::Integer(value) => *value as f32,
                        FieldValue::Number(value) => *value as f32,
                        _ => {
                            return Err(anyhow!(
                                "typed list field '{}' contains an invalid float item",
                                field.name
                            ))
                        }
                    };
                    builder.append_value(value);
                    Ok(())
                }
            );
        }
        FieldType::Double => {
            build_list!(
                Float64Builder::new(),
                |builder: &mut Float64Builder, item: &FieldValue| {
                    let value = match item {
                        FieldValue::Integer(value) => *value as f64,
                        FieldValue::Number(value) => *value,
                        _ => {
                            return Err(anyhow!(
                                "typed list field '{}' contains an invalid double item",
                                field.name
                            ))
                        }
                    };
                    builder.append_value(value);
                    Ok(())
                }
            );
        }
        FieldType::Date => {
            build_list!(
                Date32Builder::new(),
                |builder: &mut Date32Builder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid date item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_date(value)?);
                    Ok(())
                }
            );
        }
        FieldType::Time => {
            build_list!(
                Time64MicrosecondBuilder::new(),
                |builder: &mut Time64MicrosecondBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid time item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_time_micros(value)?);
                    Ok(())
                }
            );
        }
        FieldType::Timestamp => {
            build_list!(
                TimestampMicrosecondBuilder::new(),
                |builder: &mut TimestampMicrosecondBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid timestamp item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_wall_timestamp_micros(value)?);
                    Ok(())
                }
            );
        }
        FieldType::TimestampTz => {
            build_list!(
                TimestampMicrosecondBuilder::new().with_timezone("+00:00"),
                |builder: &mut TimestampMicrosecondBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid timestamp item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_zoned_timestamp_micros(value)?);
                    Ok(())
                }
            );
        }
        FieldType::TimestampNs => {
            build_list!(
                TimestampNanosecondBuilder::new(),
                |builder: &mut TimestampNanosecondBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid nanosecond timestamp item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_wall_timestamp_nanos(value)?);
                    Ok(())
                }
            );
        }
        FieldType::TimestampTzNs => {
            build_list!(
                TimestampNanosecondBuilder::new().with_timezone("+00:00"),
                |builder: &mut TimestampNanosecondBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid nanosecond timestamp item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_zoned_timestamp_nanos(value)?);
                    Ok(())
                }
            );
        }
        FieldType::Uuid => {
            build_list!(
                FixedSizeBinaryBuilder::with_capacity(values.len(), 16),
                |builder: &mut FixedSizeBinaryBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid UUID item",
                            field.name
                        ));
                    };
                    builder.append_value(Uuid::parse_str(value)?.as_bytes())?;
                    Ok(())
                }
            );
        }
        FieldType::Binary => {
            build_list!(
                LargeBinaryBuilder::new(),
                |builder: &mut LargeBinaryBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid binary item",
                            field.name
                        ));
                    };
                    builder.append_value(
                        BASE64.decode(value.strip_prefix("base64:").unwrap_or(value))?,
                    );
                    Ok(())
                }
            );
        }
        FieldType::List | FieldType::ObjectList | FieldType::AssetReference => Err(anyhow!(
            "typed list field '{}' has an unsupported nested item type",
            field.name
        )),
    }
}

fn asset_reference_array(
    arrow_field: &arrow_schema::Field,
    values: Vec<Option<&FieldValue>>,
) -> Result<ArrayRef> {
    let fields = match arrow_field.data_type() {
        arrow_schema::DataType::Struct(fields) => fields.clone(),
        other => return Err(anyhow!("asset reference has invalid Arrow type: {other:?}")),
    };
    let mut builder = StructBuilder::from_fields(fields, values.len());
    for value in values {
        match value {
            Some(FieldValue::Null) | None => append_null_asset_reference(&mut builder)?,
            Some(value) => append_asset_reference(&mut builder, value)?,
        }
    }
    Ok(Arc::new(builder.finish()))
}

fn append_asset_reference(builder: &mut StructBuilder, value: &FieldValue) -> Result<()> {
    let FieldValue::AssetReference(reference) = value else {
        return Err(anyhow!("asset reference list contains a non-asset value"));
    };
    builder
        .field_builder::<StringBuilder>(0)
        .context("invalid asset_id field builder")?
        .append_value(&reference.asset_id);
    builder
        .field_builder::<StringBuilder>(1)
        .context("invalid asset name field builder")?
        .append_value(&reference.name);
    builder
        .field_builder::<StringBuilder>(2)
        .context("invalid asset media type field builder")?
        .append_value(&reference.media_type);
    builder
        .field_builder::<Int64Builder>(3)
        .context("invalid asset size field builder")?
        .append_value(i64::try_from(reference.size_bytes)?);
    builder
        .field_builder::<StringBuilder>(4)
        .context("invalid asset checksum field builder")?
        .append_value(&reference.sha256);
    builder.append(true);
    Ok(())
}

fn append_null_asset_reference(builder: &mut StructBuilder) -> Result<()> {
    builder
        .field_builder::<StringBuilder>(0)
        .context("invalid asset_id field builder")?
        .append_null();
    builder
        .field_builder::<StringBuilder>(1)
        .context("invalid asset name field builder")?
        .append_null();
    builder
        .field_builder::<StringBuilder>(2)
        .context("invalid asset media type field builder")?
        .append_null();
    builder
        .field_builder::<Int64Builder>(3)
        .context("invalid asset size field builder")?
        .append_null();
    builder
        .field_builder::<StringBuilder>(4)
        .context("invalid asset checksum field builder")?
        .append_null();
    builder.append(false);
    Ok(())
}

fn field_array(
    field: &FormField,
    arrow_field: &arrow_schema::Field,
    revisions: &[EntryRevision],
) -> Result<ArrayRef> {
    let values = revisions
        .iter()
        .map(|revision| revision.values.get(&field.id))
        .collect::<Vec<_>>();
    match &field.field_type {
        FieldType::Boolean => Ok(Arc::new(BooleanArray::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::Boolean(value)) => Some(*value),
                    Some(FieldValue::Null) | None => None,
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ))),
        FieldType::Integer => Ok(Arc::new(Int32Array::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::Integer(value)) => i32::try_from(*value).ok(),
                    Some(FieldValue::Null) | None => None,
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ))),
        FieldType::Long => Ok(Arc::new(Int64Array::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::Integer(value)) => Some(*value),
                    Some(FieldValue::Null) | None => None,
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ))),
        FieldType::Float => Ok(Arc::new(Float32Array::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::Integer(value)) => Some(*value as f32),
                    Some(FieldValue::Number(value)) => Some(*value as f32),
                    Some(FieldValue::Null) | None => None,
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ))),
        FieldType::Double => Ok(Arc::new(Float64Array::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::Integer(value)) => Some(*value as f64),
                    Some(FieldValue::Number(value)) => Some(*value),
                    Some(FieldValue::Null) | None => None,
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ))),
        FieldType::List => Ok(typed_list_array(field, arrow_field, values)?),
        FieldType::AssetReference => Ok(asset_reference_array(arrow_field, values)?),
        FieldType::ObjectList => {
            let element_field = match arrow_field.data_type() {
                arrow_schema::DataType::List(element) => element.clone(),
                other => return Err(anyhow!("object list has invalid Arrow type: {other:?}")),
            };
            let fields = match element_field.data_type() {
                arrow_schema::DataType::Struct(fields) => fields.clone(),
                other => {
                    return Err(anyhow!(
                        "object list element has invalid Arrow type: {other:?}"
                    ))
                }
            };
            let mut builder = ListBuilder::new(StructBuilder::from_fields(fields, revisions.len()))
                .with_field(element_field);
            for value in values {
                if let Some(FieldValue::List(items)) = value {
                    for item in items {
                        let object = match item {
                            FieldValue::Object(object) => object,
                            _ => continue,
                        };
                        for (index, key) in ["type", "name", "description"].iter().enumerate() {
                            builder
                                .values()
                                .field_builder::<StringBuilder>(index)
                                .context("invalid object list field builder")?
                                .append_option(object.get(*key).and_then(|value| match value {
                                    FieldValue::String(value) => Some(value.as_str()),
                                    _ => None,
                                }));
                        }
                        builder.values().append(true);
                    }
                    builder.append(true);
                } else {
                    builder.append(false);
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        FieldType::Date => Ok(Arc::new(Date32Array::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => parse_date(value).map_err(|error| {
                        anyhow!("invalid date value for field '{}': {error}", field.name)
                    }),
                    Some(FieldValue::Null) | None => Ok(None),
                    _ => Err(anyhow!("date field '{}' must be a string", field.name)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        FieldType::Time => Ok(Arc::new(Time64MicrosecondArray::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => parse_time_micros(value).map_err(|error| {
                        anyhow!("invalid time value for field '{}': {error}", field.name)
                    }),
                    Some(FieldValue::Null) | None => Ok(None),
                    _ => Err(anyhow!("time field '{}' must be a string", field.name)),
                })
                .collect::<Result<Vec<_>>>()?,
        ))),
        FieldType::Timestamp => {
            let values = values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => {
                        parse_wall_timestamp_micros(value).map_err(|error| {
                            anyhow!(
                                "invalid timestamp value for field '{}': {error}",
                                field.name
                            )
                        })
                    }
                    Some(FieldValue::Null) | None => Ok(None),
                    _ => Err(anyhow!("timestamp field '{}' must be a string", field.name)),
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Arc::new(TimestampMicrosecondArray::from(values)))
        }
        FieldType::TimestampTz => {
            let values = values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => {
                        parse_zoned_timestamp_micros(value).map_err(|error| {
                            anyhow!(
                                "invalid timestamp value for field '{}': {error}",
                                field.name
                            )
                        })
                    }
                    Some(FieldValue::Null) | None => Ok(None),
                    _ => Err(anyhow!("timestamp field '{}' must be a string", field.name)),
                })
                .collect::<Result<Vec<_>>>()?;
            let array = TimestampMicrosecondArray::from(values);
            Ok(Arc::new(array.with_timezone("+00:00")))
        }
        FieldType::TimestampNs => {
            let values = values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => {
                        parse_wall_timestamp_nanos(value).map_err(|error| {
                            anyhow!(
                                "invalid nanosecond timestamp for field '{}': {error}",
                                field.name
                            )
                        })
                    }
                    Some(FieldValue::Null) | None => Ok(None),
                    _ => Err(anyhow!("timestamp field '{}' must be a string", field.name)),
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Arc::new(TimestampNanosecondArray::from(values)))
        }
        FieldType::TimestampTzNs => {
            let values = values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => {
                        parse_zoned_timestamp_nanos(value).map_err(|error| {
                            anyhow!(
                                "invalid nanosecond timestamp for field '{}': {error}",
                                field.name
                            )
                        })
                    }
                    Some(FieldValue::Null) | None => Ok(None),
                    _ => Err(anyhow!("timestamp field '{}' must be a string", field.name)),
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(Arc::new(
                TimestampNanosecondArray::from(values).with_timezone("+00:00"),
            ))
        }
        FieldType::Uuid => {
            let mut builder = FixedSizeBinaryBuilder::with_capacity(values.len(), 16);
            for value in values {
                match value {
                    Some(FieldValue::String(value)) => builder
                        .append_value(Uuid::parse_str(value)?.as_bytes())
                        .map_err(|error| anyhow!("invalid UUID value: {error}"))?,
                    Some(FieldValue::Null) | None => builder.append_null(),
                    _ => return Err(anyhow!("UUID field '{}' must be a string", field.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        FieldType::Binary => {
            let mut builder = LargeBinaryBuilder::with_capacity(values.len(), 0);
            for value in values {
                match value {
                    Some(FieldValue::String(value)) => builder.append_value(
                        BASE64.decode(value.strip_prefix("base64:").unwrap_or(value))?,
                    ),
                    Some(FieldValue::Null) | None => builder.append_null(),
                    _ => return Err(anyhow!("binary field '{}' must be base64 text", field.name)),
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        _ => Ok(Arc::new(StringArray::from(
            values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => Some(value.clone()),
                    Some(FieldValue::Integer(value)) => Some(value.to_string()),
                    Some(FieldValue::Null) | None => None,
                    _ => None,
                })
                .collect::<Vec<_>>(),
        ))),
    }
}

pub(crate) fn parse_date(value: &str) -> Result<Option<i32>> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid Unix epoch date");
    Ok(Some(date.signed_duration_since(epoch).num_days() as i32))
}

pub(crate) fn parse_time_micros(value: &str) -> Result<Option<i64>> {
    let time = NaiveTime::parse_from_str(value, "%H:%M:%S%.f")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M:%S"))
        // HTML time inputs omit seconds when the value is minute-precise.
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M"))?;
    Ok(Some(
        i64::from(time.num_seconds_from_midnight()) * 1_000_000
            + i64::from(time.nanosecond() / 1_000),
    ))
}

pub(crate) fn parse_wall_timestamp_micros(value: &str) -> Result<Option<i64>> {
    let timestamp = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M"))?;
    Ok(Some(wall_timestamp_micros(timestamp)?))
}

pub(crate) fn parse_zoned_timestamp_micros(value: &str) -> Result<Option<i64>> {
    Ok(Some(
        DateTime::parse_from_rfc3339(value)?.timestamp_micros(),
    ))
}

pub(crate) fn parse_wall_timestamp_nanos(value: &str) -> Result<Option<i64>> {
    let timestamp = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M"))?;
    Ok(Some(wall_timestamp_nanos(timestamp)?))
}

/// Encode a timezone-less Iceberg timestamp as its wall-clock coordinate.
/// This is numeric encoding only; it does not infer or apply a timezone.
fn wall_timestamp_micros(timestamp: NaiveDateTime) -> Result<i64> {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .expect("valid Unix epoch");
    timestamp
        .signed_duration_since(epoch)
        .num_microseconds()
        .context("timestamp is outside the representable microsecond range")
}

/// Encode a timezone-less Iceberg timestamp as its wall-clock coordinate.
/// This is numeric encoding only; it does not infer or apply a timezone.
fn wall_timestamp_nanos(timestamp: NaiveDateTime) -> Result<i64> {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .expect("valid Unix epoch");
    timestamp
        .signed_duration_since(epoch)
        .num_nanoseconds()
        .context("timestamp is outside the representable nanosecond range")
}

pub(crate) fn parse_zoned_timestamp_nanos(value: &str) -> Result<Option<i64>> {
    Ok(Some(
        DateTime::parse_from_rfc3339(value)?
            .timestamp_nanos_opt()
            .context("timestamp is outside the representable nanosecond range")?,
    ))
}

pub(crate) fn uuid_value_at(array: &dyn Array, row: usize) -> Result<Uuid> {
    if array.is_null(row) {
        return Err(anyhow!("UUID column contains a null value"));
    }
    if let Some(values) = array.as_any().downcast_ref::<FixedSizeBinaryArray>() {
        return Ok(Uuid::from_slice(values.value(row))?);
    }
    if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
        return Ok(Uuid::parse_str(values.value(row))?);
    }
    Err(anyhow!("UUID column has unsupported Arrow type"))
}

fn uuid_at(array: &dyn Array, row: usize) -> Result<ugoite_domain::id::EntryId> {
    Ok(ugoite_domain::id::EntryId::from_uuid(uuid_value_at(
        array, row,
    )?))
}

fn revisions_from_batch(
    batch: &RecordBatch,
    form: &FormDefinition,
    table_schema: &iceberg::spec::Schema,
) -> Result<Vec<EntryRevision>> {
    let entry_ids = required_column::<FixedSizeBinaryArray>(batch, "entry_id")?;
    let revision_ids = required_column::<FixedSizeBinaryArray>(batch, "revision_id")?;
    let change_ids = required_column::<StringArray>(batch, "change_id")?;
    let parents = required_column::<FixedSizeBinaryArray>(batch, "parent_revision_id")?;
    let versions = required_column::<Int64Array>(batch, "entry_version")?;
    let operations = required_column::<StringArray>(batch, "operation")?;
    let committed_at = required_column::<TimestampMicrosecondArray>(batch, "committed_at")?;
    let authors = required_column::<StringArray>(batch, "author_id")?;
    let form_versions = required_column::<Int32Array>(batch, "form_version")?;
    let source_kinds = required_column::<StringArray>(batch, "source_kind")?;
    let source_ids = required_column::<StringArray>(batch, "source_id")?;
    let extensions = required_column::<StringArray>(batch, "extension_metadata")?;
    let extra_attributes = required_column::<StringArray>(batch, "extra_attributes")?;
    let titles = required_column::<StringArray>(batch, "ugoite_entry_title")?;
    let tags = required_column::<ListArray>(batch, "ugoite_entry_tags")?;
    let created_at =
        required_column::<TimestampMicrosecondArray>(batch, "ugoite_entry_created_at")?;
    let updated_at =
        required_column::<TimestampMicrosecondArray>(batch, "ugoite_entry_updated_at")?;
    let integrity = required_column::<StructArray>(batch, "ugoite_entry_integrity")?;
    let deleted = required_column::<BooleanArray>(batch, "ugoite_entry_deleted")?;
    let deleted_at =
        required_column::<TimestampMicrosecondArray>(batch, "ugoite_entry_deleted_at")?;
    let restored_from =
        required_column::<FixedSizeBinaryArray>(batch, "ugoite_entry_restored_from")?;
    let external_ids = required_column::<StringArray>(batch, "ugoite_entry_external_id")?;
    let updated_by = batch
        .column_by_name("ugoite_entry_updated_by")
        .and_then(|column| column.as_any().downcast_ref::<StringArray>());
    let deleted_by = batch
        .column_by_name("ugoite_entry_deleted_by")
        .and_then(|column| column.as_any().downcast_ref::<StringArray>());

    let mut revisions = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        let mut values = BTreeMap::new();
        for field in &form.fields {
            let physical = table_schema
                .field_by_id(field.id.get())
                .context("Iceberg schema is missing a Form field ID")?;
            let Some(column) = batch.column_by_name(&physical.name) else {
                // An older file predates this optional Form field.
                continue;
            };
            if let Some(value) = field_value_at(
                column.as_ref(),
                row,
                &field.field_type,
                field.list_item.as_ref(),
            )? {
                values.insert(field.id, value);
            }
        }
        let operation = match required_string(operations, row, "operation")? {
            "upsert" => EntryOperation::Upsert,
            "delete" => EntryOperation::Delete,
            "restore" => EntryOperation::Restore,
            other => return Err(anyhow!("unsupported revision operation: {other}")),
        };
        let entry_version = u64::try_from(required_i64(&versions, row, "entry_version")?)?;
        let parent_revision_id = optional_uuid(parents, row)?.map(RevisionId::from);
        revisions.push(EntryRevision {
            form_id: form.id,
            entry_id: uuid_at(entry_ids, row)?,
            revision_id: RevisionId::from(uuid_value_at(revision_ids, row)?),
            change_id: required_string(change_ids, row, "change_id")?.to_string(),
            parent_revision_id,
            entry_version,
            expected_version: parent_revision_id.map(|_| entry_version.saturating_sub(1)),
            operation,
            committed_at_micros: required_i64(&committed_at, row, "committed_at")?,
            author_id: required_string(authors, row, "author_id")?.to_string(),
            form_version: ugoite_domain::form::FormVersion::new(u32::try_from(required_i32(
                form_versions,
                row,
                "form_version",
            )?)?)?,
            source_kind: required_string(source_kinds, row, "source_kind")?.to_string(),
            source_id: optional_string(source_ids, row),
            entry: EntryMetadata {
                external_id: required_string(external_ids, row, "ugoite_entry_external_id")?
                    .to_string(),
                title: required_string(titles, row, "ugoite_entry_title")?.to_string(),
                tags: string_list_at(tags, row)?,
                created_at_micros: required_i64(&created_at, row, "ugoite_entry_created_at")?,
                updated_at_micros: required_i64(&updated_at, row, "ugoite_entry_updated_at")?,
                updated_by: updated_by.map_or_else(
                    || Ok(required_string(authors, row, "author_id")?.to_string()),
                    |values| {
                        required_string(values, row, "ugoite_entry_updated_by").map(str::to_owned)
                    },
                )?,
                integrity: integrity_at(integrity, row)?,
                deleted: required_bool(deleted, row, "ugoite_entry_deleted")?,
                deleted_at_micros: optional_i64(&deleted_at, row),
                deleted_by: deleted_by.and_then(|values| optional_string(values, row)),
                restored_from: optional_uuid(restored_from, row)?.map(RevisionId::from),
            },
            values,
            extra_attributes: json_map_at(extra_attributes, row, "extra_attributes")?,
            extension_metadata: json_map_at(extensions, row, "extension_metadata")?,
        });
    }
    Ok(revisions)
}

fn required_column<'a, T: 'static>(batch: &'a RecordBatch, name: &str) -> Result<&'a T> {
    batch
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<T>())
        .with_context(|| format!("Iceberg projection has invalid {name} column"))
}

fn required_string<'a>(array: &'a StringArray, row: usize, name: &str) -> Result<&'a str> {
    (!array.is_null(row))
        .then(|| array.value(row))
        .with_context(|| format!("Iceberg {name} is null"))
}

fn optional_string(array: &StringArray, row: usize) -> Option<String> {
    (!array.is_null(row)).then(|| array.value(row).to_string())
}

fn required_i64(
    array: &impl arrow_array::ArrayAccessor<Item = i64>,
    row: usize,
    name: &str,
) -> Result<i64> {
    (!array.is_null(row))
        .then(|| array.value(row))
        .with_context(|| format!("Iceberg {name} is null"))
}

fn optional_i64(array: &impl arrow_array::ArrayAccessor<Item = i64>, row: usize) -> Option<i64> {
    (!array.is_null(row)).then(|| array.value(row))
}

fn required_i32(array: &Int32Array, row: usize, name: &str) -> Result<i32> {
    (!array.is_null(row))
        .then(|| array.value(row))
        .with_context(|| format!("Iceberg {name} is null"))
}

fn required_bool(array: &BooleanArray, row: usize, name: &str) -> Result<bool> {
    (!array.is_null(row))
        .then(|| array.value(row))
        .with_context(|| format!("Iceberg {name} is null"))
}

fn optional_uuid(array: &FixedSizeBinaryArray, row: usize) -> Result<Option<Uuid>> {
    Ok((!array.is_null(row))
        .then(|| Uuid::from_slice(array.value(row)))
        .transpose()?)
}

fn json_map_at(
    array: &StringArray,
    row: usize,
    name: &str,
) -> Result<BTreeMap<String, serde_json::Value>> {
    if array.is_null(row) || array.value(row).is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_str(array.value(row)).with_context(|| format!("invalid {name} JSON"))
}

fn string_list_at(array: &ListArray, row: usize) -> Result<Vec<String>> {
    if array.is_null(row) {
        return Ok(Vec::new());
    }
    let values = array.value(row);
    let values = values
        .as_any()
        .downcast_ref::<StringArray>()
        .context("Iceberg list metadata has invalid element type")?;
    Ok((0..values.len())
        .filter(|index| !values.is_null(*index))
        .map(|index| values.value(index).to_string())
        .collect())
}

fn metadata_rows_at(array: &ListArray, row: usize) -> Result<ArrayRef> {
    let values = array.value(row);
    if values.as_any().downcast_ref::<StructArray>().is_none() {
        return Err(anyhow!("Iceberg metadata list has invalid element type"));
    }
    Ok(values)
}

fn struct_string_at(array: &StructArray, name: &str, row: usize) -> String {
    array
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<StringArray>())
        .filter(|column| !column.is_null(row))
        .map(|column| column.value(row).to_string())
        .unwrap_or_default()
}

fn required_struct_string_at(array: &StructArray, name: &str, row: usize) -> Result<String> {
    let column = array
        .column_by_name(name)
        .with_context(|| format!("asset reference is missing {name}"))?
        .as_any()
        .downcast_ref::<StringArray>()
        .with_context(|| format!("asset reference {name} has the wrong Arrow type"))?;
    if column.is_null(row) {
        return Err(anyhow!("asset reference {name} is null"));
    }
    Ok(column.value(row).to_string())
}

fn required_struct_i64_at(array: &StructArray, name: &str, row: usize) -> Result<i64> {
    let column = array
        .column_by_name(name)
        .with_context(|| format!("asset reference is missing {name}"))?
        .as_any()
        .downcast_ref::<Int64Array>()
        .with_context(|| format!("asset reference {name} has the wrong Arrow type"))?;
    if column.is_null(row) {
        return Err(anyhow!("asset reference {name} is null"));
    }
    Ok(column.value(row))
}

fn asset_reference_at(array: &StructArray, row: usize) -> Result<FieldValue> {
    let reference = AssetReference {
        asset_id: required_struct_string_at(array, "asset_id", row)?,
        name: required_struct_string_at(array, "name", row)?,
        media_type: required_struct_string_at(array, "media_type", row)?,
        size_bytes: u64::try_from(required_struct_i64_at(array, "size_bytes", row)?)
            .context("asset reference size_bytes must be non-negative")?,
        sha256: required_struct_string_at(array, "sha256", row)?,
    };
    reference
        .validate()
        .map_err(|error| anyhow!("invalid persisted AssetReference: {error}"))?;
    Ok(FieldValue::AssetReference(reference))
}

fn integrity_at(array: &StructArray, row: usize) -> Result<EntryIntegrity> {
    if array.is_null(row) {
        return Ok(EntryIntegrity::default());
    }
    Ok(EntryIntegrity {
        checksum: struct_string_at(array, "checksum", row),
        signature: struct_string_at(array, "signature", row),
    })
}

fn field_value_at(
    column: &dyn Array,
    row: usize,
    kind: &FieldType,
    list_item: Option<&ListItemDefinition>,
) -> Result<Option<FieldValue>> {
    if column.is_null(row) {
        return Ok(None);
    }
    let invalid = || anyhow!("Iceberg field type does not match Form metadata");
    let value = match kind {
        FieldType::Boolean => FieldValue::Boolean(
            column
                .as_any()
                .downcast_ref::<BooleanArray>()
                .ok_or_else(invalid)?
                .value(row),
        ),
        FieldType::Integer | FieldType::Long => {
            FieldValue::Integer(if matches!(kind, FieldType::Integer) {
                i64::from(
                    column
                        .as_any()
                        .downcast_ref::<Int32Array>()
                        .ok_or_else(invalid)?
                        .value(row),
                )
            } else {
                column
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .ok_or_else(invalid)?
                    .value(row)
            })
        }
        FieldType::Float => FieldValue::Number(f64::from(
            column
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(invalid)?
                .value(row),
        )),
        FieldType::Double => FieldValue::Number(
            column
                .as_any()
                .downcast_ref::<Float64Array>()
                .ok_or_else(invalid)?
                .value(row),
        ),
        FieldType::Date => FieldValue::String(date_from_days(
            column
                .as_any()
                .downcast_ref::<Date32Array>()
                .ok_or_else(invalid)?
                .value(row),
        )?),
        FieldType::Time => FieldValue::String(time_from_micros(
            column
                .as_any()
                .downcast_ref::<Time64MicrosecondArray>()
                .ok_or_else(invalid)?
                .value(row),
        )?),
        FieldType::Timestamp => FieldValue::String(wall_timestamp_from_micros(
            column
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .ok_or_else(invalid)?
                .value(row),
        )?),
        FieldType::TimestampTz => FieldValue::String(timestamp_from_micros(
            column
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .ok_or_else(invalid)?
                .value(row),
        )?),
        FieldType::TimestampNs => FieldValue::String(wall_timestamp_from_nanos(
            column
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .ok_or_else(invalid)?
                .value(row),
        )?),
        FieldType::TimestampTzNs => FieldValue::String(timestamp_from_nanos(
            column
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .ok_or_else(invalid)?
                .value(row),
        )?),
        FieldType::Uuid => FieldValue::String(
            Uuid::from_slice(
                column
                    .as_any()
                    .downcast_ref::<FixedSizeBinaryArray>()
                    .ok_or_else(invalid)?
                    .value(row),
            )?
            .to_string(),
        ),
        FieldType::Binary => FieldValue::String(format!(
            "base64:{}",
            BASE64.encode(
                column
                    .as_any()
                    .downcast_ref::<LargeBinaryArray>()
                    .ok_or_else(invalid)?
                    .value(row),
            )
        )),
        FieldType::List => FieldValue::List(typed_list_at(
            column
                .as_any()
                .downcast_ref::<ListArray>()
                .ok_or_else(invalid)?,
            row,
            list_item,
        )?),
        FieldType::ObjectList => FieldValue::List(object_list_at(
            column
                .as_any()
                .downcast_ref::<ListArray>()
                .ok_or_else(invalid)?,
            row,
        )?),
        FieldType::AssetReference => {
            let value = column
                .as_any()
                .downcast_ref::<StructArray>()
                .ok_or_else(invalid)?;
            asset_reference_at(value, row)?
        }
        FieldType::String | FieldType::Markdown | FieldType::Sql | FieldType::RowReference => {
            FieldValue::String(
                column
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .ok_or_else(invalid)?
                    .value(row)
                    .to_string(),
            )
        }
    };
    Ok(Some(value))
}

fn typed_list_at(
    array: &ListArray,
    row: usize,
    item: Option<&ListItemDefinition>,
) -> Result<Vec<FieldValue>> {
    if array.is_null(row) {
        return Ok(Vec::new());
    }
    let values = array.value(row);
    let item_kind = item
        .map(|item| &item.field_type)
        .unwrap_or(&FieldType::String);
    (0..values.len())
        .map(|index| {
            Ok(
                field_value_at(values.as_ref(), index, item_kind, None)?
                    .unwrap_or(FieldValue::Null),
            )
        })
        .collect()
}

fn object_list_at(array: &ListArray, row: usize) -> Result<Vec<FieldValue>> {
    if array.is_null(row) {
        return Ok(Vec::new());
    }
    let values = metadata_rows_at(array, row)?;
    let values = values
        .as_any()
        .downcast_ref::<StructArray>()
        .expect("validated metadata struct array");
    Ok((0..values.len())
        .map(|index| {
            FieldValue::Object(BTreeMap::from([
                (
                    "type".into(),
                    FieldValue::String(struct_string_at(values, "type", index)),
                ),
                (
                    "name".into(),
                    FieldValue::String(struct_string_at(values, "name", index)),
                ),
                (
                    "description".into(),
                    FieldValue::String(struct_string_at(values, "description", index)),
                ),
            ]))
        })
        .collect())
}

fn date_from_days(days: i32) -> Result<String> {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).context("invalid Unix epoch")?;
    Ok(epoch
        .checked_add_signed(chrono::Duration::days(i64::from(days)))
        .context("date is outside the supported range")?
        .format("%Y-%m-%d")
        .to_string())
}

fn time_from_micros(micros: i64) -> Result<String> {
    let seconds = u32::try_from(micros.div_euclid(1_000_000))?;
    let nanos = u32::try_from(micros.rem_euclid(1_000_000) * 1_000)?;
    Ok(
        NaiveTime::from_num_seconds_from_midnight_opt(seconds, nanos)
            .context("time is outside the supported range")?
            .format("%H:%M:%S%.6f")
            .to_string(),
    )
}

fn timestamp_from_micros(micros: i64) -> Result<String> {
    DateTime::from_timestamp_micros(micros)
        .context("timestamp is outside the supported range")
        .map(|timestamp: DateTime<chrono::Utc>| timestamp.to_rfc3339())
}

fn wall_timestamp_from_micros(micros: i64) -> Result<String> {
    DateTime::from_timestamp_micros(micros)
        .context("timestamp is outside the supported range")
        .map(|timestamp: DateTime<chrono::Utc>| {
            let naive = timestamp.naive_utc();
            let base = naive.format("%Y-%m-%dT%H:%M:%S").to_string();
            let fraction = format!("{:06}", naive.and_utc().timestamp_subsec_micros())
                .trim_end_matches('0')
                .to_string();
            if fraction.is_empty() {
                base
            } else {
                format!("{base}.{fraction}")
            }
        })
}

fn timestamp_from_nanos(nanos: i64) -> Result<String> {
    let seconds = nanos.div_euclid(1_000_000_000);
    let nanos = u32::try_from(nanos.rem_euclid(1_000_000_000))?;
    DateTime::from_timestamp(seconds, nanos)
        .context("timestamp is outside the supported range")
        .map(|timestamp: DateTime<chrono::Utc>| {
            timestamp.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
        })
}

fn wall_timestamp_from_nanos(nanos: i64) -> Result<String> {
    let seconds = nanos.div_euclid(1_000_000_000);
    let nanos = u32::try_from(nanos.rem_euclid(1_000_000_000))?;
    DateTime::from_timestamp(seconds, nanos)
        .context("timestamp is outside the supported range")
        .map(|timestamp: DateTime<chrono::Utc>| {
            let naive = timestamp.naive_utc();
            let base = naive.format("%Y-%m-%dT%H:%M:%S").to_string();
            let fraction = format!("{:09}", naive.nanosecond())
                .trim_end_matches('0')
                .to_string();
            if fraction.is_empty() {
                base
            } else {
                format!("{base}.{fraction}")
            }
        })
}

fn validate_batch_revision_metadata(
    batches: &[RecordBatch],
    revisions: &[EntryRevision],
) -> Result<()> {
    let mut row_index = 0;
    for batch in batches {
        let entry_ids = batch
            .column_by_name("entry_id")
            .context("record batch is missing entry_id")?;
        let revision_ids = batch
            .column_by_name("revision_id")
            .context("record batch is missing revision_id")?;
        let parents = batch
            .column_by_name("parent_revision_id")
            .context("record batch is missing parent_revision_id")?;
        let entry_versions = batch
            .column_by_name("entry_version")
            .and_then(|array| array.as_any().downcast_ref::<Int64Array>())
            .context("entry_version must be an int64 column")?;
        let form_versions = batch
            .column_by_name("form_version")
            .and_then(|array| array.as_any().downcast_ref::<arrow_array::Int32Array>())
            .context("form_version must be an int32 column")?;
        let operations = batch
            .column_by_name("operation")
            .and_then(|array| array.as_any().downcast_ref::<StringArray>())
            .context("operation must be a string column")?;
        for row in 0..batch.num_rows() {
            let revision = revisions
                .get(row_index)
                .context("record batch has more rows than revision metadata")?;
            let parent = if parents.is_null(row) {
                None
            } else {
                Some(ugoite_domain::id::RevisionId::from_uuid(uuid_value_at(
                    parents.as_ref(),
                    row,
                )?))
            };
            let entry_version = u64::try_from(entry_versions.value(row))
                .map_err(|_| anyhow!("entry_version must be non-negative"))?;
            let form_version = u32::try_from(form_versions.value(row))
                .map_err(|_| anyhow!("form_version must be positive"))?;
            let operation = match revision.operation {
                EntryOperation::Upsert => "upsert",
                EntryOperation::Delete => "delete",
                EntryOperation::Restore => "restore",
            };
            let updated_by = batch
                .column_by_name("ugoite_entry_updated_by")
                .and_then(|array| array.as_any().downcast_ref::<StringArray>())
                .context("ugoite_entry_updated_by must be a string column")?;
            let deleted_by = batch
                .column_by_name("ugoite_entry_deleted_by")
                .and_then(|array| array.as_any().downcast_ref::<StringArray>())
                .context("ugoite_entry_deleted_by must be a string column")?;
            if uuid_value_at(entry_ids.as_ref(), row)? != revision.entry_id.as_uuid()
                || uuid_value_at(revision_ids.as_ref(), row)? != revision.revision_id.as_uuid()
                || parent != revision.parent_revision_id
                || entry_version != revision.entry_version
                || form_version != revision.form_version.get()
                || operations.is_null(row)
                || operations.value(row) != operation
                || updated_by.is_null(row)
                || updated_by.value(row) != revision.entry.updated_by
                || (deleted_by.is_null(row) != revision.entry.deleted_by.is_none())
                || (!deleted_by.is_null(row)
                    && deleted_by.value(row)
                        != revision.entry.deleted_by.as_deref().unwrap_or_default())
            {
                return Err(anyhow!(
                    "record batch revision metadata does not match revision metadata"
                ));
            }
            row_index += 1;
        }
    }
    if row_index != revisions.len() {
        return Err(anyhow!(
            "record batch has fewer rows than revision metadata"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod invariant_tests {
    use super::*;
    use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision};
    use ugoite_domain::form::FormVersion;
    use ugoite_domain::id::{EntryId, FieldId};

    fn form() -> FormDefinition {
        FormDefinition {
            id: FormId::from(Uuid::from_u128(18_510)),
            version: FormVersion::new(1).expect("valid test Form version"),
            name: "InvariantTest".into(),
            description: None,
            fields: vec![FormField {
                id: FieldId::new(100).expect("valid test field id"),
                name: "title".into(),
                field_type: FieldType::String,
                required: false,
                label: None,
                description: None,
                semantic_role: None,
                reference_form: None,
                list_item: None,
                validation: None,
                enum_values: Vec::new(),
                deprecated: false,
            }],
            allow_extra_attributes: false,
            extension_metadata: BTreeMap::new(),
        }
    }

    fn revision(form: &FormDefinition, revision_id: u128, title: &str) -> EntryRevision {
        EntryRevision {
            form_id: form.id,
            entry_id: EntryId::from(Uuid::from_u128(18_511)),
            revision_id: RevisionId::from(Uuid::from_u128(revision_id)),
            change_id: format!("change-{revision_id}"),
            parent_revision_id: None,
            entry_version: 1,
            expected_version: None,
            operation: EntryOperation::Upsert,
            committed_at_micros: revision_id as i64,
            author_id: "test".into(),
            form_version: form.version,
            source_kind: "test".into(),
            source_id: None,
            entry: EntryMetadata {
                updated_by: "test".into(),
                ..EntryMetadata::default()
            },
            values: BTreeMap::from([(
                FieldId::new(100).expect("valid test field id"),
                FieldValue::String(title.into()),
            )]),
            extra_attributes: BTreeMap::new(),
            extension_metadata: BTreeMap::new(),
        }
    }

    async fn append_duplicate_without_product_bypass(
        workspace: &IcebergWorkspace,
        form_id: FormId,
        revision: EntryRevision,
    ) -> Result<()> {
        let mut attempt_workspace = workspace.clone();
        let catalog = workspace
            .space_catalog
            .as_ref()
            .context("test fixture requires a SpaceCatalog")?
            .new_attempt();
        attempt_workspace.catalog = Arc::new(catalog.clone());
        attempt_workspace.space_catalog = Some(Arc::new(catalog));
        let form = attempt_workspace.load_form(form_id).await?;
        let table = attempt_workspace
            .catalog
            .load_table(&attempt_workspace.form_ident(form_id))
            .await?;
        let batch = revision_batch_from_values(
            &form,
            table.metadata().current_schema(),
            std::slice::from_ref(&revision),
        )?;
        attempt_workspace
            .append_record_batches_inner(form_id, vec![batch], &[revision], false)
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn duplicate_maximum_versions_are_rejected_by_authorized_reads() -> Result<()> {
        let workspace = IcebergWorkspace::memory_for_tests(
            SpaceId::from(Uuid::from_u128(18_512)),
            "memory://iceberg-private-invariant-fixture",
        )
        .await?;
        let form = form();
        workspace
            .commit(publication_context("test-form", "test.form", &form)?)?
            .create_form(&form)
            .await?;
        append_duplicate_without_product_bypass(
            &workspace,
            form.id,
            revision(&form, 18_513, "left"),
        )
        .await?;
        append_duplicate_without_product_bypass(
            &workspace,
            form.id,
            revision(&form, 18_514, "right"),
        )
        .await?;

        let error = workspace
            .read_revision_view(form.id, RevisionView::LatestIncludingTombstones)
            .await
            .expect_err("duplicate maximum Entry versions must remain a read invariant");
        assert!(error
            .to_string()
            .contains("multiple revisions share a maximum entry_version"));
        Ok(())
    }
}

#[cfg(test)]
mod asset_reference_decode_tests {
    use super::*;

    fn asset_fields() -> arrow_schema::Fields {
        vec![
            arrow_schema::Field::new("asset_id", arrow_schema::DataType::Utf8, true),
            arrow_schema::Field::new("name", arrow_schema::DataType::Utf8, true),
            arrow_schema::Field::new("media_type", arrow_schema::DataType::Utf8, true),
            arrow_schema::Field::new("size_bytes", arrow_schema::DataType::Int64, true),
            arrow_schema::Field::new("sha256", arrow_schema::DataType::Utf8, true),
        ]
        .into()
    }

    fn valid_asset() -> StructArray {
        StructArray::new(
            asset_fields(),
            vec![
                Arc::new(StringArray::from(vec![Some("asset-1")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("file.txt")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("text/plain")])) as ArrayRef,
                Arc::new(Int64Array::from(vec![Some(4)])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )])) as ArrayRef,
            ],
            None,
        )
    }

    #[test]
    fn asset_reference_decode_requires_a_complete_typed_struct() {
        let value = field_value_at(&valid_asset(), 0, &FieldType::AssetReference, None).unwrap();
        assert_eq!(
            value,
            Some(FieldValue::AssetReference(AssetReference {
                asset_id: "asset-1".into(),
                name: "file.txt".into(),
                media_type: "text/plain".into(),
                size_bytes: 4,
                sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            }))
        );

        let null_parent = StructArray::new_null(asset_fields(), 1);
        assert_eq!(
            field_value_at(&null_parent, 0, &FieldType::AssetReference, None).unwrap(),
            None
        );

        let mut missing_fields = asset_fields().to_vec();
        missing_fields.remove(0);
        let missing = StructArray::new(
            missing_fields.into(),
            vec![
                Arc::new(StringArray::from(vec![Some("file.txt")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("text/plain")])) as ArrayRef,
                Arc::new(Int64Array::from(vec![Some(4)])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )])) as ArrayRef,
            ],
            None,
        );
        assert!(asset_reference_at(&missing, 0).is_err());

        let null_size = StructArray::new(
            asset_fields(),
            vec![
                Arc::new(StringArray::from(vec![Some("asset-1")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("file.txt")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("text/plain")])) as ArrayRef,
                Arc::new(Int64Array::from(vec![None])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )])) as ArrayRef,
            ],
            None,
        );
        assert!(asset_reference_at(&null_size, 0).is_err());

        let negative_size = StructArray::new(
            asset_fields(),
            vec![
                Arc::new(StringArray::from(vec![Some("asset-1")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("file.txt")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("text/plain")])) as ArrayRef,
                Arc::new(Int64Array::from(vec![Some(-1)])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )])) as ArrayRef,
            ],
            None,
        );
        assert!(asset_reference_at(&negative_size, 0).is_err());

        let wrong_type = StructArray::new(
            vec![
                arrow_schema::Field::new("asset_id", arrow_schema::DataType::Int64, true),
                arrow_schema::Field::new("name", arrow_schema::DataType::Utf8, true),
                arrow_schema::Field::new("media_type", arrow_schema::DataType::Utf8, true),
                arrow_schema::Field::new("size_bytes", arrow_schema::DataType::Int64, true),
                arrow_schema::Field::new("sha256", arrow_schema::DataType::Utf8, true),
            ]
            .into(),
            vec![
                Arc::new(Int64Array::from(vec![Some(1)])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("file.txt")])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some("text/plain")])) as ArrayRef,
                Arc::new(Int64Array::from(vec![Some(4)])) as ArrayRef,
                Arc::new(StringArray::from(vec![Some(
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )])) as ArrayRef,
            ],
            None,
        );
        assert!(asset_reference_at(&wrong_type, 0).is_err());
    }
}
