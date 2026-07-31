//! Iceberg-native persistence and query boundary.
//!
//! One [`IcebergWorkspace`] represents one Ugoite Space namespace. Production
//! callers inject a durable Catalog; every built-in test workspace uses the
//! same OpenDAL-backed SpaceCatalog boundary as production.

mod migration;
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
pub mod storage;

pub use health::SpaceHealthReport;
pub use migration::{MigrationFormReport, MigrationManifest, MigrationReport};
pub use space_catalog::PublicationContext;
use space_catalog::SpaceCatalog;
pub use ugoite_domain::checkpoint::{CheckpointTable, SpaceCheckpoint};

use anyhow::{anyhow, Context, Result};
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
use iceberg::expr::Reference;
use iceberg::spec::{DataFileFormat, Datum};
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
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};
use ugoite_core::error::AppError;
use ugoite_core::query::{
    AuthorizedQueryForm, AuthorizedQueryPolicy, EntryScope, QueryLimits, QuerySystemColumn,
};
use ugoite_domain::entry::{
    AssetReference, EntryIntegrity, EntryMetadata, EntryOperation,
    EntryRevision, FieldValue,
};
use ugoite_domain::form::{
    Compatibility, FieldType, FormChange, FormChangeSet, FormDefinition, FormField,
    ListItemDefinition,
};
use ugoite_domain::id::{validate_checkpoint_name, FormId, RevisionId, SpaceId};
use ugoite_storage::{operator_from_uri, SpaceCatalogStore};
use uuid::Uuid;

const FORM_DEFINITION_PROPERTY: &str = "ugoite.form.definition.v1";
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
    warehouse: String,
    write: WriteConfig,
}

/// Query permits are process-wide per Space coordinate. A request creates a
/// short-lived authorization context, but it must not thereby create a fresh
/// production concurrency budget.
static SPACE_QUERY_PERMITS: LazyLock<Mutex<HashMap<String, Arc<Semaphore>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

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
        permits
            .entry(key)
            .or_insert_with(|| Arc::new(Semaphore::new(max_concurrency)))
            .clone()
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
            catalog.create_namespace(&namespace, HashMap::new()).await?;
        }
        Ok(Self {
            catalog: catalog.clone(),
            space_catalog: Some(catalog),
            namespace,
            space_id,
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

    /// Persists a named immutable checkpoint through the Space's OpenDAL
    /// boundary. Reusing a name fails rather than silently replacing history.
    pub async fn save_checkpoint(&self, name: &str, checkpoint: &SpaceCheckpoint) -> Result<()> {
        validate_checkpoint_name(name)?;
        self.validate_checkpoint(checkpoint)?;
        self.space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .validate_checkpoint_evidence(checkpoint)
            .await?;
        self.space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .validate_checkpoint_tables(checkpoint)
            .await?;
        let mut stored = checkpoint.clone();
        stored.name = Some(name.to_string());
        self.space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .create_checkpoint(name, &stored)
            .await
    }

    /// Loads a durable checkpoint by name. A missing object is represented by
    /// [`CheckpointUnavailable`], never by an empty or current-head fallback.
    pub async fn load_checkpoint(&self, name: &str) -> Result<SpaceCheckpoint> {
        validate_checkpoint_name(name)?;
        let checkpoint = self
            .space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .read_checkpoint(name)
            .await?;
        if checkpoint.name.as_deref() != Some(name) {
            return Err(CheckpointIntegrityError::new(
                "stored checkpoint name does not match its object name",
            )
            .into());
        }
        self.validate_checkpoint(&checkpoint)?;
        self.space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .validate_checkpoint_evidence(&checkpoint)
            .await?;
        self.space_catalog
            .as_ref()
            .context("SpaceCheckpoint requires the OpenDAL-backed SpaceCatalog")?
            .validate_checkpoint_tables(&checkpoint)
            .await?;
        Ok(checkpoint)
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
        self.read_revision_view_from_table(&form, table, view, coordinate.snapshot_id)
            .await
            .map_err(checkpoint_query_error)
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
            .filter(|coordinate| coordinate.form_name.eq_ignore_ascii_case(&relation));
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
        if !form.name.eq_ignore_ascii_case(&relation) || coordinate.form_name != form.name {
            return Err(CheckpointIntegrityError::new(
                "checkpoint Form relation does not match immutable Iceberg metadata",
            )
            .into());
        }
        Ok(form)
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

    /// Returns whether the authoritative Catalog Head currently contains this
    /// Form table. This is a domain read and intentionally exposes neither a
    /// physical table handle nor a mutation path.
    pub async fn has_form(&self, form_id: FormId) -> Result<bool> {
        Ok(self.catalog.table_exists(&self.form_ident(form_id)).await?)
    }

    pub async fn list_forms(&self) -> Result<Vec<FormDefinition>> {
        let mut forms = Vec::new();
        for ident in self.catalog.list_tables(&self.namespace).await? {
            let table = self.catalog.load_table(&ident).await?;
            if let Some(raw) = table.metadata().properties().get(FORM_DEFINITION_PROPERTY) {
                forms.push(serde_json::from_str(raw)?);
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
            Compatibility::MigrationRequired => {
                return Err(anyhow!("Form change requires a populated migration plan"))
            }
            Compatibility::Compatible => {}
        }
        let evolved = current.apply(changes)?;
        let table = self
            .catalog
            .load_table(&self.form_ident(current.id))
            .await?;
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
            let metadata = table
                .metadata()
                .clone()
                .into_builder(Some(table.metadata_location_result()?.to_string()))
                .add_current_schema(schema)?
                .set_properties(form_properties(&evolved, self.write)?)?
                .build()?
                .metadata;
            space_catalog
                .replace_table_metadata(table.identifier(), metadata)
                .await?;
            return self.load_form(changes.form_id).await;
        }
        if additions.is_empty() {
            let tx = Transaction::new(&table);
            let mut action = tx.update_table_properties();
            for (key, value) in form_properties(&evolved, self.write)? {
                action = action.set(key, value);
            }
            let catalog = self.mutation_catalog();
            action.apply(tx)?.commit(catalog.as_ref()).await?;
            return Ok(evolved);
        }
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
        for (key, value) in form_properties(&evolved, self.write)? {
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
                            anyhow!("row_reference field '{}' has no target Form", field.name)
                        })?;
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
                                anyhow!(
                                    "row_reference list field '{}' has no target Form",
                                    field.name
                                )
                            })?;
                        for value in values {
                            if let FieldValue::String(entry_id) = value {
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
        for (target_form_id, _) in &references {
            let target_form = target_forms.get(target_form_id).ok_or_else(|| {
                anyhow!("row_reference target Form {target_form_id} does not exist")
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
                    relation: target_form.name.to_ascii_lowercase(),
                    entry_scope,
                    columns: target_form
                        .fields
                        .iter()
                        .map(|field| field.name.clone())
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
                anyhow!("row_reference target Form {target_form_id} does not exist")
            })?;
            let relation = format!(
                "\"{}\"",
                target_form.name.to_ascii_lowercase().replace('"', "\"\"")
            );
            let literal = format!("'{}'", entry_id.replace('\'', "''"));
            let rows = context
                .execute(&format!(
                    "SELECT 1 FROM {relation} WHERE _ugoite_id = {literal} LIMIT 1"
                ))
                .await?;
            if rows.iter().map(|batch| batch.num_rows()).sum::<usize>() == 0 {
                return Err(anyhow!(
                    "row_reference target Entry '{}' does not belong to Form '{}'",
                    entry_id,
                    target_form.name
                ));
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
                let asset_ids = match (&field.field_type, value) {
                    (FieldType::AssetReference, FieldValue::AssetReference(reference)) => {
                        vec![reference.asset_id.as_str()]
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
                                    Some(reference.asset_id.as_str())
                                }
                                _ => None,
                            })
                            .collect()
                    }
                    _ => Vec::new(),
                };
                for asset_id in asset_ids {
                    if self.asset_is_deleted(asset_id).await? {
                        return Err(anyhow!(
                            "Asset '{}' is unavailable because it was deleted",
                            asset_id
                        ));
                    }
                }
            }
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

    pub(crate) async fn append_record_batches(
        &self,
        form_id: FormId,
        batches: Vec<RecordBatch>,
        revisions: &[EntryRevision],
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
        let mut current = self
            .read_latest_revisions_for_entries(form_id, &entry_ids)
            .await?
            .into_iter()
            .map(|revision| (revision.entry_id, revision))
            .collect::<HashMap<_, _>>();
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
            let previous = current.get(&revision.entry_id);
            if let Some(previous) = previous {
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
            revision.validate_payload(&form)?;
            current.insert(revision.entry_id, revision.clone());
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
        );
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
            revision_batch_from_values(&form, table.metadata().current_schema(), &revisions)?;
        self.append_record_batches(form_id, vec![batch], &revisions)
            .await
    }

    /// Reads canonical revisions through Iceberg's Arrow projection. Physical
    /// column decoding lives in this adapter; callers receive only domain
    /// revisions and never Arrow arrays or Iceberg tables.
    pub async fn read_revisions(&self, form_id: FormId) -> Result<Vec<EntryRevision>> {
        let form = self.load_form(form_id).await?;
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        let table_schema = table.metadata().current_schema().clone();
        let mut stream = table.scan().build()?.to_arrow().await?;
        let mut revisions = Vec::new();
        while let Some(batch) = futures::TryStreamExt::try_next(&mut stream).await? {
            revisions.extend(revisions_from_batch(&batch, &form, &table_schema)?);
        }
        Ok(revisions)
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
        self.read_revision_view_with_snapshot(form_id, view, None)
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
        self.read_revision_view_from_table(&form, table, view, snapshot_id)
            .await
    }

    async fn read_revision_view_from_table(
        &self,
        form: &FormDefinition,
        table: iceberg::table::Table,
        view: RevisionView,
        snapshot_id: Option<i64>,
    ) -> Result<Vec<EntryRevision>> {
        let batches = match view {
            RevisionView::All => {
                let scan = match snapshot_id {
                    Some(snapshot_id) => table.scan().snapshot_id(snapshot_id),
                    None => table.scan(),
                };
                let mut stream = scan.build()?.to_arrow().await?;
                let mut batches = Vec::new();
                while let Some(batch) = futures::TryStreamExt::try_next(&mut stream).await? {
                    batches.push(batch);
                }
                batches
            }
            RevisionView::LatestIncludingTombstones | RevisionView::Current => {
                self.read_latest_revision_batches(&table, None, snapshot_id, view)
                    .await?
            }
        };
        let schema = table.metadata().current_schema().clone();
        let mut revisions = Vec::new();
        for batch in &batches {
            revisions.extend(revisions_from_batch(batch, form, &schema)?);
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
                Some(entry_ids),
                None,
                RevisionView::LatestIncludingTombstones,
            )
            .await?;
        let mut revisions = Vec::new();
        for batch in &batches {
            revisions.extend(revisions_from_batch(batch, &form, &schema)?);
        }
        Ok(revisions)
    }

    async fn latest_revision_plan(
        &self,
        table: &iceberg::table::Table,
        entry_ids: Option<&[ugoite_domain::id::EntryId]>,
        snapshot_id: Option<i64>,
        view: RevisionView,
    ) -> Result<Vec<RecordBatch>> {
        let context = SessionContext::new();
        let provider = if let Some(snapshot_id) = snapshot_id {
            IcebergStaticTableProvider::try_new_from_table_snapshot(table.clone(), snapshot_id)
                .await?
        } else {
            IcebergStaticTableProvider::try_new_from_table(table.clone()).await?
        };
        context.register_table("revisions", Arc::new(provider))?;
        let revisions = context.table("revisions").await?;
        let scope = match entry_ids {
            Some([]) => return Ok(Vec::new()),
            Some(entry_ids) => {
                ugoite_core::query::EntryScope::Only(entry_ids.iter().copied().collect())
            }
            None => ugoite_core::query::EntryScope::AllCurrent,
        };
        let heads = crate::query_context::latest_revision_dataframe(revisions, &scope, view)?;
        Ok(heads
            .select_columns(&["entry_id", "revision_id", "entry_version"])?
            .collect()
            .await?)
    }

    async fn read_latest_revision_batches(
        &self,
        table: &iceberg::table::Table,
        entry_ids: Option<&[ugoite_domain::id::EntryId]>,
        snapshot_id: Option<i64>,
        view: RevisionView,
    ) -> Result<Vec<RecordBatch>> {
        let ids = self
            .latest_revision_plan(table, entry_ids, snapshot_id, view)
            .await?;
        let mut revision_ids = Vec::new();
        let mut entry_ids = std::collections::BTreeSet::new();
        for batch in ids {
            let entry_values = batch
                .column_by_name("entry_id")
                .context("latest revision plan is missing entry_id")?;
            let values = batch
                .column_by_name("revision_id")
                .context("latest revision plan is missing revision_id")?;
            for row in 0..batch.num_rows() {
                let entry_id = uuid_at(entry_values, row)?;
                if !entry_ids.insert(entry_id) {
                    return Err(anyhow!(
                        "entry revision invariant failed: multiple revisions share a maximum entry_version"
                    ));
                }
                revision_ids.push(Datum::uuid(uuid_at(values, row)?.as_uuid()));
            }
        }
        if revision_ids.is_empty() {
            return Ok(Vec::new());
        }
        let scan = table
            .scan()
            .with_filter(Reference::new("revision_id").is_in(revision_ids));
        let mut stream = match snapshot_id {
            Some(snapshot_id) => scan.snapshot_id(snapshot_id),
            None => scan,
        }
        .build()?
        .to_arrow()
        .await?;
        let mut batches = Vec::new();
        while let Some(batch) = futures::TryStreamExt::try_next(&mut stream).await? {
            batches.push(batch);
        }
        Ok(batches)
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
        format!(
            "{}/space_{}/{}",
            self.warehouse.trim_end_matches('/'),
            self.space_id.as_uuid().simple(),
            physical_form_name(form_id)
        )
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
            warehouse: self.workspace.warehouse.clone(),
            write: self.workspace.write,
        })
    }

    async fn publication_receipt(&self) -> Result<Option<space_catalog::PublicationReceipt>> {
        self.workspace
            .space_catalog
            .as_ref()
            .context("coordinator is missing its SpaceCatalog")?
            .publication_receipt(&self.publication)
            .await
            .map_err(Into::into)
    }

    pub async fn create_form(&self, form: &FormDefinition) -> Result<()> {
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            if self.publication_receipt().await?.is_some() {
                return Ok(());
            }
            match self.attempt_workspace().await?.create_form(form).await {
                Ok(()) => return Ok(()),
                Err(error) if is_publication_conflict(&error) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(anyhow!("Catalog Head changed during every create attempt"))
    }

    pub async fn evolve_form(&self, changes: &FormChangeSet) -> Result<FormDefinition> {
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            if self.publication_receipt().await?.is_some() {
                return self.workspace.load_form(changes.form_id).await;
            }
            match self.attempt_workspace().await?.evolve_form(changes).await {
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
        if let Some(receipt) = self.publication_receipt().await? {
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
            if let Some(receipt) = self.publication_receipt().await? {
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
                .publication_receipt()
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
        if self.publication_receipt().await?.is_some() {
            return Ok(());
        }
        for _ in 0..MAX_PUBLICATION_ATTEMPTS {
            if self.publication_receipt().await?.is_some() {
                return Ok(());
            }
            let attempt = self.attempt_workspace().await?;
            if attempt.asset_is_deleted(asset_id).await? {
                return Err(anyhow!("Asset '{}' is already unavailable", asset_id));
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
                return Err(anyhow!("Asset '{}' is referenced by an entry", asset_id));
            }
            if referenced_anywhere {
                // The reference is deliberately not named: the caller is not
                // authorized to learn which Entry protects the bytes. The
                // all-current query above still makes deletion fail closed.
                return Err(anyhow!("Asset cannot be deleted while it is in use"));
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
    format!("form_{}", form_id.as_uuid().simple())
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
    Ok(HashMap::from([
        (
            FORM_DEFINITION_PROPERTY.into(),
            serde_json::to_string(form)?,
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
        FieldType::Timestamp | FieldType::TimestampTz => {
            let builder = if matches!(item_kind, FieldType::TimestampTz) {
                TimestampMicrosecondBuilder::new().with_timezone("+00:00")
            } else {
                TimestampMicrosecondBuilder::new()
            };
            build_list!(
                builder,
                |builder: &mut TimestampMicrosecondBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid timestamp item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_timestamp_micros(value)?);
                    Ok(())
                }
            );
        }
        FieldType::TimestampNs | FieldType::TimestampTzNs => {
            let builder = if matches!(item_kind, FieldType::TimestampTzNs) {
                TimestampNanosecondBuilder::new().with_timezone("+00:00")
            } else {
                TimestampNanosecondBuilder::new()
            };
            build_list!(
                builder,
                |builder: &mut TimestampNanosecondBuilder, item: &FieldValue| {
                    let FieldValue::String(value) = item else {
                        return Err(anyhow!(
                            "typed list field '{}' contains an invalid nanosecond timestamp item",
                            field.name
                        ));
                    };
                    builder.append_option(parse_timestamp_nanos(value)?);
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
                    builder.append_value(BASE64.decode(value)?);
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
        FieldType::Timestamp | FieldType::TimestampTz => {
            let values = values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => {
                        parse_timestamp_micros(value).map_err(|error| {
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
            if matches!(field.field_type, FieldType::TimestampTz) {
                Ok(Arc::new(array.with_timezone("+00:00")))
            } else {
                Ok(Arc::new(array))
            }
        }
        FieldType::TimestampNs | FieldType::TimestampTzNs => {
            let values = values
                .iter()
                .map(|value| match value {
                    Some(FieldValue::String(value)) => {
                        parse_timestamp_nanos(value).map_err(|error| {
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
            let array = TimestampNanosecondArray::from(values);
            if matches!(field.field_type, FieldType::TimestampTzNs) {
                Ok(Arc::new(array.with_timezone("+00:00")))
            } else {
                Ok(Arc::new(array))
            }
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
                    Some(FieldValue::String(value)) => builder.append_value(BASE64.decode(value)?),
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

fn parse_date(value: &str) -> Result<Option<i32>> {
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid Unix epoch date");
    Ok(Some(date.signed_duration_since(epoch).num_days() as i32))
}

fn parse_time_micros(value: &str) -> Result<Option<i64>> {
    let time = NaiveTime::parse_from_str(value, "%H:%M:%S%.f")
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M:%S"))
        // HTML time inputs omit seconds when the value is minute-precise.
        .or_else(|_| NaiveTime::parse_from_str(value, "%H:%M"))?;
    Ok(Some(
        i64::from(time.num_seconds_from_midnight()) * 1_000_000
            + i64::from(time.nanosecond() / 1_000),
    ))
}

fn parse_timestamp_micros(value: &str) -> Result<Option<i64>> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Ok(Some(timestamp.timestamp_micros()));
    }
    let timestamp = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"))?;
    Ok(Some(timestamp.and_utc().timestamp_micros()))
}

fn parse_timestamp_nanos(value: &str) -> Result<Option<i64>> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(value) {
        return Ok(Some(timestamp.timestamp_nanos_opt().context(
            "timestamp is outside the representable nanosecond range",
        )?));
    }
    let timestamp = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f"))?;
    Ok(Some(timestamp.and_utc().timestamp_nanos_opt().context(
        "timestamp is outside the representable nanosecond range",
    )?))
}

fn uuid_value_at(array: &dyn Array, row: usize) -> Result<Uuid> {
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
                integrity: integrity_at(integrity, row)?,
                deleted: required_bool(deleted, row, "ugoite_entry_deleted")?,
                deleted_at_micros: optional_i64(&deleted_at, row),
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

fn struct_i64_at(array: &StructArray, name: &str, row: usize) -> i64 {
    array
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<Int64Array>())
        .filter(|column| !column.is_null(row))
        .map(|column| column.value(row))
        .unwrap_or_default()
}

fn asset_reference_at(array: &StructArray, row: usize) -> Result<FieldValue> {
    Ok(FieldValue::AssetReference(AssetReference {
        asset_id: struct_string_at(array, "asset_id", row),
        name: struct_string_at(array, "name", row),
        media_type: struct_string_at(array, "media_type", row),
        size_bytes: u64::try_from(struct_i64_at(array, "size_bytes", row))?,
        sha256: struct_string_at(array, "sha256", row),
    }))
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
        FieldType::Timestamp | FieldType::TimestampTz => FieldValue::String(timestamp_from_micros(
            column
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .ok_or_else(invalid)?
                .value(row),
        )?),
        FieldType::TimestampNs | FieldType::TimestampTzNs => {
            FieldValue::String(timestamp_from_nanos(
                column
                    .as_any()
                    .downcast_ref::<TimestampNanosecondArray>()
                    .ok_or_else(invalid)?
                    .value(row),
            )?)
        }
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
        FieldType::Binary => FieldValue::String(
            BASE64.encode(
                column
                    .as_any()
                    .downcast_ref::<LargeBinaryArray>()
                    .ok_or_else(invalid)?
                    .value(row),
            ),
        ),
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

fn timestamp_from_nanos(nanos: i64) -> Result<String> {
    let seconds = nanos.div_euclid(1_000_000_000);
    let nanos = u32::try_from(nanos.rem_euclid(1_000_000_000))?;
    DateTime::from_timestamp(seconds, nanos)
        .context("timestamp is outside the supported range")
        .map(|timestamp: DateTime<chrono::Utc>| {
            timestamp.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
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
            if uuid_value_at(entry_ids.as_ref(), row)? != revision.entry_id.as_uuid()
                || uuid_value_at(revision_ids.as_ref(), row)? != revision.revision_id.as_uuid()
                || parent != revision.parent_revision_id
                || entry_version != revision.entry_version
                || form_version != revision.form_version.get()
                || operations.is_null(row)
                || operations.value(row) != operation
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
