//! Rebuildable, non-authoritative Iceberg relations.
//!
//! The Relation Head in `ugoite-storage` is the only durable visibility
//! coordinate in this module.  Iceberg metadata and data files are immutable
//! build products below a build prefix; a failed build therefore
//! cannot replace the currently visible result or the authoritative Catalog.

use anyhow::{anyhow, bail, Context, Result};
use arrow_array::builder::{Int64Builder, StringBuilder};
use arrow_array::{Array, RecordBatch, StringArray, TimestampMicrosecondArray};
use chrono::Utc;
use datafusion::prelude::SessionContext;
use flate2::read::{DeflateDecoder, ZlibDecoder};
use futures::TryStreamExt;
use iceberg::io::FileIO;
use iceberg::spec::TableMetadataBuilder;
use iceberg::spec::{NestedField, PrimitiveType, Schema, SortOrder, Type, UnboundPartitionSpec};
use iceberg::table::Table;
use iceberg::transaction::{ApplyTransactionAction, Transaction};
use iceberg::writer::base_writer::data_file_writer::DataFileWriterBuilder;
use iceberg::writer::file_writer::location_generator::{
    DefaultFileNameGenerator, DefaultLocationGenerator,
};
use iceberg::writer::file_writer::rolling_writer::RollingFileWriterBuilder;
use iceberg::writer::file_writer::ParquetWriterBuilder;
use iceberg::writer::{IcebergWriter, IcebergWriterBuilder};
use iceberg::{
    Catalog, Error as IcebergError, ErrorKind as IcebergErrorKind, MetadataLocation, Namespace,
    NamespaceIdent, Runtime, TableCreation, TableIdent,
};
use iceberg_datafusion::IcebergStaticTableProvider;
use opendal::options::{ReadOptions, WriteOptions};
use opendal::{ErrorKind, Operator};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::io::{Cursor, Read, Seek};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{Mutex, Notify, Semaphore};
use ugoite_domain::derived_relation::DerivedRelationId;
use ugoite_domain::derived_relation::{
    canonical_json, sha256_digest, DerivedErrorCode, DerivedExposure, DerivedRelationDefinition,
    DerivedValueType, RelationField, TypedSchema,
};
use ugoite_domain::entry::AssetReference;
use ugoite_domain::form::FieldType;
use ugoite_domain::id::validate_asset_id;
use ugoite_storage::{DerivedRelationHead, DerivedRelationHeadStore, SpaceCatalogStore};
use uuid::Uuid;
use zip::ZipArchive;

pub const ASSET_TEXT_PRODUCER_ID: &str = "ugoite.asset_text";
pub const ASSET_TEXT_PARSER_VERSION: &str = "3";
// Bump this whenever the persisted AssetText contract changes in a way that
// makes an existing build unsafe to reuse. AssetReference path validation is
// part of the contract, so epoch 3 builds must be rebuilt before registration.
pub const ASSET_TEXT_COMPATIBILITY_EPOCH: u64 = 4;
const MAX_ASSET_BYTES: u64 = crate::asset::MAX_ASSET_BYTES as u64;
const MAX_ZIP_ENTRIES: usize = 10_000;
const MAX_ZIP_EXPANDED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 10_000;
const MAX_PDF_OBJECTS: usize = 100_000;
const MAX_PDF_TEXT_OPERATORS: usize = 1_000_000;
const MAX_XML_DEPTH: usize = 256;
const MAX_EXTRACTED_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_CHUNK_CHARS: usize = 16 * 1024;
const MAX_TOTAL_ASSET_TEXT_ROWS: usize = 1_000_000;
const MAX_TOTAL_ASSET_TEXT_BYTES: usize = 512 * 1024 * 1024;
const ASSET_TEXT_APPEND_BATCH_ROWS: usize = 8_192;
const MAX_SOURCE_ASSETS: usize = 1_000_000;
const MAX_SOURCE_FORMS: usize = 100_000;
const MAX_SOURCE_FORM_DEFINITION_BYTES: usize = 256 * 1024 * 1024;
const MAX_SOURCE_METADATA_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug)]
struct AssetParserInputLimit;

impl std::fmt::Display for AssetParserInputLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("asset parser input exceeds configured limit")
    }
}

impl std::error::Error for AssetParserInputLimit {}
const MAX_ASSET_REFERENCES_PER_ENTRY: usize = ugoite_domain::entry::MAX_ASSET_REFERENCES_PER_ENTRY;
const MAX_ASSET_TEXT_MATCHES: usize = 1_000_000;
pub const MAX_ASSET_TEXT_QUERY_BYTES: usize = 8 * 1024;
const MAX_ASSET_TEXT_MATCH_BYTES: usize = 64 * 1024 * 1024;
const READER_CHUNK_BYTES: usize = 256 * 1024;
const MINIMUM_GC_AGE: Duration = Duration::from_secs(60 * 60);
const GC_RETRY_DELAY: Duration = Duration::from_secs(60);
const GC_OPERATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const ASSET_TEXT_REBUILD_OPERATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_SOURCE_CHANGE_RETRIES: usize = 3;
const MAX_BACKGROUND_GC_SCHEDULERS: usize = 1024;
const MAX_CONSECUTIVE_GC_FAILURES: usize = 10;
const ASSET_TEXT_REFRESH_REQUEST_FILE: &str = "refresh-request.json";
const MAX_ASSET_TEXT_REFRESH_REQUESTS: usize = 16_384;
const MAX_ASSET_TEXT_REFRESH_SCAN_ENTRIES: usize = 64 * 1024;
const ASSET_TEXT_REFRESH_ADMISSION_LOCK: &str = "admission.lock";
const ASSET_TEXT_REFRESH_ADMISSION_LOCK_TTL: Duration = Duration::from_secs(5 * 60);
const ASSET_TEXT_REFRESH_ADMISSION_HEARTBEAT: Duration = Duration::from_secs(30);

static ASSET_TEXT_REFRESH_LOCAL_ADMISSION: OnceLock<Mutex<()>> = OnceLock::new();

struct AbortOnDrop(Option<tokio::task::JoinHandle<()>>);

impl AbortOnDrop {
    fn new(handle: tokio::task::JoinHandle<()>) -> Self {
        Self(Some(handle))
    }

    fn abort(mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// A staged build remains discoverable if the rebuild future is cancelled
/// after `staging.json` is installed. The async best-effort marker write is
/// only a fast path; the staging marker remains the durable fallback for a
/// process that exits before this task can run.
struct StagedBuildCleanup {
    store: Option<DerivedRelationHeadStore>,
    build_id: String,
}

struct AssetTextAdmissionCleanup {
    operator: Option<Operator>,
    ws_path: String,
    owner: String,
}

impl AssetTextAdmissionCleanup {
    fn new(operator: Operator, ws_path: String, owner: String) -> Self {
        Self {
            operator: Some(operator),
            ws_path,
            owner,
        }
    }

    fn disarm(&mut self) {
        self.operator = None;
    }
}

impl Drop for AssetTextAdmissionCleanup {
    fn drop(&mut self) {
        let Some(operator) = self.operator.take() else {
            return;
        };
        let ws_path = self.ws_path.clone();
        let owner = self.owner.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let _ = release_asset_text_refresh_admission_lock(&operator, &ws_path, &owner).await;
        });
    }
}

impl StagedBuildCleanup {
    fn new(store: DerivedRelationHeadStore, build_id: String) -> Self {
        Self {
            store: Some(store),
            build_id,
        }
    }

    fn disarm(&mut self) {
        self.store = None;
    }
}

impl Drop for StagedBuildCleanup {
    fn drop(&mut self) {
        let Some(store) = self.store.take() else {
            return;
        };
        let build_id = self.build_id.clone();
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        handle.spawn(async move {
            let _ = ensure_cleanup_marker(&store, &build_id).await;
        });
    }
}

struct AssetTextGcScheduler {
    notify: Notify,
    deadline: StdMutex<Option<Instant>>,
    started: AtomicBool,
}

static ASSET_TEXT_GC_SCHEDULERS: OnceLock<StdMutex<BTreeMap<String, Arc<AssetTextGcScheduler>>>> =
    OnceLock::new();

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct AssetTextRow {
    pub asset_id: String,
    pub source_sha256: String,
    pub source_size_bytes: i64,
    pub parser_id: String,
    pub parser_version: String,
    pub producer_fingerprint: String,
    pub status: String,
    pub chunk_index: i64,
    pub source_locator: Option<String>,
    pub text: Option<String>,
    pub text_length: i64,
    pub parsed_at: String,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct AssetTextManifest {
    format_version: u32,
    relation_id: String,
    build_id: String,
    producer_fingerprint: String,
    input_digest: String,
    row_count: usize,
    source_coordinate: Value,
    row_digest: String,
    #[serde(default)]
    assets_referenced: usize,
    #[serde(default)]
    assets_ready: usize,
    #[serde(default)]
    assets_empty: usize,
    #[serde(default)]
    assets_failed: usize,
    #[serde(default)]
    assets_unsupported: usize,
}

#[derive(Default)]
struct AssetTextStatusCounts {
    assets_referenced: usize,
    assets_ready: usize,
    assets_empty: usize,
    assets_failed: usize,
    assets_unsupported: usize,
}

#[derive(Default)]
struct BoundedAssetTextRows {
    rows: Vec<AssetTextRow>,
    total_bytes: usize,
    error: Option<anyhow::Error>,
}

impl BoundedAssetTextRows {
    fn failed(&self) -> bool {
        self.error.is_some()
    }

    fn push(&mut self, row: AssetTextRow) {
        if self.error.is_some() {
            return;
        }
        let Some(row_bytes) = [
            row.asset_id.len(),
            row.source_sha256.len(),
            row.parser_id.len(),
            row.parser_version.len(),
            row.producer_fingerprint.len(),
            row.status.len(),
            row.source_locator.as_ref().map_or(0, String::len),
            row.text.as_ref().map_or(0, String::len),
            row.parsed_at.len(),
            row.error_code.as_ref().map_or(0, String::len),
            std::mem::size_of::<AssetTextRow>(),
        ]
        .into_iter()
        .try_fold(0usize, usize::checked_add) else {
            self.error = Some(anyhow::anyhow!("AssetText rebuild output size overflow"));
            return;
        };
        let Some(next_total_bytes) = self.total_bytes.checked_add(row_bytes) else {
            self.error = Some(anyhow::anyhow!(
                "AssetText rebuild output exceeds its total byte limit"
            ));
            return;
        };
        if self.rows.len() >= MAX_TOTAL_ASSET_TEXT_ROWS
            || next_total_bytes > MAX_TOTAL_ASSET_TEXT_BYTES
        {
            self.error = Some(anyhow::anyhow!(
                "AssetText rebuild output exceeds its aggregate limit"
            ));
            return;
        }
        self.total_bytes = next_total_bytes;
        self.rows.push(row);
    }

    fn finish(self) -> Result<Vec<AssetTextRow>> {
        if let Some(error) = self.error {
            return Err(error);
        }
        Ok(self.rows)
    }
}

#[derive(Clone, Debug, Serialize)]
struct SourceReference {
    asset_id: String,
    name: String,
    media_type: String,
    source_sha256: String,
    source_size_bytes: u64,
    integrity_error: Option<String>,
}

#[derive(Clone, Debug)]
struct ExtractedChunk {
    locator: Value,
    text: String,
}

#[derive(Clone, Debug)]
struct ParserIdentity {
    id: &'static str,
    version: &'static str,
}

#[derive(Clone, Debug)]
enum Dispatch {
    PlainText(ParserIdentity),
    Pdf(ParserIdentity),
    Docx(ParserIdentity),
    Xlsx(ParserIdentity),
    Pptx(ParserIdentity),
    Unsupported(ParserIdentity),
}

impl Dispatch {
    fn parser(&self) -> &ParserIdentity {
        match self {
            Self::PlainText(parser)
            | Self::Pdf(parser)
            | Self::Docx(parser)
            | Self::Xlsx(parser)
            | Self::Pptx(parser)
            | Self::Unsupported(parser) => parser,
        }
    }
}

pub fn asset_text_definition() -> DerivedRelationDefinition {
    let fields = vec![
        (1, "asset_id", DerivedValueType::String, false),
        (2, "source_sha256", DerivedValueType::String, false),
        (3, "source_size_bytes", DerivedValueType::Long, false),
        (4, "parser_id", DerivedValueType::String, false),
        (5, "parser_version", DerivedValueType::String, false),
        (6, "producer_fingerprint", DerivedValueType::String, false),
        (7, "status", DerivedValueType::String, false),
        (8, "chunk_index", DerivedValueType::Long, false),
        (9, "source_locator", DerivedValueType::String, true),
        (10, "text", DerivedValueType::String, true),
        (11, "text_length", DerivedValueType::Long, false),
        (12, "parsed_at", DerivedValueType::Timestamp, false),
        (13, "error_code", DerivedValueType::String, true),
    ]
    .into_iter()
    .map(|(field_id, name, value_type, nullable)| RelationField {
        field_id,
        name: name.to_string(),
        value_type,
        nullable,
    })
    .collect();
    DerivedRelationDefinition {
        relation_id: DerivedRelationId::ASSET_TEXT,
        name: ASSET_TEXT_PRODUCER_ID.to_string(),
        definition_version: 2,
        schema: TypedSchema { fields },
        logical_key: vec!["asset_id".into(), "chunk_index".into()],
        exposure: DerivedExposure::Internal,
        producer_id: ASSET_TEXT_PRODUCER_ID.to_string(),
    }
}

pub fn asset_text_producer_fingerprint() -> String {
    // This is deliberately a semantic contract, not a crate version.  Any
    // parser, normalization, dispatch, or chunking change must update it.
    sha256_digest(
        b"ugoite.asset_text/protocol=3;dispatch=text/plain,text/markdown,pdf,docx,xlsx,pptx;pdf=literal+bounded-flate;normalization=line-endings+control-chars;chunk=semantic-boundary+16384-unicode-scalars;limits=64MiB-input+10000-zip-entries+128MiB-zip+10000-pdf-pages+100000-pdf-objects+1000000-pdf-text-operators+16MiB-text+256-xml-depth+1000000-total-rows+512MiB-total-text+16MiB-pdf-decoded-stream;blocking=bounded-4;schema=2",
    )
}

pub fn asset_text_definition_fingerprint() -> String {
    asset_text_definition().fingerprint()
}

fn parser_identity(id: &'static str) -> ParserIdentity {
    ParserIdentity {
        id,
        version: ASSET_TEXT_PARSER_VERSION,
    }
}

fn asset_text_schema() -> Schema {
    fn field(id: i32, name: &str, ty: Type, required: bool) -> Arc<NestedField> {
        Arc::new(NestedField::new(id, name, ty, required))
    }
    Schema::builder()
        .with_fields(vec![
            field(1, "asset_id", Type::Primitive(PrimitiveType::String), true),
            field(
                2,
                "source_sha256",
                Type::Primitive(PrimitiveType::String),
                true,
            ),
            field(
                3,
                "source_size_bytes",
                Type::Primitive(PrimitiveType::Long),
                true,
            ),
            field(4, "parser_id", Type::Primitive(PrimitiveType::String), true),
            field(
                5,
                "parser_version",
                Type::Primitive(PrimitiveType::String),
                true,
            ),
            field(
                6,
                "producer_fingerprint",
                Type::Primitive(PrimitiveType::String),
                true,
            ),
            field(7, "status", Type::Primitive(PrimitiveType::String), true),
            field(8, "chunk_index", Type::Primitive(PrimitiveType::Long), true),
            field(
                9,
                "source_locator",
                Type::Primitive(PrimitiveType::String),
                false,
            ),
            field(10, "text", Type::Primitive(PrimitiveType::String), false),
            field(
                11,
                "text_length",
                Type::Primitive(PrimitiveType::Long),
                true,
            ),
            field(
                12,
                "parsed_at",
                Type::Primitive(PrimitiveType::Timestamptz),
                true,
            ),
            field(
                13,
                "error_code",
                Type::Primitive(PrimitiveType::String),
                false,
            ),
        ])
        .build()
        .expect("AssetText schema is static and valid")
}

/// A relation-local implementation of the official Iceberg Catalog API. It
/// deliberately has no Space Catalog state. The in-process table pointer is
/// only a transaction helper; the durable Relation Head is published by the
/// caller after all immutable objects are complete.
#[derive(Debug, Clone)]
struct DerivedRelationCatalog {
    table: Arc<Mutex<Option<Table>>>,
    file_io: FileIO,
    runtime: Runtime,
    namespace: NamespaceIdent,
}

impl DerivedRelationCatalog {
    fn new(file_io: FileIO, runtime: Runtime, namespace: NamespaceIdent) -> Self {
        Self {
            table: Arc::new(Mutex::new(None)),
            file_io,
            runtime,
            namespace,
        }
    }

    async fn current(&self) -> iceberg::Result<Table> {
        self.table.lock().await.clone().ok_or_else(|| {
            IcebergError::new(
                IcebergErrorKind::TableNotFound,
                "derived table is not created",
            )
        })
    }
}

#[async_trait::async_trait]
impl Catalog for DerivedRelationCatalog {
    async fn list_namespaces(
        &self,
        parent: Option<&NamespaceIdent>,
    ) -> iceberg::Result<Vec<NamespaceIdent>> {
        Ok(parent
            .is_none()
            .then_some(self.namespace.clone())
            .into_iter()
            .collect())
    }

    async fn create_namespace(
        &self,
        namespace: &NamespaceIdent,
        _properties: HashMap<String, String>,
    ) -> iceberg::Result<Namespace> {
        self.get_namespace(namespace).await
    }

    async fn get_namespace(&self, namespace: &NamespaceIdent) -> iceberg::Result<Namespace> {
        if namespace == &self.namespace {
            Ok(Namespace::new(namespace.clone()))
        } else {
            Err(IcebergError::new(
                IcebergErrorKind::DataInvalid,
                "unknown derived namespace",
            ))
        }
    }

    async fn namespace_exists(&self, namespace: &NamespaceIdent) -> iceberg::Result<bool> {
        Ok(namespace == &self.namespace)
    }

    async fn update_namespace(
        &self,
        _namespace: &NamespaceIdent,
        _properties: HashMap<String, String>,
    ) -> iceberg::Result<()> {
        Err(IcebergError::new(
            IcebergErrorKind::FeatureUnsupported,
            "derived namespace properties are immutable",
        ))
    }

    async fn drop_namespace(&self, _namespace: &NamespaceIdent) -> iceberg::Result<()> {
        Err(IcebergError::new(
            IcebergErrorKind::FeatureUnsupported,
            "derived namespaces cannot be dropped",
        ))
    }

    async fn list_tables(&self, namespace: &NamespaceIdent) -> iceberg::Result<Vec<TableIdent>> {
        if namespace != &self.namespace {
            return Ok(Vec::new());
        }
        Ok(self
            .table
            .lock()
            .await
            .as_ref()
            .map(|table| table.identifier().clone())
            .into_iter()
            .collect())
    }

    async fn create_table(
        &self,
        namespace: &NamespaceIdent,
        creation: TableCreation,
    ) -> iceberg::Result<Table> {
        if namespace != &self.namespace {
            return Err(IcebergError::new(
                IcebergErrorKind::DataInvalid,
                "derived table namespace mismatch",
            ));
        }
        let requested_schema = creation.schema.clone();
        let table_name = creation.name.clone();
        let metadata = TableMetadataBuilder::from_table_creation(creation)?
            .build()?
            .metadata;
        let metadata = crate::space_catalog::preserve_schema_field_ids(metadata, requested_schema)?;
        let metadata_location = MetadataLocation::try_new_with_metadata(&metadata)?;
        metadata.write_to(&self.file_io, &metadata_location).await?;
        let table = Table::builder()
            .identifier(TableIdent::new(namespace.clone(), table_name))
            .metadata(metadata)
            .metadata_location(metadata_location.to_string())
            .file_io(self.file_io.clone())
            .runtime(self.runtime.clone())
            .build()?;
        let mut guard = self.table.lock().await;
        if guard.is_some() {
            return Err(IcebergError::new(
                IcebergErrorKind::TableAlreadyExists,
                "derived table already exists",
            ));
        }
        *guard = Some(table.clone());
        Ok(guard.clone().expect("derived table was just stored"))
    }

    async fn load_table(&self, table: &TableIdent) -> iceberg::Result<Table> {
        let current = self.current().await?;
        if current.identifier() == table {
            Ok(current)
        } else {
            Err(IcebergError::new(
                IcebergErrorKind::TableNotFound,
                "unknown derived table",
            ))
        }
    }

    async fn drop_table(&self, _table: &TableIdent) -> iceberg::Result<()> {
        Err(IcebergError::new(
            IcebergErrorKind::FeatureUnsupported,
            "derived tables are replaced by current-build swap",
        ))
    }

    async fn purge_table(&self, _table: &TableIdent) -> iceberg::Result<()> {
        Err(IcebergError::new(
            IcebergErrorKind::FeatureUnsupported,
            "derived tables are garbage collected by prefix",
        ))
    }

    async fn table_exists(&self, table: &TableIdent) -> iceberg::Result<bool> {
        Ok(self
            .table
            .lock()
            .await
            .as_ref()
            .is_some_and(|current| current.identifier() == table))
    }

    async fn rename_table(&self, _src: &TableIdent, _dest: &TableIdent) -> iceberg::Result<()> {
        Err(IcebergError::new(
            IcebergErrorKind::FeatureUnsupported,
            "derived table identifiers are stable",
        ))
    }

    async fn register_table(
        &self,
        table: &TableIdent,
        metadata_location: String,
    ) -> iceberg::Result<Table> {
        let metadata =
            iceberg::spec::TableMetadata::read_from(&self.file_io, &metadata_location).await?;
        let loaded = Table::builder()
            .identifier(table.clone())
            .metadata(metadata)
            .metadata_location(metadata_location)
            .file_io(self.file_io.clone())
            .runtime(self.runtime.clone())
            .build()?;
        *self.table.lock().await = Some(loaded.clone());
        Ok(loaded)
    }

    async fn update_table(&self, commit: iceberg::TableCommit) -> iceberg::Result<Table> {
        let current = self.current().await?;
        let staged = commit.apply(current)?;
        let location = MetadataLocation::from_str(staged.metadata_location_result()?)?;
        staged
            .metadata()
            .write_to(staged.file_io(), &location)
            .await?;
        *self.table.lock().await = Some(staged.clone());
        Ok(staged)
    }
}

pub async fn rebuild_asset_text(op: &Operator, ws_path: &str) -> Result<DerivedRelationHead> {
    rebuild_asset_text_with_timeout(op, ws_path, false).await
}

/// Shared backends use the exact-read/if-match path and deliberately do not
/// take the process-local rebuild mutex. A losing build remains an immutable
/// garbage candidate and is never published.
pub async fn rebuild_asset_text_shared(
    op: &Operator,
    ws_path: &str,
) -> Result<DerivedRelationHead> {
    rebuild_asset_text_with_timeout(op, ws_path, true).await
}

async fn rebuild_asset_text_with_timeout(
    op: &Operator,
    ws_path: &str,
    shared: bool,
) -> Result<DerivedRelationHead> {
    match tokio::time::timeout(ASSET_TEXT_REBUILD_OPERATION_TIMEOUT, async {
        let mut last_source_change = None;
        for _ in 0..=MAX_SOURCE_CHANGE_RETRIES {
            match rebuild_asset_text_with_mode(op, ws_path, shared).await {
                Ok(head) => return Ok(head),
                Err(error) if is_asset_text_source_changed(&error) => {
                    last_source_change = Some(error);
                    tokio::task::yield_now().await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(last_source_change.expect("source-change retry must have an error"))
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            // A timeout may drop an in-flight build after staging. Its
            // durable staging marker remains recoverable by relation GC.
            schedule_asset_text_gc(op, ws_path);
            Err(anyhow!("AssetText rebuild operation timed out"))
        }
    }
}

pub fn is_asset_text_source_changed(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("authoritative source changed"))
}

fn asset_text_refresh_request_prefix(ws_path: &str) -> String {
    format!(
        "{ws_path}/_ugoite/derived/relations/{}/refresh-requests/",
        DerivedRelationId::ASSET_TEXT,
    )
}

fn asset_text_refresh_request_path(ws_path: &str, token: &str) -> String {
    format!("{}{token}.json", asset_text_refresh_request_prefix(ws_path))
}

fn legacy_asset_text_refresh_request_path(ws_path: &str) -> String {
    format!(
        "{ws_path}/_ugoite/derived/relations/{}/{}",
        DerivedRelationId::ASSET_TEXT,
        ASSET_TEXT_REFRESH_REQUEST_FILE
    )
}

fn refresh_request_token(path: &str) -> Option<&str> {
    let token = path.rsplit('/').next()?.strip_suffix(".json")?;
    let uuid = Uuid::parse_str(token).ok()?;
    (uuid.get_version_num() == 7 && (uuid.as_bytes()[8] & 0xc0) == 0x80).then_some(token)
}

fn refresh_request_admission_lock_path(ws_path: &str) -> String {
    format!(
        "{}{ASSET_TEXT_REFRESH_ADMISSION_LOCK}",
        asset_text_refresh_request_prefix(ws_path),
    )
}

fn refresh_request_admission_lock_bytes(
    owner: &str,
    released: bool,
    acquired_at: i64,
    heartbeat_at: i64,
) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "owner": owner,
        "acquired_at": acquired_at,
        "heartbeat_at": heartbeat_at,
        "released": released,
    }))
    .expect("AssetText refresh admission lock is serializable")
}

fn refresh_request_admission_lock_reclaimable(
    bytes: &[u8],
    last_modified: Option<SystemTime>,
) -> bool {
    let value = serde_json::from_slice::<Value>(bytes).ok();
    if value
        .as_ref()
        .and_then(|value| value.get("released"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return true;
    }
    // JSON timestamps are diagnostic only. Lease expiry must use the
    // backend's modification timestamp so clock skew between shared writers
    // cannot reclaim a live admission lock.
    last_modified
        .and_then(|timestamp| SystemTime::now().duration_since(timestamp).ok())
        .is_some_and(|age| age >= ASSET_TEXT_REFRESH_ADMISSION_LOCK_TTL)
}

async fn acquire_asset_text_refresh_admission_lock(op: &Operator, ws_path: &str) -> Result<String> {
    let capabilities = op.info().capability();
    let has_shared_contract = capabilities.stat
        && capabilities.read_with_if_match
        && capabilities.write_with_if_not_exists
        && capabilities.write_with_if_match;
    if !has_shared_contract {
        bail!("AssetText refresh marker admission requires conditional object writes");
    }
    let path = refresh_request_admission_lock_path(ws_path);
    let owner = Uuid::now_v7().to_string();
    for _ in 0..3 {
        let now = Utc::now().timestamp();
        match op
            .write_options(
                &path,
                refresh_request_admission_lock_bytes(&owner, false, now, now),
                WriteOptions {
                    if_not_exists: true,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => {
                let metadata = match op.stat(&path).await {
                    Ok(metadata) => metadata,
                    Err(error) => {
                        let _ = op.delete(&path).await;
                        return Err(error.into());
                    }
                };
                if metadata.etag().filter(|etag| !etag.is_empty()).is_none()
                    || metadata.last_modified().is_none()
                {
                    let _ = op.delete(&path).await;
                    bail!("AssetText refresh marker admission lock lacks server metadata");
                }
                return Ok(owner);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConditionNotMatch | ErrorKind::AlreadyExists
                ) => {}
            Err(error) => return Err(error.into()),
        }

        let metadata = match op.stat(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let Some(etag) = metadata.etag().filter(|etag| !etag.is_empty()) else {
            bail!("AssetText refresh marker admission lock has no ETag")
        };
        if metadata.last_modified().is_none() {
            bail!("AssetText refresh marker admission lock has no server timestamp")
        }
        let bytes = match op
            .read_options(
                &path,
                ReadOptions {
                    if_match: Some(etag.to_string()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(bytes) => bytes.to_vec(),
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => continue,
            Err(error) => return Err(error.into()),
        };
        if !refresh_request_admission_lock_reclaimable(
            &bytes,
            metadata.last_modified().map(Into::into),
        ) {
            bail!("AssetText refresh marker admission is busy")
        }
        let now = Utc::now().timestamp();
        match op
            .write_options(
                &path,
                refresh_request_admission_lock_bytes(&owner, false, now, now),
                WriteOptions {
                    if_match: Some(etag.to_string()),
                    ..Default::default()
                },
            )
            .await
        {
            Ok(_) => {
                let metadata = match op.stat(&path).await {
                    Ok(metadata) => metadata,
                    Err(error) => return Err(error.into()),
                };
                if metadata.etag().filter(|etag| !etag.is_empty()).is_none()
                    || metadata.last_modified().is_none()
                {
                    bail!("AssetText refresh marker admission lock lacks server metadata");
                }
                return Ok(owner);
            }
            Err(error) if error.kind() == ErrorKind::ConditionNotMatch => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("AssetText refresh marker admission changed while acquiring its lock")
}

async fn release_asset_text_refresh_admission_lock(
    op: &Operator,
    ws_path: &str,
    owner: &str,
) -> Result<()> {
    let path = refresh_request_admission_lock_path(ws_path);
    let metadata = match op.stat(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let Some(etag) = metadata.etag().filter(|etag| !etag.is_empty()) else {
        bail!("AssetText refresh marker admission lock has no ETag")
    };
    let bytes = match op
        .read_options(
            &path,
            ReadOptions {
                if_match: Some(etag.to_string()),
                ..Default::default()
            },
        )
        .await
    {
        Ok(bytes) => bytes.to_vec(),
        Err(error) if error.kind() == ErrorKind::ConditionNotMatch => return Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let current_owner = serde_json::from_slice::<Value>(&bytes)?
        .get("owner")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if current_owner.as_deref() != Some(owner) {
        return Ok(());
    }
    match op
        .write_options(
            &path,
            refresh_request_admission_lock_bytes(
                owner,
                true,
                Utc::now().timestamp(),
                Utc::now().timestamp(),
            ),
            WriteOptions {
                if_match: Some(etag.to_string()),
                ..Default::default()
            },
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::ConditionNotMatch => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Renews the durable admission lease while a shared refresh-marker drain is
/// scanning/deleting a large prefix. A false result means that another writer
/// has already replaced or removed this owner's lease; callers must then fail
/// closed rather than continue claiming atomic capacity or marker ownership.
async fn renew_asset_text_refresh_admission_lock(
    op: &Operator,
    ws_path: &str,
    owner: &str,
) -> Result<bool> {
    let path = refresh_request_admission_lock_path(ws_path);
    let metadata = match op.stat(&path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let Some(etag) = metadata.etag().filter(|etag| !etag.is_empty()) else {
        bail!("AssetText refresh marker admission lock has no ETag")
    };
    let bytes = match op
        .read_options(
            &path,
            ReadOptions {
                if_match: Some(etag.to_string()),
                ..Default::default()
            },
        )
        .await
    {
        Ok(bytes) => bytes.to_vec(),
        Err(error) if error.kind() == ErrorKind::ConditionNotMatch => return Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let value = serde_json::from_slice::<Value>(&bytes)?;
    if value.get("owner").and_then(Value::as_str) != Some(owner)
        || value.get("released").and_then(Value::as_bool) == Some(true)
    {
        return Ok(false);
    }
    let acquired_at = value
        .get("acquired_at")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| Utc::now().timestamp());
    let now = Utc::now().timestamp();
    match op
        .write_options(
            &path,
            refresh_request_admission_lock_bytes(owner, false, acquired_at, now),
            WriteOptions {
                if_match: Some(etag.to_string()),
                ..Default::default()
            },
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::ConditionNotMatch => Ok(false),
        Err(error) => Err(error.into()),
    }
}

async fn with_asset_text_refresh_admission_lock<T, F, Fut>(
    op: &Operator,
    ws_path: &str,
    operation: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let capabilities = op.info().capability();
    let has_shared_contract = capabilities.stat
        && capabilities.read_with_if_match
        && capabilities.write_with_if_not_exists
        && capabilities.write_with_if_match;
    if !has_shared_contract {
        // The in-memory backend intentionally has no conditional-object
        // contract. It is a single-process test/local backend, so retain the
        // same admission invariant with one process-local mutex. Shared
        // remote writers are rejected rather than pretending a read-then-write
        // check is atomic.
        if op.info().scheme() != "memory" {
            bail!("AssetText refresh marker admission requires conditional object writes");
        }
        let lock = ASSET_TEXT_REFRESH_LOCAL_ADMISSION.get_or_init(Mutex::default);
        let guard = lock.lock().await;
        let result = operation().await;
        drop(guard);
        return result;
    }
    // Capability flags are only an advertised shape. Reuse the storage
    // boundary's behavioral probe before admitting a shared refresh drain, so
    // a backend that merely reports conditional-write support cannot create
    // two owners of the same marker snapshot.
    SpaceCatalogStore::new(op.clone(), ws_path.to_string())?
        .verify_shared_writes()
        .await?;
    let owner = acquire_asset_text_refresh_admission_lock(op, ws_path).await?;
    let mut admission_cleanup =
        AssetTextAdmissionCleanup::new(op.clone(), ws_path.to_owned(), owner.clone());
    let lease_lost = Arc::new(AtomicBool::new(false));
    let heartbeat = {
        let op = op.clone();
        let ws_path = ws_path.to_owned();
        let owner = owner.clone();
        let lease_lost = lease_lost.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(ASSET_TEXT_REFRESH_ADMISSION_HEARTBEAT).await;
                match renew_asset_text_refresh_admission_lock(&op, &ws_path, &owner).await {
                    Ok(true) => {}
                    Ok(false) | Err(_) => {
                        lease_lost.store(true, Ordering::Release);
                        break;
                    }
                }
            }
        })
    };
    // Do not cancel an in-flight marker mutation when the heartbeat reports a
    // loss. The operation may already have committed at the storage boundary;
    // letting it settle gives the caller a reconciliable result instead of an
    // ambiguous cancellation. The latched post-check fails closed.
    let result = operation().await;
    heartbeat.abort();
    let _ = heartbeat.await;
    let result = if lease_lost.load(Ordering::Acquire) {
        Err(anyhow!(
            "AssetText refresh marker admission lease was lost during operation"
        ))
    } else {
        result
    };
    let release = release_asset_text_refresh_admission_lock(op, ws_path, &owner).await;
    if release.is_ok() {
        admission_cleanup.disarm();
    }
    match (result, release) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(release_error)) => Err(error.context(format!(
            "release AssetText refresh marker admission lock: {release_error:#}"
        ))),
    }
}

fn refresh_request_is_at_or_before(path: &str, cutoff: &str) -> bool {
    // The fixed-name marker predates immutable UUID-v7 markers. It is always
    // part of the snapshot because there is no creation coordinate to compare.
    path.ends_with(&format!("/{ASSET_TEXT_REFRESH_REQUEST_FILE}"))
        || refresh_request_token(path).is_some_and(|token| token <= cutoff)
}

/// Lists one bounded batch from the marker snapshot. UUID-v7 marker names are
/// ordered by creation time; the cutoff makes markers created while a build is
/// running ineligible for acknowledgement even when the directory is larger
/// than the in-memory batch bound.
async fn asset_text_refresh_request_batch_before(
    op: &Operator,
    ws_path: &str,
    cutoff: &str,
) -> Result<Vec<String>> {
    let mut paths = Vec::with_capacity(MAX_ASSET_TEXT_REFRESH_REQUESTS);
    let legacy_path = legacy_asset_text_refresh_request_path(ws_path);
    if op.exists(&legacy_path).await? {
        paths.push(legacy_path);
    }
    let mut lister = op
        .lister_with(&asset_text_refresh_request_prefix(ws_path))
        .recursive(false)
        .await?;
    let mut examined = 0usize;
    while let Some(entry) = lister.try_next().await? {
        examined = examined.saturating_add(1);
        if examined > MAX_ASSET_TEXT_REFRESH_SCAN_ENTRIES {
            bail!(
                "AssetText refresh marker prefix exceeds the {}-entry safety bound",
                MAX_ASSET_TEXT_REFRESH_SCAN_ENTRIES
            );
        }
        let path = entry.path();
        if path.ends_with(".json")
            && refresh_request_is_at_or_before(path, cutoff)
            && paths.len() < MAX_ASSET_TEXT_REFRESH_REQUESTS
        {
            paths.push(path.to_string());
        }
        if paths.len() == MAX_ASSET_TEXT_REFRESH_REQUESTS {
            break;
        }
    }
    Ok(paths)
}

/// A bounded drain is repeated until no marker from the build's snapshot
/// remains. Markers created after the cutoff stay durable for the next worker
/// or startup rearm, including when the initial directory exceeded capacity.
#[cfg(test)]
async fn clear_asset_text_refresh_requests_through(
    op: &Operator,
    ws_path: &str,
    cutoff: &str,
) -> Result<()> {
    loop {
        let paths = asset_text_refresh_request_batch_before(op, ws_path, cutoff).await?;
        if paths.is_empty() {
            return Ok(());
        }
        clear_asset_text_refresh_request_paths(op, &paths).await?;
    }
}

async fn clear_asset_text_refresh_requests_with_admission_lock(
    op: &Operator,
    ws_path: &str,
) -> Result<()> {
    with_asset_text_refresh_admission_lock(op, ws_path, || async {
        // Lock acquisition is the explicit snapshot boundary. All production
        // marker writers use the same admission lock, so draining bounded
        // batches cannot delete a request created after this drain acquired
        // the lease. The cutoff also protects against a stale owner that
        // continues after its heartbeat is lost and another owner takes over.
        let cutoff = Uuid::now_v7().to_string();
        loop {
            let paths = asset_text_refresh_request_batch_before(op, ws_path, &cutoff).await?;
            if paths.is_empty() {
                return Ok(());
            }
            clear_asset_text_refresh_request_paths(op, &paths).await?;
        }
    })
    .await
}

async fn asset_text_refresh_request_count(op: &Operator, ws_path: &str) -> Result<usize> {
    let mut count = usize::from(
        op.exists(&legacy_asset_text_refresh_request_path(ws_path))
            .await?,
    );
    let mut lister = op
        .lister_with(&asset_text_refresh_request_prefix(ws_path))
        .recursive(false)
        .await?;
    let mut examined = 0usize;
    while let Some(entry) = lister.try_next().await? {
        examined = examined.saturating_add(1);
        if examined > MAX_ASSET_TEXT_REFRESH_SCAN_ENTRIES {
            bail!(
                "AssetText refresh marker prefix exceeds the {}-entry safety bound",
                MAX_ASSET_TEXT_REFRESH_SCAN_ENTRIES
            );
        }
        if refresh_request_token(entry.path()).is_some() {
            count = count.saturating_add(1);
            if count >= MAX_ASSET_TEXT_REFRESH_REQUESTS {
                return Ok(count);
            }
        }
    }
    Ok(count)
}

async fn clear_asset_text_refresh_request_paths(op: &Operator, paths: &[String]) -> Result<()> {
    for path in paths {
        match op.delete(path).await {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub async fn mark_asset_text_refresh_requested(op: &Operator, ws_path: &str) -> Result<String> {
    with_asset_text_refresh_admission_lock(op, ws_path, || async {
        if asset_text_refresh_request_count(op, ws_path).await? >= MAX_ASSET_TEXT_REFRESH_REQUESTS {
            bail!("AssetText refresh request markers exceed {MAX_ASSET_TEXT_REFRESH_REQUESTS}");
        }
        let token = Uuid::now_v7().to_string();
        op.write(
            &asset_text_refresh_request_path(ws_path, &token),
            serde_json::to_vec(&json!({"token": token, "requested_at": Utc::now()}))?,
        )
        .await
        .map(|_| token)
        .map_err(Into::into)
    })
    .await
}

pub async fn clear_asset_text_refresh_requested(op: &Operator, ws_path: &str) -> Result<()> {
    clear_asset_text_refresh_requests_with_admission_lock(op, ws_path).await
}

pub async fn asset_text_refresh_requested(op: &Operator, ws_path: &str) -> Result<bool> {
    if op
        .exists(&legacy_asset_text_refresh_request_path(ws_path))
        .await?
    {
        return Ok(true);
    }
    let mut lister = op
        .lister_with(&asset_text_refresh_request_prefix(ws_path))
        .recursive(false)
        .await?;
    let mut examined = 0usize;
    while let Some(entry) = lister.try_next().await? {
        examined = examined.saturating_add(1);
        if examined > MAX_ASSET_TEXT_REFRESH_SCAN_ENTRIES {
            bail!(
                "AssetText refresh marker prefix exceeds the {}-entry safety bound",
                MAX_ASSET_TEXT_REFRESH_SCAN_ENTRIES
            );
        }
        if refresh_request_token(entry.path()).is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Returns whether the durable authoritative source requires an AssetText
/// refresh. The Catalog Head coordinate is the commit-coupled fallback intent:
/// even if a process crashes before it can write a refresh marker, a stale
/// Derived Head remains observable and can be rearmed on the next startup.
pub async fn asset_text_refresh_needed(op: &Operator, ws_path: &str) -> Result<bool> {
    let source_coordinate = authoritative_source_coordinate(op, ws_path).await?;
    let head_store =
        DerivedRelationHeadStore::new(op.clone(), ws_path, DerivedRelationId::ASSET_TEXT.as_uuid())
            .single_process();
    let head = match head_store.read_exact().await {
        Ok(Some(head)) => head.head,
        Ok(None) => {
            // An empty Space has no source coordinate and does not need an
            // initial empty build. Once an authoritative mutation creates a
            // Catalog Head, the missing Derived Head is stale by definition.
            return Ok(source_coordinate
                .get("catalog_head_sha256")
                .is_some_and(|value| !value.is_null()));
        }
        Err(error)
            if error
                .downcast_ref::<ugoite_storage::LegacyDerivedRelationHead>()
                .is_some() =>
        {
            return Ok(true);
        }
        Err(error) => return Err(error),
    };
    let stale = head.source_coordinate != source_coordinate
        || head.producer_fingerprint != asset_text_producer_fingerprint()
        || head.definition_fingerprint != asset_text_definition_fingerprint()
        || head.compatibility_epoch != ASSET_TEXT_COMPATIBILITY_EPOCH;
    if stale {
        return Ok(true);
    }
    asset_text_refresh_requested(op, ws_path).await
}

/// Returns true when a shared Relation Head replacement lost its conditional
/// write race. The immutable build is garbage, but the refresh worker should
/// retry from the newest Head instead of allowing a quiet Space to remain
/// stale indefinitely.
pub fn is_shared_publish_conflict(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<opendal::Error>()
            .is_some_and(|error| error.kind() == ErrorKind::ConditionNotMatch)
    })
}

async fn rebuild_asset_text_with_mode(
    op: &Operator,
    ws_path: &str,
    shared: bool,
) -> Result<DerivedRelationHead> {
    let definition = asset_text_definition();
    let producer_fingerprint = asset_text_producer_fingerprint();
    let relation_uuid = definition.relation_id.as_uuid();
    let head_store = if shared {
        DerivedRelationHeadStore::new(op.clone(), ws_path, relation_uuid)
            .shared()
            .await?
    } else {
        DerivedRelationHeadStore::new(op.clone(), ws_path, relation_uuid).single_process()
    };
    // v1 Heads point to the removed materializations layout. Keep the exact
    // disposable coordinate pinned until the new build is ready, then replace
    // it under the same relation-local mutex/CAS used for current Heads. The
    // detached prefix is marked for grace-period GC only after that swap.
    let _rebuild_guard = if shared {
        None
    } else {
        Some(head_store.single_process_lock().lock_owned().await)
    };
    let mut legacy_expected = None;
    if let Err(error) = head_store.read_exact().await {
        if error
            .downcast_ref::<ugoite_storage::LegacyDerivedRelationHead>()
            .is_some()
        {
            legacy_expected = Some(
                head_store
                    .read_legacy_exact()
                    .await?
                    .context("legacy DerivedRelation Head disappeared")?,
            );
        } else {
            return Err(error);
        }
    }
    let expected = if legacy_expected.is_some() {
        None
    } else {
        head_store.read_exact().await?
    };
    let current_generation = expected
        .as_ref()
        .map(|head| head.head.generation)
        .or_else(|| legacy_expected.as_ref().map(|head| head.generation))
        .unwrap_or(0);
    let generation = current_generation
        .checked_add(1)
        .context("derived generation overflow")?;
    let source_coordinate = authoritative_source_coordinate(op, ws_path).await?;
    let source_rows = collect_source_references(op, ws_path).await?;
    let source_coordinate_after_scan = authoritative_source_coordinate(op, ws_path).await?;
    if source_coordinate != source_coordinate_after_scan {
        bail!("authoritative source changed during AssetText rebuild; retry");
    }
    let input_digest = source_references_digest(&source_rows)?;
    let rows = build_asset_text_rows(op, ws_path, &source_rows, &producer_fingerprint).await?;
    let source_coordinate_after_rows = authoritative_source_coordinate(op, ws_path).await?;
    if source_coordinate != source_coordinate_after_rows {
        bail!("authoritative source changed while parsing AssetText; retry");
    }
    let row_digest = asset_text_rows_digest(&rows)?;
    let build_id = Uuid::now_v7().to_string();
    let build_path = head_store.builds_path(&build_id);
    if let Err(error) = head_store.mark_staging(&build_id).await {
        // Shared marker installation is two durable writes. If the claim was
        // installed but staging.json was not, wake relation maintenance now;
        // the claim/marker recovery path will reclaim the partial build after
        // its normal grace boundary instead of waiting for another mutation or
        // a process restart.
        let _ = ensure_cleanup_marker(&head_store, &build_id).await;
        schedule_asset_text_gc(op, ws_path);
        return Err(error);
    }
    let mut staged_cleanup = StagedBuildCleanup::new(head_store.clone(), build_id.clone());
    let heartbeat_store = head_store.clone();
    let heartbeat_build_id = build_id.clone();
    let staging_heartbeat_lost = Arc::new(AtomicBool::new(false));
    let heartbeat_lost = staging_heartbeat_lost.clone();
    let staging_heartbeat = AbortOnDrop::new(tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if heartbeat_store
                .renew_staging(&heartbeat_build_id)
                .await
                .is_err()
            {
                heartbeat_lost.store(true, Ordering::Release);
                return;
            }
        }
    }));
    // Every object written below belongs to this immutable build. If staging
    // fails before publication, leave an explicit garbage marker and staging
    // marker so relation GC can reclaim the partial prefix as well.
    let build_result: Result<DerivedRelationHead> = async {
        let store = SpaceCatalogStore::new(op.clone(), ws_path)?;
        // Iceberg locations must use the same URI namespace as the official
        // SpaceCatalog FileIO.  `Operator::info().root()` is not a portable
        // warehouse URI for all OpenDAL backends (notably memory and remote
        // stores), so do not manufacture a second location scheme here.
        let table_location = format!("{}{}", iceberg_root_uri(&store), build_path);
        let catalog = DerivedRelationCatalog::new(
            crate::space_catalog::file_io_for_store(&store),
            Runtime::current(),
            NamespaceIdent::new("derived".to_string()),
        );
        let namespace = NamespaceIdent::new("derived".to_string());
        catalog.create_namespace(&namespace, HashMap::new()).await?;
        let table_name = format!("derived_{}", relation_uuid.simple());
        let table = catalog
            .create_table(
                &namespace,
                TableCreation::builder()
                    .name(table_name.clone())
                    .location(table_location)
                    .schema(asset_text_schema())
                    .partition_spec(UnboundPartitionSpec::default())
                    .sort_order(SortOrder::unsorted_order())
                    .build(),
            )
            .await?;
        for batch in rows.chunks(ASSET_TEXT_APPEND_BATCH_ROWS) {
            append_rows(&table, &catalog, batch).await?;
        }
        let final_table = catalog
            .load_table(&TableIdent::new(namespace.clone(), table_name.clone()))
            .await?;
        let metadata_location = final_table.metadata_location_result()?.to_string();
        let status_counts = asset_text_status_counts(&source_rows, &rows);
        let manifest = AssetTextManifest {
            format_version: 2,
            relation_id: relation_uuid.to_string(),
            build_id: build_id.clone(),
            producer_fingerprint: producer_fingerprint.clone(),
            input_digest: input_digest.clone(),
            row_count: rows.len(),
            source_coordinate: source_coordinate.clone(),
            row_digest: row_digest.clone(),
            assets_referenced: status_counts.assets_referenced,
            assets_ready: status_counts.assets_ready,
            assets_empty: status_counts.assets_empty,
            assets_failed: status_counts.assets_failed,
            assets_unsupported: status_counts.assets_unsupported,
        };
        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let manifest_location = format!("{build_path}/manifest.json");
        op.write_options(
            &manifest_location,
            manifest_bytes,
            WriteOptions {
                if_not_exists: true,
                ..Default::default()
            },
        )
        .await?;
        Ok(DerivedRelationHead {
            format_version: 1,
            space_id: space_id_from_metadata(op, ws_path).await?,
            relation_id: relation_uuid.to_string(),
            generation,
            definition_version: definition.definition_version,
            definition_fingerprint: definition.fingerprint(),
            producer_id: ASSET_TEXT_PRODUCER_ID.to_string(),
            producer_fingerprint,
            compatibility_epoch: ASSET_TEXT_COMPATIBILITY_EPOCH,
            build_id: build_id.clone(),
            table_identifier: serde_json::to_value(final_table.identifier())?,
            table_uuid: final_table.metadata().uuid().to_string(),
            metadata_location,
            snapshot_id: final_table.metadata().current_snapshot_id(),
            schema_id: final_table.metadata().current_schema_id(),
            input_digest,
            source_coordinate,
            head_fence: String::new(),
            checksum: String::new(),
        })
    }
    .await;
    staging_heartbeat.abort();
    if staging_heartbeat_lost.load(Ordering::Acquire) {
        let _ = ensure_cleanup_marker(&head_store, &build_id).await;
        schedule_asset_text_gc(op, ws_path);
        return Err(anyhow!(
            "DerivedRelation staging heartbeat was lost before publication"
        ));
    }
    let head = match build_result {
        Ok(head) => head,
        Err(error) => {
            let _ = ensure_cleanup_marker(&head_store, &build_id).await;
            schedule_asset_text_gc(op, ws_path);
            return Err(error);
        }
    };
    let source_coordinate_before_publish = match authoritative_source_coordinate(op, ws_path).await
    {
        Ok(coordinate) => coordinate,
        Err(error) => {
            let _ = ensure_cleanup_marker(&head_store, &head.build_id).await;
            schedule_asset_text_gc(op, ws_path);
            return Err(error);
        }
    };
    if source_coordinate_before_publish != head.source_coordinate {
        let _ = ensure_cleanup_marker(&head_store, &head.build_id).await;
        schedule_asset_text_gc(op, ws_path);
        return Err(anyhow!(
            "authoritative source changed before AssetText publication; retry"
        ));
    }
    let publish_result = if let Some(legacy_expected) = legacy_expected.as_ref() {
        if shared {
            head_store.publish_over_legacy(legacy_expected, &head).await
        } else {
            head_store
                .publish_over_legacy_with_single_process_lock(legacy_expected, &head)
                .await
        }
    } else if shared {
        head_store.publish(expected.as_ref(), &head).await
    } else {
        head_store
            .publish_with_single_process_lock(expected.as_ref(), &head)
            .await
    };
    if let Err(publication_error) = publish_result {
        // A conditional write can succeed at the storage boundary while its
        // response is lost.  Derived relations do not need the authoritative
        // publication-chain proof, but they still reread their own Head and
        // accept the outcome when this build command is visibly current.
        if let Ok(Some(current)) = head_store.read_exact().await {
            if current.head.build_id == head.build_id {
                if let Some(expected) = expected.as_ref() {
                    let _ = ensure_cleanup_marker(&head_store, &expected.head.build_id).await;
                }
                if legacy_expected.is_some() {
                    let _ = head_store.mark_legacy_materializations_garbage().await;
                }
                // A previous uncertain publication may have left a garbage
                // marker on this build. Once the exact Head reread proves the
                // build is current, that marker must not retain its old age
                // and make the next swap eligible for premature GC.
                let _ = head_store.clear_garbage(&head.build_id).await;
                // Keep staging protection until the successful Head outcome
                // is observed. A slow GC must not delete this build between
                // validation and publication.
                let _ = head_store.clear_staging(&head.build_id).await;
                schedule_asset_text_gc(op, ws_path);
                return Ok(current.head);
            }
        }
        let _ = ensure_cleanup_marker(&head_store, &head.build_id).await;
        schedule_asset_text_gc(op, ws_path);
        return Err(publication_error);
    }
    if let Some(expected) = expected.as_ref() {
        // The CAS already made the new build visible. Record the superseded
        // build before the confirmation read so a transient read failure does
        // not strand it without a durable cleanup candidate.
        let _ = ensure_cleanup_marker(&head_store, &expected.head.build_id).await;
    }
    if legacy_expected.is_some() {
        // The new Head is now authoritative. Retain the detached v1 prefix
        // until its reader grace period expires; the marker is durable so a
        // crash after the CAS cannot strand the old bytes.
        let _ = head_store.mark_legacy_materializations_garbage().await;
    }
    schedule_asset_text_gc(op, ws_path);
    let current = match head_store.read_exact().await {
        Ok(Some(current)) => current.head,
        Ok(None) => {
            let _ = ensure_cleanup_marker(&head_store, &head.build_id).await;
            schedule_asset_text_gc(op, ws_path);
            return Err(anyhow::anyhow!("published derived Head disappeared"));
        }
        Err(error) => {
            let _ = ensure_cleanup_marker(&head_store, &head.build_id).await;
            schedule_asset_text_gc(op, ws_path);
            return Err(error);
        }
    };
    let candidate_cleanup_marked = if current.build_id != head.build_id {
        // A shared writer may have won immediately after this writer's CAS.
        // The successful candidate is then garbage too; leaving only its
        // staging marker would make it invisible to lifecycle GC forever.
        ensure_cleanup_marker(&head_store, &head.build_id).await
    } else {
        // The build may have been conservatively marked garbage after an
        // uncertain CAS response. Current Head confirmation makes that marker
        // invalid, regardless of its original timestamp.
        let _ = head_store.clear_garbage(&head.build_id).await;
        true
    };
    // A completed build no longer needs its active-build heartbeat. If this
    // delete is interrupted, the conservative staging marker still keeps the
    // build protected until the grace period has elapsed.
    if candidate_cleanup_marked {
        let _ = head_store.clear_staging(&head.build_id).await;
    }
    let source_coordinate_after_publish = match authoritative_source_coordinate(op, ws_path).await {
        Ok(coordinate) => coordinate,
        Err(error) => {
            if current.build_id == head.build_id {
                let _ = ensure_cleanup_marker(&head_store, &current.build_id).await;
            }
            schedule_asset_text_gc(op, ws_path);
            return Err(error);
        }
    };
    if source_coordinate_after_publish != current.source_coordinate {
        if current.build_id == head.build_id {
            let _ = ensure_cleanup_marker(&head_store, &current.build_id).await;
        }
        schedule_asset_text_gc(op, ws_path);
        return Err(anyhow!(
            "authoritative source changed after AssetText publication; retry"
        ));
    }
    if current.build_id == head.build_id {
        staged_cleanup.disarm();
    }
    let _ = head_store
        .garbage_collect_with_single_process_lock(Some(&current.build_id), MINIMUM_GC_AGE)
        .await;
    schedule_asset_text_gc(op, ws_path);
    // Remove only the requests observed before this build. The drain is
    // bounded per listing/deletion batch, so marker overflow cannot strand
    // old requests or require an unbounded in-memory snapshot.
    if let Err(error) = clear_asset_text_refresh_requests_with_admission_lock(op, ws_path).await {
        // Head publication is already complete. Keep durable request markers
        // for the next maintenance pass instead of reporting a failed build
        // and triggering full-rebuild retry churn.
        eprintln!("AssetText refresh marker cleanup deferred for {ws_path}: {error:#}");
        schedule_asset_text_gc(op, ws_path);
    }
    Ok(current)
}

fn schedule_asset_text_gc(op: &Operator, ws_path: &str) {
    schedule_asset_text_gc_after_delay(op, ws_path, MINIMUM_GC_AGE);
}

fn schedule_asset_text_gc_after_delay(op: &Operator, ws_path: &str, delay: Duration) {
    let relation_id = asset_text_definition().relation_id.as_uuid();
    let key = format!(
        "{}:{}:{}:operator={:p}:{}:{}",
        op.info().scheme(),
        op.info().name(),
        op.info().root(),
        Arc::as_ptr(op.service()),
        ws_path,
        relation_id,
    );
    let schedulers = ASSET_TEXT_GC_SCHEDULERS.get_or_init(|| StdMutex::new(BTreeMap::new()));
    let scheduler = {
        let mut schedulers = schedulers
            .lock()
            .expect("AssetText GC scheduler map poisoned");
        if !schedulers.contains_key(&key) && schedulers.len() >= MAX_BACKGROUND_GC_SCHEDULERS {
            // Derived refresh is best effort.  A full process-local registry
            // must not turn an authoritative mutation into unbounded memory
            // growth; explicit `index run` remains the repair path.
            return;
        }
        let scheduler = schedulers
            .entry(key.clone())
            .or_insert_with(|| {
                Arc::new(AssetTextGcScheduler {
                    notify: Notify::new(),
                    deadline: StdMutex::new(None),
                    started: AtomicBool::new(false),
                })
            })
            .clone();
        let mut deadline = scheduler
            .deadline
            .lock()
            .expect("AssetText GC deadline poisoned");
        let next = Instant::now() + delay;
        if deadline.is_none_or(|current| next < current) {
            *deadline = Some(next);
        }
        drop(deadline);
        scheduler
    };
    scheduler.notify.notify_one();
    if scheduler
        .started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let operator = op.clone();
        let workspace_path = ws_path.to_string();
        let scheduler_key = key;
        let scheduler = scheduler.clone();
        tokio::spawn(async move {
            let mut consecutive_failures = 0usize;
            loop {
                let deadline = scheduler
                    .deadline
                    .lock()
                    .expect("AssetText GC deadline poisoned")
                    .to_owned();
                let Some(deadline) = deadline else {
                    scheduler.notify.notified().await;
                    continue;
                };
                tokio::select! {
                    _ = scheduler.notify.notified() => {}
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                        let due = {
                            let mut deadline_slot = scheduler
                                .deadline
                                .lock()
                                .expect("AssetText GC deadline poisoned");
                            if deadline_slot.is_some_and(|current| current <= Instant::now()) {
                                *deadline_slot = None;
                                true
                            } else {
                                false
                            }
                        };
                        if !due {
                            continue;
                        }
                        let base = DerivedRelationHeadStore::new(
                            operator.clone(),
                            &workspace_path,
                            relation_id,
                        );
                        let maintenance = tokio::time::timeout(GC_OPERATION_TIMEOUT, async {
                            let head_store = if matches!(
                                operator.info().scheme(),
                                "s3" | "gcs" | "oss" | "azdls"
                            ) {
                                base.shared().await?
                            } else {
                                base.single_process()
                            };
                            let current_build = head_store.read_exact().await?;
                            let current_build_id =
                                current_build.map(|head| head.head.build_id);
                            head_store.mark_legacy_materializations_garbage().await?;
                            head_store
                                .garbage_collect_legacy_materializations(MINIMUM_GC_AGE)
                                .await?;
                            head_store
                                .garbage_collect(
                                    current_build_id.as_deref(),
                                    MINIMUM_GC_AGE,
                                )
                                .await?;
                            let pending = head_store
                                .has_pending_garbage(
                                    current_build_id.as_deref(),
                                    MINIMUM_GC_AGE,
                                )
                                .await?;
                            Ok::<bool, anyhow::Error>(pending)
                        })
                        .await;
                        let (retry_gc, storage_failed) = match maintenance {
                            Ok(Ok(pending)) => (pending, false),
                            Ok(Err(_)) | Err(_) => (true, true),
                        };
                        if storage_failed {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                        } else {
                            consecutive_failures = 0;
                        }
                        if consecutive_failures >= MAX_CONSECUTIVE_GC_FAILURES {
                            // Durable markers are rediscovered on the next
                            // process startup or explicit maintenance run;
                            // never retain an Operator forever on a broken
                            // backend.
                            let mut schedulers = ASSET_TEXT_GC_SCHEDULERS
                                .get_or_init(|| StdMutex::new(BTreeMap::new()))
                                .lock()
                                .expect("AssetText GC scheduler map poisoned");
                            if schedulers
                                .get(&scheduler_key)
                                .is_some_and(|current| Arc::ptr_eq(current, &scheduler))
                            {
                                schedulers.remove(&scheduler_key);
                            }
                            return;
                        }
                        if retry_gc {
                            // GC is maintenance, not request authority. Keep
                            // the scheduler alive when cleanup was deferred or
                            // a durable candidate remains, and retry transient
                            // storage failures instead of losing the only
                            // process-local wake-up for durable garbage.
                            schedule_asset_text_gc_after_delay(
                                &operator,
                                &workspace_path,
                                GC_RETRY_DELAY,
                            );
                        }
                        let should_exit = {
                            // Schedule and retirement both take the map lock
                            // before the scheduler deadline lock. This makes
                            // the final idle check atomic with respect to a
                            // new refresh notification and lets the task
                            // remove its own per-Space entry safely.
                            let mut schedulers = ASSET_TEXT_GC_SCHEDULERS
                                .get_or_init(|| StdMutex::new(BTreeMap::new()))
                                .lock()
                                .expect("AssetText GC scheduler map poisoned");
                            let is_current = schedulers
                                .get(&scheduler_key)
                                .is_some_and(|current| Arc::ptr_eq(current, &scheduler));
                            let idle = scheduler
                                .deadline
                                .lock()
                                .expect("AssetText GC deadline poisoned")
                                .is_none();
                            if is_current && idle {
                                schedulers.remove(&scheduler_key);
                                true
                            } else {
                                false
                            }
                        };
                        if should_exit {
                            return;
                        }
                    }
                }
            }
        });
    }
}

async fn ensure_cleanup_marker(head_store: &DerivedRelationHeadStore, build_id: &str) -> bool {
    for _ in 0..3 {
        if let Ok(()) = head_store.mark_garbage(build_id).await {
            return true;
        }
    }
    // A staging marker is also a durable GC candidate. Keep it when writing
    // garbage.json is temporarily unavailable, so a successful Head swap can
    // never make the superseded prefix undiscoverable to a later GC pass.
    for _ in 0..3 {
        if head_store.mark_staging(build_id).await.is_ok() {
            return false;
        }
    }
    false
}

/// Runs the relation GC synchronously for one-shot local maintenance commands.
/// The normal server path schedules the same work after the reader grace
/// period; a short-lived CLI cannot keep that timer task alive after exit.
pub async fn garbage_collect_asset_text(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    let relation_id = asset_text_definition().relation_id.as_uuid();
    let base = DerivedRelationHeadStore::new(op.clone(), ws_path, relation_id);
    let head_store = if matches!(op.info().scheme(), "s3" | "gcs" | "oss" | "azdls") {
        base.shared().await?
    } else {
        base.single_process()
    };
    let current_build_id = match head_store.read_exact().await {
        Ok(head) => {
            // A current-build Head is authoritative. Any v1 materialization
            // prefix is now detached garbage, including one left by a crash
            // immediately after a shared legacy-to-current swap. Discovery and
            // deletion both honor the reader grace period.
            head_store.mark_legacy_materializations_garbage().await?;
            head_store
                .garbage_collect_legacy_materializations(MINIMUM_GC_AGE)
                .await?;
            head.map(|head| head.head.build_id)
        }
        Err(error)
            if error
                .downcast_ref::<ugoite_storage::LegacyDerivedRelationHead>()
                .is_some() =>
        {
            // Never mark or delete the v1 prefix while its legacy Head still
            // points at it. A later rebuild will replace that Head by exact
            // CAS first.
            None
        }
        Err(error) => return Err(error),
    };
    head_store
        .garbage_collect(current_build_id.as_deref(), MINIMUM_GC_AGE)
        .await
}

/// Rehydrates the process-local GC wake-up for an existing Space. This is
/// called during server startup so durable markers from a previous process are
/// discovered without waiting for a new rebuild.
pub async fn rearm_asset_text_gc(op: &Operator, ws_path: &str) -> Result<()> {
    match garbage_collect_asset_text(op, ws_path).await {
        Ok(_) => {
            schedule_asset_text_gc(op, ws_path);
            Ok(())
        }
        Err(error) => {
            schedule_asset_text_gc_after_delay(op, ws_path, GC_RETRY_DELAY);
            Err(error)
        }
    }
}

fn iceberg_root_uri(store: &SpaceCatalogStore) -> String {
    let warehouse_uri = &store.iceberg_storage().warehouse_uri;
    if warehouse_uri == "memory:" {
        "memory:///".to_string()
    } else {
        format!("{}/", warehouse_uri.trim_end_matches('/'))
    }
}

async fn append_rows(
    table: &Table,
    catalog: &DerivedRelationCatalog,
    rows: &[AssetTextRow],
) -> Result<()> {
    let arrow_schema = Arc::new(iceberg::arrow::schema_to_arrow_schema(
        table.metadata().current_schema(),
    )?);
    let mut asset_id = StringBuilder::new();
    let mut source_sha256 = StringBuilder::new();
    let mut source_size_bytes = Int64Builder::new();
    let mut parser_id = StringBuilder::new();
    let mut parser_version = StringBuilder::new();
    let mut producer_fingerprint = StringBuilder::new();
    let mut status = StringBuilder::new();
    let mut chunk_index = Int64Builder::new();
    let mut source_locator = StringBuilder::new();
    let mut text = StringBuilder::new();
    let mut text_length = Int64Builder::new();
    let mut parsed_at = Vec::with_capacity(rows.len());
    let mut error_code = StringBuilder::new();
    for row in rows {
        asset_id.append_value(&row.asset_id);
        source_sha256.append_value(&row.source_sha256);
        source_size_bytes.append_value(row.source_size_bytes);
        parser_id.append_value(&row.parser_id);
        parser_version.append_value(&row.parser_version);
        producer_fingerprint.append_value(&row.producer_fingerprint);
        status.append_value(&row.status);
        chunk_index.append_value(row.chunk_index);
        source_locator.append_option(row.source_locator.as_deref());
        text.append_option(row.text.as_deref());
        text_length.append_value(row.text_length);
        parsed_at.push(chrono::DateTime::parse_from_rfc3339(&row.parsed_at)?.timestamp_micros());
        error_code.append_option(row.error_code.as_deref());
    }
    let batch = RecordBatch::try_new(
        arrow_schema,
        vec![
            Arc::new(asset_id.finish()),
            Arc::new(source_sha256.finish()),
            Arc::new(source_size_bytes.finish()),
            Arc::new(parser_id.finish()),
            Arc::new(parser_version.finish()),
            Arc::new(producer_fingerprint.finish()),
            Arc::new(status.finish()),
            Arc::new(chunk_index.finish()),
            Arc::new(source_locator.finish()),
            Arc::new(text.finish()),
            Arc::new(text_length.finish()),
            Arc::new(TimestampMicrosecondArray::from_iter_values(parsed_at).with_timezone_utc()),
            Arc::new(error_code.finish()),
        ],
    )?;
    let table_properties = table.metadata().table_properties()?;
    let parquet_writer = ParquetWriterBuilder::from_table_properties(
        &table_properties,
        table.metadata().current_schema().clone(),
    )?;
    let location_generator = DefaultLocationGenerator::new(table.metadata())?;
    let file_name_generator = DefaultFileNameGenerator::new(
        Uuid::now_v7().to_string(),
        None,
        iceberg::spec::DataFileFormat::Parquet,
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
    writer.write(batch).await?;
    let data_files = writer.close().await?;
    if data_files.is_empty() {
        bail!("AssetText writer produced no data files");
    }
    let tx = Transaction::new(table);
    tx.fast_append()
        .add_data_files(data_files)
        .apply(tx)?
        .commit(catalog)
        .await?;
    Ok(())
}

fn asset_text_rows_digest(rows: &[AssetTextRow]) -> Result<String> {
    // Digest rows one at a time.  The aggregate output is already bounded,
    // but canonicalizing the whole Vec would create a second peak-sized JSON
    // allocation before Arrow writing starts.
    let mut digest = Sha256::new();
    for row in rows {
        digest.update(canonical_json(row)?);
        digest.update([0]);
    }
    Ok(format!("sha256:{}", hex::encode(digest.finalize())))
}

fn source_references_digest(references: &[SourceReference]) -> Result<String> {
    let mut digest = Sha256::new();
    for reference in references {
        digest.update(canonical_json(reference)?);
        digest.update([0]);
    }
    Ok(format!("sha256:{}", hex::encode(digest.finalize())))
}

async fn space_id_from_metadata(op: &Operator, ws_path: &str) -> Result<String> {
    let path = format!("{ws_path}/meta.json");
    let value: Value = serde_json::from_slice(&op.read(&path).await?.to_vec())?;
    value
        .get("space_uid")
        .or_else(|| value.get("space_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .context("Space metadata has no immutable space ID")
}

async fn authoritative_source_coordinate(op: &Operator, ws_path: &str) -> Result<Value> {
    let store = SpaceCatalogStore::new(op.clone(), ws_path)?.single_process();
    let Some(exact) = store.read_exact_head().await? else {
        // A newly created, still-empty Space has no Catalog Head.  This is a
        // valid empty source coordinate, not a derived corruption state.
        return Ok(json!({"catalog_head_sha256": Value::Null, "catalog_head_etag": Value::Null}));
    };
    Ok(json!({
        "catalog_head_sha256": sha256_digest(&exact.bytes),
        "catalog_head_etag": exact.etag,
    }))
}

async fn collect_source_references(op: &Operator, ws_path: &str) -> Result<Vec<SourceReference>> {
    let workspace = crate::iceberg_store::native_workspace(op, ws_path).await?;
    let definitions = workspace
        .list_forms_bounded(MAX_SOURCE_FORMS, MAX_SOURCE_FORM_DEFINITION_BYTES)
        .await?
        .into_iter()
        .map(|definition| (definition.name.clone(), definition))
        .collect::<BTreeMap<_, _>>();
    let form_names = definitions.keys().cloned().collect::<Vec<_>>();
    let mut references = BTreeMap::<String, SourceReference>::new();
    let mut asset_checksums = BTreeMap::<String, (String, u64)>::new();
    let mut conflicting_assets = HashSet::new();
    let mut source_metadata_bytes = 0usize;
    for form_name in form_names {
        let definition = definitions
            .get(&form_name)
            .context("current Entry Form missing")?;
        // This canonical latest-revision view scans the authoritative Form
        // table directly and is not subject to the normal 10k search window.
        // Delete tombstones are deliberately excluded after max-version
        // selection, so deleted Entries cannot seed a derived source set.
        workspace
            .visit_current_revision_view_for_derived(definition.id, |revision| {
                if matches!(
                    revision.operation,
                    ugoite_domain::entry::EntryOperation::Delete
                ) {
                    return Ok(());
                }
                let mut asset_reference_count = 0usize;
                for field in &definition.fields {
                    if !matches!(
                        field.field_type,
                        FieldType::AssetReference | FieldType::List
                    ) {
                        continue;
                    }
                    let Some(value) = revision.values.get(&field.id) else {
                        continue;
                    };
                    let asset_references = typed_asset_references_for_field(field, value)?;
                    asset_reference_count = asset_reference_count
                        .checked_add(asset_references.len())
                        .ok_or_else(|| anyhow!("Entry AssetReference count overflowed"))?;
                    if asset_reference_count > MAX_ASSET_REFERENCES_PER_ENTRY {
                        bail!(
                            "Entry AssetReference payload exceeds {MAX_ASSET_REFERENCES_PER_ENTRY} references"
                        );
                    }
                    for reference in asset_references {
                        let candidate = SourceReference {
                            asset_id: reference.asset_id,
                            name: reference.name,
                            media_type: reference.media_type,
                            source_sha256: reference.sha256,
                            source_size_bytes: reference.size_bytes,
                            integrity_error: None,
                        };
                        if !references.contains_key(&candidate.asset_id)
                            && references.len() >= MAX_SOURCE_ASSETS
                        {
                            bail!("AssetText source exceeds its unique-asset limit");
                        }
                        if !references.contains_key(&candidate.asset_id) {
                            let candidate_bytes = [
                                candidate.asset_id.len(),
                                candidate.name.len(),
                                candidate.media_type.len(),
                                candidate.source_sha256.len(),
                                std::mem::size_of::<SourceReference>(),
                            ]
                            .into_iter()
                            .try_fold(0usize, usize::checked_add)
                            .context("AssetText source metadata size overflow")?;
                            source_metadata_bytes = source_metadata_bytes
                                .checked_add(candidate_bytes)
                                .context("AssetText source metadata size overflow")?;
                            if source_metadata_bytes > MAX_SOURCE_METADATA_BYTES {
                                bail!("AssetText source metadata exceeds its byte limit");
                            }
                        }
                        merge_source_reference(
                            &mut references,
                            &mut asset_checksums,
                            &mut conflicting_assets,
                            candidate,
                        );
                    }
                }
                Ok(())
            })
            .await?;
    }
    Ok(references
        .into_values()
        .map(|mut reference| {
            if conflicting_assets.contains(&reference.asset_id) {
                reference.integrity_error = Some(
                    DerivedErrorCode::SourceRevisionIntegrityFailed
                        .as_str()
                        .to_string(),
                );
            }
            reference
        })
        .collect())
}

fn merge_source_reference(
    references: &mut BTreeMap<String, SourceReference>,
    asset_checksums: &mut BTreeMap<String, (String, u64)>,
    conflicting_assets: &mut HashSet<String>,
    candidate: SourceReference,
) {
    if let Some((sha256, size_bytes)) = asset_checksums.get(&candidate.asset_id) {
        if sha256 != &candidate.source_sha256 || *size_bytes != candidate.source_size_bytes {
            conflicting_assets.insert(candidate.asset_id.clone());
        }
    } else {
        asset_checksums.insert(
            candidate.asset_id.clone(),
            (candidate.source_sha256.clone(), candidate.source_size_bytes),
        );
    }
    references
        .entry(candidate.asset_id.clone())
        .and_modify(|existing| {
            if existing.source_sha256 != candidate.source_sha256
                || existing.source_size_bytes != candidate.source_size_bytes
            {
                conflicting_assets.insert(candidate.asset_id.clone());
            } else if source_reference_is_preferred(&candidate, existing) {
                // Asset metadata is Entry-owned and can be represented by
                // several references. Keep the most useful parser hint so a
                // generic first reference cannot make a parse unsupported.
                existing.name = candidate.name.clone();
                existing.media_type = candidate.media_type.clone();
            }
        })
        .or_insert(candidate);
}

fn source_reference_metadata_rank(reference: &SourceReference) -> u8 {
    let name = reference.name.to_ascii_lowercase();
    let media_type = reference.media_type.to_ascii_lowercase();
    if matches!(media_type.as_str(), "text/plain" | "text/markdown") {
        // Text MIME is the safest fallback for an opaque object. Structured
        // formats still win when their bytes have a PDF/OOXML signature in
        // detect_dispatch, while this avoids routing plain bytes through a
        // conflicting PDF or Office hint.
        4
    } else if matches!(
        media_type.as_str(),
        "application/pdf"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    ) {
        3
    } else if [".pdf", ".txt", ".md", ".docx", ".xlsx", ".pptx"]
        .iter()
        .any(|extension| name.ends_with(extension))
    {
        2
    } else if !reference.name.is_empty() || !reference.media_type.is_empty() {
        1
    } else {
        0
    }
}

fn source_reference_is_preferred(candidate: &SourceReference, existing: &SourceReference) -> bool {
    let candidate_rank = source_reference_metadata_rank(candidate);
    let existing_rank = source_reference_metadata_rank(existing);
    candidate_rank > existing_rank
        || (candidate_rank == existing_rank
            && source_reference_tie_break_key(candidate) < source_reference_tie_break_key(existing))
}

fn source_reference_tie_break_key(reference: &SourceReference) -> (String, String, String, String) {
    (
        reference.name.to_ascii_lowercase(),
        reference.media_type.to_ascii_lowercase(),
        reference.name.clone(),
        reference.media_type.clone(),
    )
}

fn typed_asset_references_for_field(
    field: &ugoite_domain::form::FormField,
    value: &ugoite_domain::entry::FieldValue,
) -> Result<Vec<AssetReference>> {
    match (&field.field_type, field.list_item.as_ref(), value) {
        (_, _, ugoite_domain::entry::FieldValue::Null) => Ok(Vec::new()),
        (
            FieldType::AssetReference,
            _,
            ugoite_domain::entry::FieldValue::AssetReference(reference),
        ) => {
            reference
                .validate()
                .map_err(|error| anyhow!("invalid persisted AssetReference: {error}"))?;
            Ok(vec![reference.clone()])
        }
        (FieldType::AssetReference, _, _) => {
            bail!("Entry AssetReference field has an invalid value")
        }
        (FieldType::List, Some(item), ugoite_domain::entry::FieldValue::List(values))
            if item.field_type == FieldType::AssetReference =>
        {
            if values.len() > MAX_ASSET_REFERENCES_PER_ENTRY {
                bail!("Entry AssetReference list exceeds {MAX_ASSET_REFERENCES_PER_ENTRY} items");
            }
            values
                .iter()
                .filter_map(|value| match value {
                    ugoite_domain::entry::FieldValue::Null => None,
                    ugoite_domain::entry::FieldValue::AssetReference(reference) => Some(
                        reference
                            .validate()
                            .map_err(|error| anyhow!("invalid persisted AssetReference: {error}"))
                            .map(|()| reference.clone()),
                    ),
                    _ => Some(Err(anyhow::anyhow!(
                        "Entry AssetReference list has an invalid value"
                    ))),
                })
                .collect()
        }
        _ => Ok(Vec::new()),
    }
}

fn asset_text_status_counts(
    references: &[SourceReference],
    rows: &[AssetTextRow],
) -> AssetTextStatusCounts {
    let mut status_by_asset = BTreeMap::<String, String>::new();
    for reference in references {
        status_by_asset
            .entry(reference.asset_id.clone())
            .or_insert_with(|| "failed".to_string());
    }
    for row in rows {
        let status = status_by_asset
            .entry(row.asset_id.clone())
            .or_insert_with(|| row.status.clone());
        if status == "failed" || status == "missing" || status == "source_mismatch" {
            *status = row.status.clone();
        } else if row.status == "ready" {
            *status = "ready".to_string();
        }
    }
    let mut counts = AssetTextStatusCounts {
        assets_referenced: status_by_asset.len(),
        ..Default::default()
    };
    for status in status_by_asset.values() {
        match status.as_str() {
            "ready" => counts.assets_ready += 1,
            "empty" => counts.assets_empty += 1,
            "unsupported" => counts.assets_unsupported += 1,
            _ => counts.assets_failed += 1,
        }
    }
    counts
}

async fn build_asset_text_rows(
    op: &Operator,
    ws_path: &str,
    references: &[SourceReference],
    producer_fingerprint: &str,
) -> Result<Vec<AssetTextRow>> {
    let parsed_at = Utc::now().to_rfc3339();
    let mut rows = BoundedAssetTextRows::default();
    for reference in references {
        validate_asset_id(&reference.asset_id)
            .map_err(|error| anyhow!("invalid AssetReference asset_id: {error}"))?;
        if rows.failed() {
            return rows.finish();
        }
        let base = |parser_id: String,
                    parser_version: String,
                    status: &str,
                    chunk_index: i64,
                    locator: Option<String>,
                    text: Option<String>,
                    error_code: Option<&str>| AssetTextRow {
            asset_id: reference.asset_id.clone(),
            source_sha256: reference.source_sha256.clone(),
            source_size_bytes: i64::try_from(reference.source_size_bytes).unwrap_or(i64::MAX),
            parser_id,
            parser_version,
            producer_fingerprint: producer_fingerprint.to_string(),
            status: status.to_string(),
            chunk_index,
            source_locator: locator,
            text_length: text
                .as_ref()
                .map(|value| value.chars().count() as i64)
                .unwrap_or(0),
            text,
            parsed_at: parsed_at.clone(),
            error_code: error_code.map(str::to_string),
        };
        let path = format!("{ws_path}/assets/{}", reference.asset_id);
        if let Some(error_code) = reference.integrity_error.as_deref() {
            rows.push(base(
                "integrity".into(),
                ASSET_TEXT_PARSER_VERSION.into(),
                "failed",
                0,
                None,
                None,
                Some(error_code),
            ));
            continue;
        }
        let bytes = match read_asset_exact(op, &path).await {
            Ok(bytes) => bytes,
            Err(error)
                if error
                    .downcast_ref::<opendal::Error>()
                    .is_some_and(|error| error.kind() == ErrorKind::NotFound) =>
            {
                rows.push(base(
                    "missing".into(),
                    ASSET_TEXT_PARSER_VERSION.into(),
                    "missing",
                    0,
                    None,
                    None,
                    Some(DerivedErrorCode::AssetMissing.as_str()),
                ));
                continue;
            }
            Err(error) if error.downcast_ref::<AssetParserInputLimit>().is_some() => {
                rows.push(base(
                    "reader".into(),
                    ASSET_TEXT_PARSER_VERSION.into(),
                    "failed",
                    0,
                    None,
                    None,
                    Some(DerivedErrorCode::AssetParserLimit.as_str()),
                ));
                continue;
            }
            Err(_) => {
                rows.push(base(
                    "reader".into(),
                    ASSET_TEXT_PARSER_VERSION.into(),
                    "failed",
                    0,
                    None,
                    None,
                    Some(DerivedErrorCode::AssetParserFailed.as_str()),
                ));
                continue;
            }
        };
        if bytes.len() as u64 != reference.source_size_bytes {
            rows.push(base(
                "integrity".into(),
                ASSET_TEXT_PARSER_VERSION.into(),
                "source_mismatch",
                0,
                None,
                None,
                Some(DerivedErrorCode::AssetSizeMismatch.as_str()),
            ));
            continue;
        }
        if bytes.len() as u64 > MAX_ASSET_BYTES {
            rows.push(base(
                "limits".into(),
                ASSET_TEXT_PARSER_VERSION.into(),
                "failed",
                0,
                None,
                None,
                Some(DerivedErrorCode::AssetParserLimit.as_str()),
            ));
            continue;
        }
        let (actual_sha, parser, unsupported, chunks) =
            match process_asset_async(reference.name.clone(), reference.media_type.clone(), bytes)
                .await
            {
                Ok(value) => value,
                Err(code) => {
                    rows.push(base(
                        "parser".into(),
                        ASSET_TEXT_PARSER_VERSION.into(),
                        "failed",
                        0,
                        None,
                        None,
                        Some(coarse_parser_error_code(code)),
                    ));
                    continue;
                }
            };
        if actual_sha != reference.source_sha256 {
            rows.push(base(
                "integrity".into(),
                ASSET_TEXT_PARSER_VERSION.into(),
                "source_mismatch",
                0,
                None,
                None,
                Some(DerivedErrorCode::AssetChecksumMismatch.as_str()),
            ));
            continue;
        }
        if unsupported {
            rows.push(base(
                parser.id.into(),
                parser.version.into(),
                "unsupported",
                0,
                None,
                None,
                Some(DerivedErrorCode::AssetUnsupportedFormat.as_str()),
            ));
        } else if chunks.is_empty() {
            rows.push(base(
                parser.id.into(),
                parser.version.into(),
                "empty",
                0,
                None,
                None,
                None,
            ));
        } else {
            for (index, chunk) in chunks.into_iter().enumerate() {
                let text = chunk.text;
                let status = if text.is_empty() { "empty" } else { "ready" };
                rows.push(base(
                    parser.id.into(),
                    parser.version.into(),
                    status,
                    i64::try_from(index).unwrap_or(i64::MAX),
                    Some(serde_json::to_string(&chunk.locator)?),
                    (!text.is_empty()).then_some(text),
                    None,
                ));
                if rows.failed() {
                    return rows.finish();
                }
            }
        }
    }
    rows.finish()
}

async fn read_asset_exact(op: &Operator, path: &str) -> Result<Vec<u8>> {
    read_asset_exact_with_limit(op, path, MAX_ASSET_BYTES as usize).await
}

async fn read_asset_exact_with_limit(
    op: &Operator,
    path: &str,
    max_bytes: usize,
) -> Result<Vec<u8>> {
    let metadata = op.stat(path).await?;
    let mut reader = op.reader_with(path);
    if let Some(etag) = metadata.etag().filter(|etag| !etag.is_empty()) {
        reader = reader.if_match(etag);
    }
    let reader = reader.chunk(READER_CHUNK_BYTES).await?;
    let mut stream = reader.into_stream(0..).await?;
    let mut bytes = Vec::new();
    while let Some(buffer) = stream.try_next().await? {
        bytes.extend(buffer.into_iter().flatten());
        if bytes.len() > max_bytes {
            return Err(anyhow::Error::new(AssetParserInputLimit));
        }
    }
    Ok(bytes)
}

fn detect_dispatch(name: &str, media_type: &str, bytes: &[u8]) -> Dispatch {
    let lower_name = name.to_ascii_lowercase();
    let lower_media = media_type.to_ascii_lowercase();
    // Object bytes are authoritative when they carry a strong container
    // signature. Entry-owned names can differ for the same Asset, so inspect
    // the immutable object before using a per-reference filename hint.
    if bytes.starts_with(b"%PDF-") {
        return Dispatch::Pdf(parser_identity("pdf_text_layer"));
    }
    // MIME and filename are only hints. Valid OOXML containers are recognized
    // by their internal part names before either hint can misroute them.
    if bytes.starts_with(b"PK") && validate_zip_entry_count(bytes).is_ok() {
        if let Ok(mut archive) = ZipArchive::new(Cursor::new(bytes)) {
            if archive.len() <= MAX_ZIP_ENTRIES {
                let mut has_document = false;
                let mut has_workbook = false;
                let mut has_slides = false;
                for index in 0..archive.len() {
                    if let Ok(file) = archive.by_index(index) {
                        let part = file.name();
                        has_document |= part == "word/document.xml";
                        has_workbook |= part.starts_with("xl/workbook") && part.ends_with(".xml");
                        has_slides |=
                            part.starts_with("ppt/slides/slide") && part.ends_with(".xml");
                    }
                }
                if has_document {
                    return Dispatch::Docx(parser_identity("docx_xml"));
                }
                if has_workbook {
                    return Dispatch::Xlsx(parser_identity("xlsx_xml"));
                }
                if has_slides {
                    return Dispatch::Pptx(parser_identity("pptx_xml"));
                }
            }
        }
    }
    // A consistent MIME type outranks a conflicting Entry-owned filename.
    // This is important for one Asset referenced by Entries with different
    // display names.
    if lower_media == "application/pdf" {
        return Dispatch::Pdf(parser_identity("pdf_text_layer"));
    }
    if lower_media == "application/vnd.openxmlformats-officedocument.wordprocessingml.document" {
        return Dispatch::Docx(parser_identity("docx_xml"));
    }
    if lower_media == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" {
        return Dispatch::Xlsx(parser_identity("xlsx_xml"));
    }
    if lower_media == "application/vnd.openxmlformats-officedocument.presentationml.presentation" {
        return Dispatch::Pptx(parser_identity("pptx_xml"));
    }
    if lower_media == "text/plain" || lower_media == "text/markdown" {
        return Dispatch::PlainText(parser_identity(if lower_media == "text/markdown" {
            "markdown"
        } else {
            "plain_text"
        }));
    }
    if lower_name.ends_with(".pdf") {
        return Dispatch::Pdf(parser_identity("pdf_text_layer"));
    }
    if lower_name.ends_with(".docx") {
        return Dispatch::Docx(parser_identity("docx_xml"));
    }
    if lower_name.ends_with(".xlsx") {
        return Dispatch::Xlsx(parser_identity("xlsx_xml"));
    }
    if lower_name.ends_with(".pptx") {
        return Dispatch::Pptx(parser_identity("pptx_xml"));
    }
    if lower_name.ends_with(".txt") || lower_name.ends_with(".md") {
        return Dispatch::PlainText(parser_identity(if lower_name.ends_with(".md") {
            "markdown"
        } else {
            "plain_text"
        }));
    }
    Dispatch::Unsupported(parser_identity("unsupported"))
}

fn extract_chunks(
    dispatch: &Dispatch,
    bytes: &[u8],
) -> std::result::Result<Vec<ExtractedChunk>, &'static str> {
    let mut chunks = Vec::new();
    let mut total_bytes = 0;
    match dispatch {
        Dispatch::PlainText(_) => append_text_chunks(
            &mut chunks,
            &mut total_bytes,
            String::from_utf8_lossy(bytes).as_ref(),
            json!({"block": 0}),
        )?,
        Dispatch::Pdf(_) => extract_pdf_chunks(bytes, &mut chunks, &mut total_bytes)?,
        Dispatch::Docx(_) => extract_ooxml_chunks(
            bytes,
            "word/document.xml",
            "paragraph",
            &mut chunks,
            &mut total_bytes,
        )?,
        Dispatch::Xlsx(_) => extract_ooxml_workbook_chunks(bytes, &mut chunks, &mut total_bytes)?,
        Dispatch::Pptx(_) => extract_ooxml_slides(bytes, &mut chunks, &mut total_bytes)?,
        Dispatch::Unsupported(_) => {}
    }
    Ok(chunks)
}

fn coarse_parser_error_code(code: &str) -> &'static str {
    match code {
        "parser_limit" => DerivedErrorCode::AssetParserLimit.as_str(),
        _ => DerivedErrorCode::AssetParserFailed.as_str(),
    }
}

async fn process_asset_async(
    name: String,
    media_type: String,
    bytes: Vec<u8>,
) -> std::result::Result<(String, ParserIdentity, bool, Vec<ExtractedChunk>), &'static str> {
    // Dispatch, hashing, plain-text normalization, and structured extraction
    // all run behind the same bounded blocking budget. A large TXT asset must
    // not bypass the semaphore merely because it does not need an Office/PDF
    // parser.
    static PARSER_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let semaphore = PARSER_SEMAPHORE
        .get_or_init(|| Arc::new(Semaphore::new(4)))
        .clone();
    let _permit = semaphore
        .acquire_owned()
        .await
        .map_err(|_| "parser_failed")?;
    tokio::task::spawn_blocking(move || {
        let actual_sha = hex::encode(Sha256::digest(&bytes));
        let dispatch = detect_dispatch(&name, &media_type, &bytes);
        let parser = dispatch.parser().clone();
        let unsupported = matches!(dispatch, Dispatch::Unsupported(_));
        let mut chunks = extract_chunks(&dispatch, &bytes)?;
        for chunk in &mut chunks {
            chunk.text = normalize_text(&chunk.text);
        }
        Ok((actual_sha, parser, unsupported, chunks))
    })
    .await
    .map_err(|_| "parser_failed")?
}

fn append_extracted_chunk(
    output: &mut Vec<ExtractedChunk>,
    total_bytes: &mut usize,
    chunk: ExtractedChunk,
) -> std::result::Result<(), &'static str> {
    let next_total = total_bytes
        .checked_add(chunk.text.len())
        .ok_or("parser_limit")?;
    if next_total > MAX_EXTRACTED_TEXT_BYTES {
        return Err("parser_limit");
    }
    *total_bytes = next_total;
    output.push(chunk);
    Ok(())
}

fn append_text_chunks(
    output: &mut Vec<ExtractedChunk>,
    total_bytes: &mut usize,
    text: &str,
    locator: Value,
) -> std::result::Result<(), &'static str> {
    let normalized = normalize_text(text);
    let mut blocks = normalized
        .split("\n\n")
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>();
    if blocks.is_empty() && !normalized.is_empty() {
        blocks.push(&normalized);
    }
    for block in blocks {
        let mut current = String::new();
        let mut current_chars = 0usize;
        for character in block.chars() {
            current.push(character);
            current_chars += 1;
            if current_chars >= MAX_TEXT_CHUNK_CHARS {
                append_extracted_chunk(
                    output,
                    total_bytes,
                    ExtractedChunk {
                        locator: locator.clone(),
                        text: std::mem::take(&mut current),
                    },
                )?;
                current_chars = 0;
            }
        }
        if !current.is_empty() {
            append_extracted_chunk(
                output,
                total_bytes,
                ExtractedChunk {
                    locator: locator.clone(),
                    text: current,
                },
            )?;
        }
    }
    Ok(())
}

fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|character| *character == '\n' || *character == '\t' || !character.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
fn extract_pdf_text(bytes: &[u8]) -> String {
    extract_pdf_text_bounded(bytes, MAX_EXTRACTED_TEXT_BYTES).unwrap_or_default()
}

fn extract_pdf_text_bounded(
    bytes: &[u8],
    max_bytes: usize,
) -> std::result::Result<String, &'static str> {
    let mut text = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'(' {
            index += 1;
            continue;
        }
        index += 1;
        let mut value = Vec::new();
        let mut depth = 1usize;
        while index < bytes.len() && depth > 0 {
            match bytes[index] {
                b'\\' if index + 1 < bytes.len() => {
                    index += 1;
                    value.push(match bytes[index] {
                        b'n' => b'\n',
                        b'r' => b'\r',
                        b't' => b'\t',
                        other => other,
                    });
                }
                b'(' => {
                    depth += 1;
                    value.push(b'(');
                }
                b')' => {
                    depth -= 1;
                    if depth > 0 {
                        value.push(b')');
                    }
                }
                byte => value.push(byte),
            }
            if value.len() > max_bytes {
                return Err("parser_limit");
            }
            index += 1;
        }
        if let Ok(value) = String::from_utf8(value) {
            if text.len().saturating_add(value.len()) > max_bytes {
                return Err("parser_limit");
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&value);
        }
    }
    Ok(text)
}

fn bounded_flate_decode(
    input: &[u8],
    max_bytes: usize,
) -> std::result::Result<Vec<u8>, &'static str> {
    fn read_bounded<R: Read>(
        reader: R,
        max_bytes: usize,
    ) -> std::result::Result<Vec<u8>, &'static str> {
        let mut output = Vec::new();
        reader
            .take(max_bytes.saturating_add(1) as u64)
            .read_to_end(&mut output)
            .map_err(|_| "malformed_pdf")?;
        if output.len() > max_bytes {
            return Err("parser_limit");
        }
        Ok(output)
    }

    match read_bounded(ZlibDecoder::new(input), max_bytes) {
        Ok(output) => Ok(output),
        Err("parser_limit") => Err("parser_limit"),
        Err(_) => read_bounded(DeflateDecoder::new(input), max_bytes),
    }
}

fn extract_pdf_stream_text_bounded(
    bytes: &[u8],
    max_bytes: usize,
    text_operators: &mut usize,
) -> std::result::Result<String, &'static str> {
    if !pdf_object_count_within_limit(bytes, MAX_PDF_OBJECTS) {
        return Err("parser_limit");
    }
    let mut decoded_bytes = 0usize;
    let mut text = String::new();
    let mut stream_count = 0usize;
    let mut offset = 0usize;
    while let Some(relative) = bytes[offset..]
        .windows(b"stream".len())
        .position(|window| window == b"stream")
    {
        let stream_start = offset + relative;
        stream_count = stream_count.checked_add(1).ok_or("parser_limit")?;
        if stream_count > MAX_PDF_OBJECTS {
            return Err("parser_limit");
        }
        let mut data_start = stream_start + b"stream".len();
        if bytes.get(data_start..data_start + 2) == Some(b"\r\n") {
            data_start += 2;
        } else if bytes.get(data_start..data_start + 1) == Some(b"\n") {
            data_start += 1;
        }
        let Some(relative_end) = bytes[data_start..]
            .windows(b"endstream".len())
            .position(|window| window == b"endstream")
        else {
            return Err("malformed_pdf");
        };
        let stream_end = data_start + relative_end;
        let dictionary_start = bytes[..stream_start]
            .windows(2)
            .rposition(|window| window == b"<<")
            .unwrap_or(stream_start);
        let dictionary = &bytes[dictionary_start..stream_start];
        let has_filter = dictionary
            .windows(b"/Filter".len())
            .any(|window| window == b"/Filter");
        let has_flate = dictionary
            .windows(b"/FlateDecode".len())
            .any(|window| window == b"/FlateDecode");
        if has_filter && !has_flate {
            return Err("malformed_pdf");
        }
        if !has_flate {
            offset = stream_end + b"endstream".len();
            continue;
        }
        let remaining = max_bytes.saturating_sub(decoded_bytes);
        let decoded = bounded_flate_decode(&bytes[data_start..stream_end], remaining)?;
        decoded_bytes = decoded_bytes
            .checked_add(decoded.len())
            .ok_or("parser_limit")?;
        let stream_operators = decoded.windows(2).filter(|window| *window == b"Tj").count()
            + decoded.windows(2).filter(|window| *window == b"TJ").count();
        *text_operators = text_operators
            .checked_add(stream_operators)
            .ok_or("parser_limit")?;
        if *text_operators > MAX_PDF_TEXT_OPERATORS {
            return Err("parser_limit");
        }
        let remaining = max_bytes.saturating_sub(text.len());
        let stream_text = extract_pdf_text_bounded(&decoded, remaining)?;
        if !stream_text.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&stream_text);
        }
        offset = stream_end + b"endstream".len();
    }
    Ok(text)
}

fn pdf_object_count_within_limit(bytes: &[u8], maximum: usize) -> bool {
    let mut count = 0usize;
    let mut offset = 0usize;
    while let Some(relative) = bytes[offset..]
        .windows(b" obj".len())
        .position(|window| window == b" obj")
    {
        let object_token = offset + relative;
        let mut cursor = object_token;
        while cursor > 0 && bytes[cursor - 1].is_ascii_whitespace() {
            cursor -= 1;
        }
        let generation_end = cursor;
        while cursor > 0 && bytes[cursor - 1].is_ascii_digit() {
            cursor -= 1;
        }
        if cursor == generation_end {
            offset = object_token + b" obj".len();
            continue;
        }
        while cursor > 0 && bytes[cursor - 1].is_ascii_whitespace() {
            cursor -= 1;
        }
        let object_number_end = cursor;
        while cursor > 0 && bytes[cursor - 1].is_ascii_digit() {
            cursor -= 1;
        }
        if cursor == object_number_end || (cursor > 0 && !bytes[cursor - 1].is_ascii_whitespace()) {
            offset = object_token + b" obj".len();
            continue;
        }
        count += 1;
        if count > maximum {
            return false;
        }
        offset = object_token + b" obj".len();
    }
    true
}

fn extract_pdf_chunks(
    bytes: &[u8],
    chunks: &mut Vec<ExtractedChunk>,
    total_bytes: &mut usize,
) -> std::result::Result<(), &'static str> {
    // Use a bounded literal text-layer scanner plus bounded Flate decoding.
    // General PDF extraction APIs commonly materialize a complete decoded
    // stream before the caller can enforce its output quota; that is not safe
    // for an untrusted Asset. Unsupported filters are a coarse parser failure
    // and never publish a partial build.
    let page_markers = bytes
        .windows(b"/Type /Page".len() + 1)
        .filter(|window| window.starts_with(b"/Type /Page") && window[b"/Type /Page".len()] != b's')
        .count();
    let mut text_operators = bytes.windows(2).filter(|window| *window == b"Tj").count()
        + bytes.windows(2).filter(|window| *window == b"TJ").count();
    if page_markers > MAX_PDF_PAGES || text_operators > MAX_PDF_TEXT_OPERATORS {
        return Err("parser_limit");
    }
    let mut text = extract_pdf_text_bounded(bytes, MAX_EXTRACTED_TEXT_BYTES).map_err(|error| {
        if error == "parser_limit" {
            error
        } else {
            "malformed_pdf"
        }
    })?;
    if bytes
        .windows(b"stream".len())
        .any(|window| window == b"stream")
    {
        let stream_text = extract_pdf_stream_text_bounded(
            bytes,
            MAX_EXTRACTED_TEXT_BYTES.saturating_sub(text.len()),
            &mut text_operators,
        )?;
        if !stream_text.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&stream_text);
        }
    }
    if text.is_empty() {
        return Ok(());
    }
    append_text_chunks(chunks, total_bytes, &text, json!({"page": 1}))?;
    Ok(())
}

/// Reject an excessive ZIP central-directory count before `ZipArchive::new`.
/// The archive constructor parses and allocates the directory, so checking
/// `archive.len()` after construction is too late for untrusted Asset bytes.
fn validate_zip_entry_count(bytes: &[u8]) -> std::result::Result<(), &'static str> {
    let Some(eocd) = bytes.windows(4).rposition(|window| window == b"PK\x05\x06") else {
        return Err("malformed_container");
    };
    if bytes.len().saturating_sub(eocd) < 22 {
        return Err("malformed_container");
    }
    let count = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]);
    if count != u16::MAX {
        return if usize::from(count) <= MAX_ZIP_ENTRIES {
            Ok(())
        } else {
            Err("parser_limit")
        };
    }
    let Some(locator) = eocd.checked_sub(20) else {
        return Err("malformed_container");
    };
    if bytes.get(locator..locator + 4) != Some(b"PK\x06\x07") {
        return Err("malformed_container");
    }
    let zip64_offset = u64::from_le_bytes(
        bytes[locator + 8..locator + 16]
            .try_into()
            .map_err(|_| "malformed_container")?,
    );
    let zip64_offset = usize::try_from(zip64_offset).map_err(|_| "parser_limit")?;
    if bytes.get(zip64_offset..zip64_offset + 40).is_none()
        || bytes.get(zip64_offset..zip64_offset + 4) != Some(b"PK\x06\x06")
    {
        return Err("malformed_container");
    }
    let total_entries = u64::from_le_bytes(
        bytes[zip64_offset + 32..zip64_offset + 40]
            .try_into()
            .map_err(|_| "malformed_container")?,
    );
    if total_entries > MAX_ZIP_ENTRIES as u64 {
        Err("parser_limit")
    } else {
        Ok(())
    }
}

fn extract_ooxml_chunks(
    bytes: &[u8],
    target: &str,
    kind: &str,
    chunks: &mut Vec<ExtractedChunk>,
    total_bytes: &mut usize,
) -> std::result::Result<(), &'static str> {
    validate_zip_entry_count(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "malformed_container")?;
    validate_zip_limits(&mut archive)?;
    let mut file = archive.by_name(target).map_err(|_| "malformed_container")?;
    let xml = read_zip_entry(&mut file)?;
    let text = xml_text(&xml)?;
    append_text_chunks(chunks, total_bytes, &text, json!({kind: 1}))
}

fn extract_ooxml_slides(
    bytes: &[u8],
    chunks: &mut Vec<ExtractedChunk>,
    total_bytes: &mut usize,
) -> std::result::Result<(), &'static str> {
    validate_zip_entry_count(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "malformed_container")?;
    validate_zip_limits(&mut archive)?;
    let mut slide_index = 0usize;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| "malformed_container")?;
        let name = file.name().to_string();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            slide_index += 1;
            let xml = read_zip_entry(&mut file)?;
            append_text_chunks(
                chunks,
                total_bytes,
                &xml_text(&xml)?,
                json!({"slide": slide_index}),
            )?;
        }
    }
    if chunks.is_empty() {
        return Err("malformed_container");
    }
    Ok(())
}

fn extract_ooxml_workbook_chunks(
    bytes: &[u8],
    chunks: &mut Vec<ExtractedChunk>,
    total_bytes: &mut usize,
) -> std::result::Result<(), &'static str> {
    validate_zip_entry_count(bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "malformed_container")?;
    validate_zip_limits(&mut archive)?;
    let mut sheet_index = 0usize;
    if let Ok(mut shared_strings) = archive.by_name("xl/sharedStrings.xml") {
        let xml = read_zip_entry(&mut shared_strings)?;
        append_text_chunks(
            chunks,
            total_bytes,
            &xml_text(&xml)?,
            json!({"sheet": "shared_strings"}),
        )?;
    }
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| "malformed_container")?;
        let name = file.name().to_string();
        if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
            sheet_index += 1;
            let xml = read_zip_entry(&mut file)?;
            append_text_chunks(
                chunks,
                total_bytes,
                &xml_text(&xml)?,
                json!({"sheet": sheet_index}),
            )?;
        }
    }
    if chunks.is_empty() {
        return Err("malformed_container");
    }
    Ok(())
}

fn read_zip_entry<R: Read>(
    file: &mut zip::read::ZipFile<'_, R>,
) -> std::result::Result<Vec<u8>, &'static str> {
    if file.size() > MAX_ZIP_EXPANDED_BYTES {
        return Err("parser_limit");
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|_| "malformed_container")?;
    Ok(bytes)
}

fn validate_zip_limits<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
) -> std::result::Result<(), &'static str> {
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err("parser_limit");
    }
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(|_| "malformed_container")?;
        expanded = expanded.saturating_add(file.size());
        if expanded > MAX_ZIP_EXPANDED_BYTES {
            return Err("parser_limit");
        }
    }
    Ok(())
}

fn xml_text(bytes: &[u8]) -> std::result::Result<String, &'static str> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut output = String::new();
    let mut depth = 0usize;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(_)) => {
                depth = depth.saturating_add(1);
                if depth > MAX_XML_DEPTH {
                    return Err("parser_limit");
                }
            }
            Ok(Event::End(_)) => {
                depth = depth.checked_sub(1).ok_or("malformed_xml")?;
            }
            Ok(Event::Text(text)) => {
                let value = text.decode().map_err(|_| "malformed_xml")?;
                if !value.is_empty() {
                    if !output.is_empty() {
                        output.push(' ');
                    }
                    output.push_str(&value);
                    if output.len() > MAX_EXTRACTED_TEXT_BYTES {
                        return Err("parser_limit");
                    }
                }
            }
            Ok(Event::CData(text)) => {
                if !output.is_empty() {
                    output.push(' ');
                }
                output.push_str(&String::from_utf8_lossy(&text));
                if output.len() > MAX_EXTRACTED_TEXT_BYTES {
                    return Err("parser_limit");
                }
            }
            Ok(_) => {}
            Err(_) => return Err("malformed_xml"),
        }
        buffer.clear();
    }
    if depth != 0 {
        return Err("malformed_xml");
    }
    Ok(output)
}

pub async fn register_asset_text_table(
    context: &SessionContext,
    op: &Operator,
    ws_path: &str,
    table_name: &str,
) -> Result<bool> {
    let store = SpaceCatalogStore::new(op.clone(), ws_path)?.single_process();
    let head_store =
        DerivedRelationHeadStore::new(op.clone(), ws_path, DerivedRelationId::ASSET_TEXT.as_uuid())
            .single_process();
    let Some(head) = head_store.read_exact().await? else {
        return Ok(false);
    };
    let current_source_coordinate = authoritative_source_coordinate(op, ws_path).await?;
    if head.head.source_coordinate != current_source_coordinate {
        return Ok(false);
    }
    if head.head.producer_fingerprint != asset_text_producer_fingerprint()
        || head.head.definition_fingerprint != asset_text_definition_fingerprint()
        || head.head.compatibility_epoch != ASSET_TEXT_COMPATIBILITY_EPOCH
    {
        return Ok(false);
    }
    let file_io = crate::space_catalog::file_io_for_store(&store);
    let metadata =
        iceberg::spec::TableMetadata::read_from(&file_io, &head.head.metadata_location).await?;
    let table_ident: TableIdent = serde_json::from_value(head.head.table_identifier.clone())?;
    let table = Table::builder()
        .identifier(table_ident)
        .metadata(metadata)
        .metadata_location(head.head.metadata_location.clone())
        .file_io(file_io)
        .runtime(Runtime::current())
        .build()?;
    let provider = IcebergStaticTableProvider::try_new_from_table(table).await?;
    context.register_table(table_name, Arc::new(provider))?;
    Ok(true)
}

pub async fn asset_text_search_matches(
    op: &Operator,
    ws_path: &str,
    query: &str,
) -> Result<Option<HashSet<String>>> {
    if query.len() > MAX_ASSET_TEXT_QUERY_BYTES {
        bail!("AssetText search query exceeds its byte limit");
    }
    let context =
        crate::query_context::bounded_session_context(&ugoite_core::query::QueryLimits {
            max_memory_bytes: 64 * 1024 * 1024,
            max_rows: MAX_ASSET_TEXT_MATCHES.saturating_add(1),
            timeout: Duration::from_secs(30),
            max_concurrency: 1,
            allowed_functions: BTreeSet::from(["lower".to_string()]),
        })?;
    if !register_asset_text_table(&context, op, ws_path, "__ugoite_internal_asset_text").await? {
        return Ok(None);
    }
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''");
    let sql = format!(
        "SELECT asset_id FROM __ugoite_internal_asset_text WHERE status = 'ready' AND text IS NOT NULL AND lower(text) LIKE lower('%{escaped}%') ESCAPE '\\' LIMIT {}",
        MAX_ASSET_TEXT_MATCHES.saturating_add(1)
    );
    let mut stream = context.sql(&sql).await?.execute_stream().await?;
    let mut matches = HashSet::new();
    let mut matched_bytes = 0usize;
    let mut scanned_rows = 0usize;
    tokio::time::timeout(Duration::from_secs(30), async {
        while let Some(batch) = stream.try_next().await? {
            scanned_rows = scanned_rows
                .checked_add(batch.num_rows())
                .context("AssetText search row count overflow")?;
            if scanned_rows > MAX_ASSET_TEXT_MATCHES {
                bail!("AssetText search exceeds its matching-row limit");
            }
            if batch.get_array_memory_size() > 64 * 1024 * 1024 {
                bail!("AssetText search batch exceeds its memory limit");
            }
            let values = batch
                .column_by_name("asset_id")
                .context("AssetText provider omitted asset_id")?;
            let values = values
                .as_any()
                .downcast_ref::<StringArray>()
                .context("AssetText asset_id has invalid type")?;
            for index in 0..values.len() {
                if !values.is_null(index) {
                    let asset_id = values.value(index);
                    if !matches.contains(asset_id) && matches.len() >= MAX_ASSET_TEXT_MATCHES {
                        bail!("AssetText search exceeds its matching-asset limit");
                    }
                    if matches.contains(asset_id) {
                        continue;
                    }
                    matched_bytes = matched_bytes
                        .checked_add(asset_id.len())
                        .context("AssetText search matched-ID byte count overflow")?;
                    if matched_bytes > MAX_ASSET_TEXT_MATCH_BYTES {
                        bail!("AssetText search exceeds its matched-ID byte limit");
                    }
                    matches.insert(asset_id.to_string());
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow!("AssetText search timed out"))??;
    Ok(Some(matches))
}

pub async fn asset_text_stats(op: &Operator, ws_path: &str) -> Result<Value> {
    let head_store =
        DerivedRelationHeadStore::new(op.clone(), ws_path, DerivedRelationId::ASSET_TEXT.as_uuid())
            .single_process();
    let Some(head) = head_store.read_exact().await? else {
        return Ok(json!({"state":"missing","stale":true}));
    };
    let manifest_location = format!(
        "{}/manifest.json",
        head_store.builds_path(&head.head.build_id)
    );
    let manifest: AssetTextManifest =
        serde_json::from_slice(&op.read(&manifest_location).await?.to_vec())
            .context("decode AssetText build manifest")?;
    let current_source_coordinate = authoritative_source_coordinate(op, ws_path).await?;
    let stale = head.head.producer_fingerprint != asset_text_producer_fingerprint()
        || head.head.definition_fingerprint != asset_text_definition_fingerprint()
        || head.head.compatibility_epoch != ASSET_TEXT_COMPATIBILITY_EPOCH
        || head.head.source_coordinate != current_source_coordinate;
    let refresh_requested = asset_text_refresh_requested(op, ws_path).await?;
    Ok(json!({
        "state": "ready",
        "current_producer_fingerprint": asset_text_producer_fingerprint(),
        "materialized_producer_fingerprint": head.head.producer_fingerprint,
        "compatibility_epoch": head.head.compatibility_epoch,
        "stale": stale,
        "refresh_requested": refresh_requested,
        "build_id": head.head.build_id,
        "generation": head.head.generation,
        "assets_referenced": manifest.assets_referenced,
        "assets_ready": manifest.assets_ready,
        "assets_empty": manifest.assets_empty,
        "assets_failed": manifest.assets_failed,
        "assets_unsupported": manifest.assets_unsupported,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_asset_read_is_classified_as_parser_limit() -> anyhow::Result<()> {
        let operator = opendal::Operator::new(opendal::services::Memory::default())?;
        operator
            .write("spaces/parser-limit/assets/a", b"12345".to_vec())
            .await?;
        let error = read_asset_exact_with_limit(&operator, "spaces/parser-limit/assets/a", 4)
            .await
            .expect_err("reader must stop at its configured limit");
        assert!(error.downcast_ref::<AssetParserInputLimit>().is_some());
        Ok(())
    }

    #[tokio::test]
    async fn refresh_request_marker_survives_until_a_successful_clear() -> anyhow::Result<()> {
        let operator = opendal::Operator::new(opendal::services::Memory::default())?;
        let workspace = "spaces/refresh-marker";

        assert!(!asset_text_refresh_requested(&operator, workspace).await?);
        mark_asset_text_refresh_requested(&operator, workspace).await?;
        assert!(asset_text_refresh_requested(&operator, workspace).await?);
        clear_asset_text_refresh_requested(&operator, workspace).await?;
        assert!(!asset_text_refresh_requested(&operator, workspace).await?);
        Ok(())
    }

    #[tokio::test]
    async fn finalization_cannot_clear_a_newer_refresh_request() -> anyhow::Result<()> {
        let operator = opendal::Operator::new(opendal::services::Memory::default())?;
        let workspace = "spaces/refresh-marker-race";

        let first = mark_asset_text_refresh_requested(&operator, workspace).await?;
        let first_path = asset_text_refresh_request_path(workspace, &first);
        let _second = mark_asset_text_refresh_requested(&operator, workspace).await?;

        // A build that started with only the first marker may acknowledge only
        // that immutable token. The newer request must remain for the worker
        // or startup rearm to observe.
        clear_asset_text_refresh_request_paths(&operator, &[first_path]).await?;
        assert!(asset_text_refresh_requested(&operator, workspace).await?);

        clear_asset_text_refresh_requested(&operator, workspace).await?;
        assert!(!asset_text_refresh_requested(&operator, workspace).await?);
        Ok(())
    }

    #[tokio::test]
    async fn refresh_marker_overflow_drains_in_batches_and_preserves_newer_requests(
    ) -> anyhow::Result<()> {
        let operator = opendal::Operator::new(opendal::services::Memory::default())?;
        let workspace = "spaces/refresh-marker-overflow";

        for _ in 0..MAX_ASSET_TEXT_REFRESH_REQUESTS {
            let token = Uuid::now_v7().to_string();
            operator
                .write(
                    &asset_text_refresh_request_path(workspace, &token),
                    b"{}".to_vec(),
                )
                .await?;
        }
        assert!(mark_asset_text_refresh_requested(&operator, workspace)
            .await
            .is_err());

        let cutoff = Uuid::now_v7().to_string();
        let newer = Uuid::now_v7().to_string();
        operator
            .write(
                &asset_text_refresh_request_path(workspace, &newer),
                b"{}".to_vec(),
            )
            .await?;
        clear_asset_text_refresh_requests_through(&operator, workspace, &cutoff).await?;
        assert!(asset_text_refresh_requested(&operator, workspace).await?);
        clear_asset_text_refresh_requested(&operator, workspace).await?;
        assert!(!asset_text_refresh_requested(&operator, workspace).await?);
        Ok(())
    }

    #[tokio::test]
    async fn invalid_legacy_asset_reference_cannot_escape_space_storage() -> anyhow::Result<()> {
        let operator = opendal::Operator::new(opendal::services::Memory::default())?;
        let outside_path = "spaces/other/assets/sentinel";
        operator
            .write(outside_path, b"must not be read".to_vec())
            .await?;
        let reference = SourceReference {
            asset_id: "../other/assets/sentinel".to_string(),
            name: "sentinel.txt".to_string(),
            media_type: "text/plain".to_string(),
            source_sha256: "a".repeat(64),
            source_size_bytes: 16,
            integrity_error: None,
        };

        let error = build_asset_text_rows(&operator, "spaces/demo", &[reference], "test-producer")
            .await
            .expect_err("invalid legacy asset IDs must fail closed before object reads");
        assert!(error
            .to_string()
            .contains("invalid AssetReference asset_id"));
        assert!(operator.exists(outside_path).await?);
        Ok(())
    }

    #[tokio::test]
    async fn empty_space_has_no_durable_refresh_need_without_a_catalog_head() -> anyhow::Result<()>
    {
        let operator = opendal::Operator::new(opendal::services::Memory::default())?;
        assert!(!asset_text_refresh_needed(&operator, "spaces/empty").await?);
        Ok(())
    }

    #[test]
    fn asset_text_schema_and_definition_share_stable_fields() {
        let definition = asset_text_definition();
        assert_eq!(
            definition.schema.fields.len(),
            asset_text_schema().as_struct().fields().len()
        );
        assert_eq!(
            definition.logical_key.last().map(String::as_str),
            Some("chunk_index")
        );
        assert_eq!(
            definition
                .schema
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "asset_id",
                "source_sha256",
                "source_size_bytes",
                "parser_id",
                "parser_version",
                "producer_fingerprint",
                "status",
                "chunk_index",
                "source_locator",
                "text",
                "text_length",
                "parsed_at",
                "error_code",
            ]
        );
    }

    #[test]
    fn text_normalization_is_deterministic_and_unicode_aware() {
        assert_eq!(normalize_text("a\r\nb\r\n\u{0000}c"), "a\nb\nc");
        assert_eq!("設備投資".chars().count(), 4);
    }

    #[test]
    fn pdf_text_layer_literals_are_searchable_without_executing_content() {
        assert_eq!(
            extract_pdf_text("BT (設備投資) Tj ET".as_bytes()),
            "設備投資"
        );
    }

    #[test]
    fn pdf_text_layer_parser_handles_page_content_streams() {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0usize];
        {
            let mut append = |object: &str| {
                offsets.push(pdf.len());
                pdf.extend_from_slice(object.as_bytes());
            };
            append("1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
            append("2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
            append("3 0 obj\n<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 5 0 R >> >> /MediaBox [0 0 612 792] /Contents 4 0 R >>\nendobj\n");
            let stream = "BT /F1 12 Tf 72 720 Td (Investment) Tj ET\n";
            append(&format!(
                "4 0 obj\n<< /Length {} >>\nstream\n{}endstream\nendobj\n",
                stream.len(),
                stream
            ));
            append("5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");
        }
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );
        let chunks = extract_chunks(&Dispatch::Pdf(parser_identity("pdf")), &pdf)
            .expect("valid PDF fixture");
        assert!(chunks.iter().any(|chunk| chunk.text.contains("Investment")));
        assert_eq!(chunks[0].locator, json!({"page": 1}));
    }

    #[test]
    fn pdf_decoded_stream_limit_is_enforced_before_expansion() {
        use std::io::Write;

        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder
            .write_all(&vec![b'x'; MAX_EXTRACTED_TEXT_BYTES + 1])
            .expect("compress hostile stream");
        let compressed = encoder.finish().expect("finish hostile stream");
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let offset = pdf.len();
        pdf.extend_from_slice(
            format!(
                "1 0 obj\n<< /Length {} /Filter /FlateDecode >>\nstream\n",
                compressed.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&compressed);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 2\n0000000000 65535 f \n");
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );

        assert!(matches!(
            extract_chunks(&Dispatch::Pdf(parser_identity("pdf")), &pdf),
            Err("parser_limit")
        ));
    }

    #[test]
    fn pdf_decoded_stream_operator_limit_is_enforced_after_decompression() {
        use std::io::Write;

        let content = b"() Tj ".repeat(MAX_PDF_TEXT_OPERATORS + 1);
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&content).expect("compress operators");
        let compressed = encoder.finish().expect("finish operators");
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let offset = pdf.len();
        pdf.extend_from_slice(
            format!(
                "1 0 obj\n<< /Length {} /Filter /FlateDecode >>\nstream\n",
                compressed.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&compressed);
        pdf.extend_from_slice(b"\nendstream\nendobj\n");
        let xref = pdf.len();
        pdf.extend_from_slice(b"xref\n0 2\n0000000000 65535 f \n");
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
        );

        assert!(matches!(
            extract_chunks(&Dispatch::Pdf(parser_identity("pdf")), &pdf),
            Err("parser_limit")
        ));
    }

    #[test]
    fn parser_limits_reject_hostile_pdf_and_xml_work() {
        let hostile_pdf = b"/Type /Page ".repeat(MAX_PDF_PAGES + 1);
        assert!(matches!(
            extract_chunks(&Dispatch::Pdf(parser_identity("pdf")), &hostile_pdf),
            Err("parser_limit")
        ));

        let mut deeply_nested = String::new();
        for index in 0..=MAX_XML_DEPTH {
            deeply_nested.push_str(&format!("<n{index}>"));
        }
        deeply_nested.push_str("text");
        for index in (0..=MAX_XML_DEPTH).rev() {
            deeply_nested.push_str(&format!("</n{index}>"));
        }
        assert_eq!(xml_text(deeply_nested.as_bytes()), Err("parser_limit"));
    }

    #[test]
    fn pdf_object_limit_is_checked_before_stream_decode() {
        let bytes = b"1 0 obj\nendobj\n2 0 obj\nendobj\n";
        assert!(pdf_object_count_within_limit(bytes, 2));
        assert!(!pdf_object_count_within_limit(bytes, 1));
    }

    #[test]
    fn zip_entry_limit_is_checked_before_archive_materialization() {
        let mut bytes = vec![0; 22];
        bytes[..4].copy_from_slice(b"PK\x05\x06");
        bytes[10..12].copy_from_slice(&u16::try_from(MAX_ZIP_ENTRIES + 1).unwrap().to_le_bytes());
        assert_eq!(validate_zip_entry_count(&bytes), Err("parser_limit"));
        assert!(matches!(
            extract_chunks(&Dispatch::Docx(parser_identity("docx")), &bytes),
            Err("parser_limit")
        ));
    }

    #[test]
    fn extracted_text_limit_is_shared_by_plain_text_dispatch() {
        let bytes = vec![b'x'; MAX_EXTRACTED_TEXT_BYTES + 1];
        assert!(matches!(
            extract_chunks(&Dispatch::PlainText(parser_identity("plain_text")), &bytes),
            Err("parser_limit")
        ));
    }

    #[test]
    fn extracted_text_limit_is_enforced_while_accumulating_chunks() {
        let first = "x".repeat(MAX_EXTRACTED_TEXT_BYTES / 2);
        let second = "x".repeat(MAX_EXTRACTED_TEXT_BYTES / 2 + 1);
        let mut chunks = Vec::new();
        let mut total_bytes = 0;
        append_text_chunks(&mut chunks, &mut total_bytes, &first, json!({"part": 1}))
            .expect("first parser part fits");
        assert!(
            append_text_chunks(&mut chunks, &mut total_bytes, &second, json!({"part": 2}),)
                .is_err()
        );
        assert!(total_bytes <= MAX_EXTRACTED_TEXT_BYTES);
    }

    #[tokio::test]
    async fn empty_space_rebuild_publishes_only_a_derived_head() -> anyhow::Result<()> {
        let op = opendal::Operator::new(opendal::services::Memory::default())?;
        crate::space::create_space(&op, "derived-empty", "memory:///").await?;
        let catalog_head_path = "spaces/derived-empty/_ugoite/catalog/head.json";
        let before = if op.exists(catalog_head_path).await? {
            Some(op.read(catalog_head_path).await?.to_vec())
        } else {
            None
        };
        let head = rebuild_asset_text(&op, "spaces/derived-empty").await?;
        assert_eq!(head.relation_id, DerivedRelationId::ASSET_TEXT.to_string());
        assert_eq!(head.generation, 1);
        let after = if op.exists(catalog_head_path).await? {
            Some(op.read(catalog_head_path).await?.to_vec())
        } else {
            None
        };
        assert_eq!(before, after);
        Ok(())
    }

    #[tokio::test]
    async fn asset_text_search_rejects_an_oversized_query_before_planning() -> anyhow::Result<()> {
        let op = opendal::Operator::new(opendal::services::Memory::default())?;
        let error = asset_text_search_matches(
            &op,
            "spaces/missing",
            &"x".repeat(MAX_ASSET_TEXT_QUERY_BYTES + 1),
        )
        .await
        .expect_err("oversized query must fail before DataFusion planning");
        assert!(error.to_string().contains("query exceeds"));
        Ok(())
    }

    #[tokio::test]
    async fn text_asset_materializes_and_is_searchable() -> anyhow::Result<()> {
        let op = opendal::Operator::new(opendal::services::Memory::default())?;
        crate::space::create_space(&op, "derived-text", "memory:///").await?;
        let ws_path = "spaces/derived-text";
        crate::form::upsert_form(
            &op,
            ws_path,
            &json!({"name":"Notes","fields":{"Attachment":{"type":"asset_reference"}}}),
        )
        .await?;
        let reference =
            crate::asset::save_asset(&op, ws_path, "report.txt", "設備投資".as_bytes()).await?;
        let orphan =
            crate::asset::save_asset(&op, ws_path, "orphan.txt", "未参照秘密".as_bytes()).await?;
        let content = format!(
            "---\nform: Notes\nAttachment: {}\n---\n# August meeting",
            serde_json::to_string(&reference)?
        );
        crate::entry::create_entry(
            &op,
            ws_path,
            "meeting-1",
            &content,
            "author",
            &crate::integrity::FakeIntegrityProvider,
        )
        .await?;
        let catalog_head_path = format!("{ws_path}/_ugoite/catalog/head.json");
        let catalog_head_before = op.read(&catalog_head_path).await?.to_vec();
        let head = rebuild_asset_text(&op, ws_path).await?;
        assert!(head.snapshot_id.is_some());
        let manifest: AssetTextManifest = serde_json::from_slice(
            &op.read(&format!(
                "{ws_path}/_ugoite/derived/relations/{}/builds/{}/manifest.json",
                head.relation_id, head.build_id
            ))
            .await?
            .to_vec(),
        )?;
        assert_eq!(manifest.assets_referenced, 1);
        assert_eq!(
            op.read(&catalog_head_path).await?.to_vec(),
            catalog_head_before
        );
        let matches = asset_text_search_matches(&op, ws_path, "設備投資")
            .await?
            .expect("published AssetText relation");
        assert!(matches.contains(&reference.asset_id));
        let results = crate::search::search_entries(&op, ws_path, "設備投資", 10).await?;
        assert_eq!(
            results
                .iter()
                .map(|result| result.id.as_str())
                .collect::<Vec<_>>(),
            vec!["meeting-1"]
        );
        assert!(!asset_text_search_matches(&op, ws_path, "未参照秘密")
            .await?
            .expect("published AssetText relation")
            .contains(&orphan.asset_id));
        let scopes = std::collections::BTreeMap::from([(
            "notes".to_string(),
            ugoite_core::query::EntryScope::Only(std::collections::BTreeSet::new()),
        )]);
        assert!(
            crate::search::search_entries_with_scopes(&op, ws_path, "設備投資", &scopes, 10)
                .await?
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn same_asset_referenced_by_one_hundred_entries_is_one_source() {
        let mut references = BTreeMap::new();
        let mut checksums = BTreeMap::new();
        let mut conflicts = HashSet::new();
        for _ in 0..100 {
            merge_source_reference(
                &mut references,
                &mut checksums,
                &mut conflicts,
                SourceReference {
                    asset_id: "asset-1".into(),
                    name: "report.txt".into(),
                    media_type: "text/plain".into(),
                    source_sha256: "sha".into(),
                    source_size_bytes: 3,
                    integrity_error: None,
                },
            );
        }
        assert_eq!(references.len(), 1);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn duplicate_asset_reference_uses_mime_before_conflicting_filename() {
        let mut references = BTreeMap::new();
        let mut checksums = BTreeMap::new();
        let mut conflicts = HashSet::new();
        merge_source_reference(
            &mut references,
            &mut checksums,
            &mut conflicts,
            SourceReference {
                asset_id: "asset-1".into(),
                name: "z.txt".into(),
                media_type: "text/plain".into(),
                source_sha256: "sha".into(),
                source_size_bytes: 10,
                integrity_error: None,
            },
        );
        merge_source_reference(
            &mut references,
            &mut checksums,
            &mut conflicts,
            SourceReference {
                asset_id: "asset-1".into(),
                name: "a.pdf".into(),
                media_type: "text/plain".into(),
                source_sha256: "sha".into(),
                source_size_bytes: 10,
                integrity_error: None,
            },
        );
        let reference = references.get("asset-1").expect("merged Asset reference");
        let dispatch = detect_dispatch(&reference.name, &reference.media_type, b"plain text");
        assert!(matches!(dispatch, Dispatch::PlainText(_)));
    }

    #[test]
    fn duplicate_asset_reference_prefers_text_mime_over_structured_mime() {
        let mut references = BTreeMap::new();
        let mut checksums = BTreeMap::new();
        let mut conflicts = HashSet::new();
        merge_source_reference(
            &mut references,
            &mut checksums,
            &mut conflicts,
            SourceReference {
                asset_id: "asset-1".into(),
                name: "a.pdf".into(),
                media_type: "application/pdf".into(),
                source_sha256: "sha".into(),
                source_size_bytes: 10,
                integrity_error: None,
            },
        );
        merge_source_reference(
            &mut references,
            &mut checksums,
            &mut conflicts,
            SourceReference {
                asset_id: "asset-1".into(),
                name: "z.txt".into(),
                media_type: "text/plain".into(),
                source_sha256: "sha".into(),
                source_size_bytes: 10,
                integrity_error: None,
            },
        );
        let reference = references.get("asset-1").expect("merged Asset reference");
        assert_eq!(reference.media_type, "text/plain");
        assert!(matches!(
            detect_dispatch(&reference.name, &reference.media_type, b"plain text"),
            Dispatch::PlainText(_)
        ));
    }
}
