//! Rebuildable, non-authoritative Iceberg relations.
//!
//! The Relation Head in `ugoite-storage` is the only durable visibility
//! coordinate in this module.  Iceberg metadata and data files are immutable
//! build products below a build prefix; a failed build therefore
//! cannot replace the currently visible result or the authoritative Catalog.

use anyhow::{bail, Context, Result};
use arrow_array::builder::{Int64Builder, StringBuilder};
use arrow_array::{Array, RecordBatch, StringArray, TimestampMicrosecondArray};
use chrono::Utc;
use datafusion::prelude::SessionContext;
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
use opendal::options::WriteOptions;
use opendal::{ErrorKind, Operator};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read, Seek};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify, Semaphore};
use ugoite_domain::derived_relation::DerivedRelationId;
use ugoite_domain::derived_relation::{
    canonical_json, sha256_digest, DerivedErrorCode, DerivedExposure, DerivedRelationDefinition,
    DerivedValueType, RelationField, TypedSchema,
};
use ugoite_domain::entry::AssetReference;
use ugoite_domain::form::{FieldType, FormDefinition};
use ugoite_storage::{DerivedRelationHead, DerivedRelationHeadStore, SpaceCatalogStore};
use uuid::Uuid;
use zip::ZipArchive;

pub const ASSET_TEXT_PRODUCER_ID: &str = "ugoite.asset_text";
pub const ASSET_TEXT_PARSER_VERSION: &str = "2";
pub const ASSET_TEXT_COMPATIBILITY_EPOCH: u64 = 2;
const MAX_ASSET_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 10_000;
const MAX_ZIP_EXPANDED_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PDF_PAGES: usize = 10_000;
const MAX_PDF_TEXT_OPERATORS: usize = 1_000_000;
const MAX_XML_DEPTH: usize = 256;
const MAX_EXTRACTED_TEXT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TEXT_CHUNK_CHARS: usize = 16 * 1024;
const READER_CHUNK_BYTES: usize = 256 * 1024;
const MINIMUM_GC_AGE: Duration = Duration::from_secs(60 * 60);
const GC_RETRY_DELAY: Duration = Duration::from_secs(60);

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
        b"ugoite.asset_text/protocol=2;dispatch=text/plain,text/markdown,pdf,docx,xlsx,pptx;pdf=text-layer;normalization=line-endings+control-chars;chunk=semantic-boundary+16384-unicode-scalars;limits=64MiB-input+128MiB-zip+10000-pdf-pages+16MiB-text+256-xml-depth;blocking=bounded-4;schema=2",
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
    rebuild_asset_text_with_mode(op, ws_path, false).await
}

/// Shared backends use the exact-read/if-match path and deliberately do not
/// take the process-local rebuild mutex. A losing build remains an immutable
/// garbage candidate and is never published.
pub async fn rebuild_asset_text_shared(
    op: &Operator,
    ws_path: &str,
) -> Result<DerivedRelationHead> {
    rebuild_asset_text_with_mode(op, ws_path, true).await
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
    // v1 Heads point to the removed materializations layout. Derived state is
    // disposable, so local rebuilds can remove that Head before creating the
    // first current-build Head. Shared rebuilds retain the exact legacy
    // coordinate and replace it with a conditional Head swap after staging;
    // this is a format discard, not a legacy-read compatibility path.
    let mut legacy_expected = None;
    if let Err(error) = head_store.read_exact().await {
        if error
            .downcast_ref::<ugoite_storage::LegacyDerivedRelationHead>()
            .is_some()
        {
            if shared {
                legacy_expected = Some(
                    head_store
                        .read_legacy_exact()
                        .await?
                        .context("legacy DerivedRelation Head disappeared")?,
                );
            } else {
                head_store.invalidate_legacy_head().await?;
            }
        } else {
            return Err(error);
        }
    }
    let _rebuild_guard = if shared {
        None
    } else {
        Some(head_store.single_process_lock().lock_owned().await)
    };
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
    let input_digest = sha256_digest(&canonical_json(&source_rows)?);
    let rows = build_asset_text_rows(op, ws_path, &source_rows, &producer_fingerprint).await?;
    let row_digest = sha256_digest(&canonical_json(&rows)?);
    let build_id = Uuid::now_v7().to_string();
    let build_path = head_store.builds_path(&build_id);
    head_store.mark_staging(&build_id).await?;
    let heartbeat_store = head_store.clone();
    let heartbeat_build_id = build_id.clone();
    let staging_heartbeat = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let _ = heartbeat_store.renew_staging(&heartbeat_build_id).await;
        }
    });
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
        if !rows.is_empty() {
            append_rows(&table, &catalog, &rows).await?;
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
    let head = match build_result {
        Ok(head) => head,
        Err(error) => {
            let _ = ensure_cleanup_marker(&head_store, &build_id).await;
            schedule_asset_text_gc(op, ws_path);
            return Err(error);
        }
    };
    let publish_result = if shared {
        if let Some(legacy_expected) = legacy_expected.as_ref() {
            head_store.publish_over_legacy(legacy_expected, &head).await
        } else {
            head_store.publish(expected.as_ref(), &head).await
        }
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
                    let _ = head_store.remove_legacy_materializations().await;
                }
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
        // The new Head is now authoritative. The old v1 prefix is disposable
        // and must not remain a second, undiscoverable materialization tree.
        // Maintenance retries this cleanup after a crash or transient delete
        // failure.
        let _ = head_store.remove_legacy_materializations().await;
    }
    schedule_asset_text_gc(op, ws_path);
    let current = match head_store.read_exact().await {
        Ok(Some(current)) => current.head,
        Ok(None) => {
            return Err(anyhow::anyhow!("published derived Head disappeared"));
        }
        Err(error) => return Err(error),
    };
    let candidate_cleanup_marked = if current.build_id != head.build_id {
        // A shared writer may have won immediately after this writer's CAS.
        // The successful candidate is then garbage too; leaving only its
        // staging marker would make it invisible to lifecycle GC forever.
        ensure_cleanup_marker(&head_store, &head.build_id).await
    } else {
        true
    };
    // A completed build no longer needs its active-build heartbeat. If this
    // delete is interrupted, the conservative staging marker still keeps the
    // build protected until the grace period has elapsed.
    if candidate_cleanup_marked {
        let _ = head_store.clear_staging(&head.build_id).await;
    }
    let _ = head_store
        .garbage_collect_with_single_process_lock(Some(&current.build_id), MINIMUM_GC_AGE)
        .await;
    schedule_asset_text_gc(op, ws_path);
    Ok(current)
}

fn schedule_asset_text_gc(op: &Operator, ws_path: &str) {
    schedule_asset_text_gc_after_delay(op, ws_path, MINIMUM_GC_AGE);
}

fn schedule_asset_text_gc_after_delay(op: &Operator, ws_path: &str, delay: Duration) {
    let relation_id = asset_text_definition().relation_id.as_uuid();
    let key = format!(
        "{}:{}:{}:{}:{}",
        op.info().scheme(),
        op.info().name(),
        op.info().root(),
        ws_path,
        relation_id,
    );
    let schedulers = ASSET_TEXT_GC_SCHEDULERS.get_or_init(|| StdMutex::new(BTreeMap::new()));
    let scheduler = {
        let mut schedulers = schedulers
            .lock()
            .expect("AssetText GC scheduler map poisoned");
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
        if deadline.is_none_or(|current| next > current) {
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
                        let head_store = if matches!(
                            operator.info().scheme(),
                            "s3" | "gcs" | "oss" | "azdls"
                        ) {
                            base.shared().await.ok()
                        } else {
                            Some(base.single_process())
                        };
                        let retry_gc = if let Some(head_store) = head_store {
                            match head_store.read_exact().await {
                                Ok(current_build) => {
                                    let current_build_id =
                                        current_build.map(|head| head.head.build_id);
                                    match head_store
                                        .garbage_collect(
                                            current_build_id.as_deref(),
                                            MINIMUM_GC_AGE,
                                        )
                                        .await
                                    {
                                        Ok(_) => head_store
                                            .has_pending_garbage(
                                                current_build_id.as_deref(),
                                                MINIMUM_GC_AGE,
                                            )
                                            .await
                                            .unwrap_or(true),
                                        Err(_) => true,
                                    }
                                }
                                Err(_) => true,
                            }
                        } else {
                            true
                        };
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
            // immediately after a shared legacy-to-current swap.
            head_store.remove_legacy_materializations().await?;
            head.map(|head| head.head.build_id)
        }
        Err(error)
            if error
                .downcast_ref::<ugoite_storage::LegacyDerivedRelationHead>()
                .is_some() =>
        {
            // Never delete the v1 prefix while its legacy Head still points at
            // it. A later rebuild will replace that Head by exact CAS first.
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
    );
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
    let form_names = crate::entry::list_form_names(op, ws_path).await?;
    let mut definitions = BTreeMap::<String, FormDefinition>::new();
    for name in &form_names {
        let definition = crate::iceberg_store::load_domain_form(op, ws_path, name).await?;
        definitions.insert(name.clone(), definition);
    }
    let mut references = BTreeMap::<String, SourceReference>::new();
    let mut asset_checksums = BTreeMap::<String, (String, u64)>::new();
    let mut conflicting_assets = HashSet::new();
    for form_name in form_names {
        let definition = definitions
            .get(&form_name)
            .context("current Entry Form missing")?;
        // This canonical latest-revision view scans the authoritative Form
        // table directly and is not subject to the normal 10k search window.
        // Delete tombstones are deliberately excluded after max-version
        // selection, so deleted Entries cannot seed a derived source set.
        let revisions = workspace
            .read_current_revision_view_for_derived(definition.id)
            .await?;
        for revision in revisions {
            if matches!(
                revision.operation,
                ugoite_domain::entry::EntryOperation::Delete
            ) {
                continue;
            }
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
                for reference in asset_references {
                    let candidate = SourceReference {
                        asset_id: reference.asset_id,
                        name: reference.name,
                        media_type: reference.media_type,
                        source_sha256: reference.sha256,
                        source_size_bytes: reference.size_bytes,
                        integrity_error: None,
                    };
                    merge_source_reference(
                        &mut references,
                        &mut asset_checksums,
                        &mut conflicting_assets,
                        candidate,
                    );
                }
            }
        }
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

fn source_reference_metadata_rank(reference: &SourceReference) -> u8 {
    let name = reference.name.to_ascii_lowercase();
    let media_type = reference.media_type.to_ascii_lowercase();
    if media_type == "application/pdf"
        || media_type == "text/plain"
        || media_type == "text/markdown"
        || media_type == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        || media_type == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        || media_type == "application/vnd.openxmlformats-officedocument.presentationml.presentation"
        || [".pdf", ".txt", ".md", ".docx", ".xlsx", ".pptx"]
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

fn typed_asset_references_for_field(
    field: &ugoite_domain::form::FormField,
    value: &ugoite_domain::entry::FieldValue,
) -> Result<Vec<AssetReference>> {
    let encoded = serde_json::to_value(value)?;
    match (&field.field_type, field.list_item.as_ref(), encoded) {
        (_, _, Value::Null) => Ok(Vec::new()),
        (FieldType::AssetReference, _, value) => {
            Ok(vec![serde_json::from_value::<AssetReference>(value)?])
        }
        (FieldType::List, Some(item), Value::Array(values))
            if item.field_type == FieldType::AssetReference =>
        {
            values
                .into_iter()
                .filter(|value| !value.is_null())
                .map(serde_json::from_value::<AssetReference>)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Into::into)
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
    let mut rows = Vec::new();
    for reference in references {
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
        let actual_sha = hex::encode(Sha256::digest(&bytes));
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
        let dispatch = detect_dispatch(&reference.name, &reference.media_type, &bytes);
        let parser = dispatch.parser().clone();
        let chunks = match extract_chunks_async(dispatch.clone(), bytes.clone()).await {
            Ok(chunks) => chunks,
            Err(code) => {
                rows.push(base(
                    parser.id.into(),
                    parser.version.into(),
                    "failed",
                    0,
                    None,
                    None,
                    Some(coarse_parser_error_code(code)),
                ));
                continue;
            }
        };
        if matches!(dispatch, Dispatch::Unsupported(_)) {
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
                let text = normalize_text(&chunk.text);
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
            }
        }
    }
    Ok(rows)
}

async fn read_asset_exact(op: &Operator, path: &str) -> Result<Vec<u8>> {
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
        if bytes.len() > MAX_ASSET_BYTES as usize {
            bail!("asset parser input exceeds configured limit");
        }
    }
    Ok(bytes)
}

fn detect_dispatch(name: &str, media_type: &str, bytes: &[u8]) -> Dispatch {
    let lower_name = name.to_ascii_lowercase();
    let lower_media = media_type.to_ascii_lowercase();
    if lower_media == "application/pdf"
        || lower_name.ends_with(".pdf")
        || bytes.starts_with(b"%PDF-")
    {
        return Dispatch::Pdf(parser_identity("pdf_text_layer"));
    }
    if lower_name.ends_with(".docx")
        || lower_media == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    {
        return Dispatch::Docx(parser_identity("docx_xml"));
    }
    if lower_name.ends_with(".xlsx")
        || lower_media == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    {
        return Dispatch::Xlsx(parser_identity("xlsx_xml"));
    }
    if lower_name.ends_with(".pptx")
        || lower_media
            == "application/vnd.openxmlformats-officedocument.presentationml.presentation"
    {
        return Dispatch::Pptx(parser_identity("pptx_xml"));
    }
    // MIME and filename are only hints. Valid OOXML containers are also
    // recognized by their internal part names, while malformed recognized
    // containers remain on the format-specific parser path above.
    if bytes.starts_with(b"PK") {
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
    if lower_media == "text/plain"
        || lower_media == "text/markdown"
        || lower_name.ends_with(".txt")
        || lower_name.ends_with(".md")
    {
        return Dispatch::PlainText(parser_identity(
            if lower_media == "text/markdown" || lower_name.ends_with(".md") {
                "markdown"
            } else {
                "plain_text"
            },
        ));
    }
    Dispatch::Unsupported(parser_identity("unsupported"))
}

fn extract_chunks(
    dispatch: &Dispatch,
    bytes: &[u8],
) -> std::result::Result<Vec<ExtractedChunk>, &'static str> {
    let chunks = match dispatch {
        Dispatch::PlainText(_) => Ok(split_text_chunks(
            String::from_utf8_lossy(bytes).as_ref(),
            json!({"block": 0}),
        )),
        Dispatch::Pdf(_) => extract_pdf_chunks(bytes),
        Dispatch::Docx(_) => extract_ooxml_chunks(bytes, "word/document.xml", "paragraph"),
        Dispatch::Xlsx(_) => extract_ooxml_workbook_chunks(bytes),
        Dispatch::Pptx(_) => extract_ooxml_slides(bytes),
        Dispatch::Unsupported(_) => Ok(Vec::new()),
    }?;
    let total_bytes = chunks
        .iter()
        .try_fold(0usize, |total, chunk| total.checked_add(chunk.text.len()))
        .ok_or("parser_limit")?;
    if total_bytes > MAX_EXTRACTED_TEXT_BYTES {
        return Err("parser_limit");
    }
    Ok(chunks)
}

fn coarse_parser_error_code(code: &str) -> &'static str {
    match code {
        "parser_limit" => DerivedErrorCode::AssetParserLimit.as_str(),
        _ => DerivedErrorCode::AssetParserFailed.as_str(),
    }
}

async fn extract_chunks_async(
    dispatch: Dispatch,
    bytes: Vec<u8>,
) -> std::result::Result<Vec<ExtractedChunk>, &'static str> {
    if matches!(
        dispatch,
        Dispatch::Pdf(_) | Dispatch::Docx(_) | Dispatch::Xlsx(_) | Dispatch::Pptx(_)
    ) {
        static PARSER_SEMAPHORE: OnceLock<Arc<Semaphore>> = OnceLock::new();
        let semaphore = PARSER_SEMAPHORE
            .get_or_init(|| Arc::new(Semaphore::new(4)))
            .clone();
        let _permit = semaphore
            .acquire_owned()
            .await
            .map_err(|_| "parser_failed")?;
        tokio::task::spawn_blocking(move || extract_chunks(&dispatch, &bytes))
            .await
            .map_err(|_| "parser_failed")?
    } else {
        extract_chunks(&dispatch, &bytes)
    }
}

fn split_text_chunks(text: &str, locator: Value) -> Vec<ExtractedChunk> {
    let normalized = normalize_text(text);
    let mut blocks = normalized
        .split("\n\n")
        .filter(|block| !block.is_empty())
        .collect::<Vec<_>>();
    if blocks.is_empty() && !normalized.is_empty() {
        blocks.push(&normalized);
    }
    let mut chunks = Vec::new();
    for block in blocks {
        let mut current = String::new();
        let mut current_chars = 0usize;
        for character in block.chars() {
            current.push(character);
            current_chars += 1;
            if current_chars >= MAX_TEXT_CHUNK_CHARS {
                chunks.push(ExtractedChunk {
                    locator: locator.clone(),
                    text: std::mem::take(&mut current),
                });
                current_chars = 0;
            }
        }
        if !current.is_empty() {
            chunks.push(ExtractedChunk {
                locator: locator.clone(),
                text: current,
            });
        }
    }
    chunks
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
            index += 1;
        }
        if let Ok(value) = String::from_utf8(value) {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&value);
        }
    }
    text
}

fn extract_pdf_chunks(bytes: &[u8]) -> std::result::Result<Vec<ExtractedChunk>, &'static str> {
    // pdf-extract handles compressed content streams, text encodings, and
    // page boundaries.  Keep the call behind the explicit input/page limits
    // and convert parser failures to a coarse diagnostic; source bytes never
    // become a durable error payload.
    let page_markers = bytes
        .windows(b"/Type /Page".len() + 1)
        .filter(|window| window.starts_with(b"/Type /Page") && window[b"/Type /Page".len()] != b's')
        .count();
    let text_operators = bytes.windows(2).filter(|window| *window == b"Tj").count()
        + bytes.windows(2).filter(|window| *window == b"TJ").count();
    if page_markers > MAX_PDF_PAGES || text_operators > MAX_PDF_TEXT_OPERATORS {
        return Err("parser_limit");
    }
    let pages = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem_by_pages(bytes)
    }))
    .map_err(|_| "malformed_pdf")?
    .map_err(|_| "malformed_pdf")?;
    if pages.len() > MAX_PDF_PAGES {
        return Err("parser_limit");
    }
    let mut chunks = Vec::new();
    for (index, page) in pages.into_iter().enumerate() {
        chunks.extend(split_text_chunks(&page, json!({"page": index + 1})));
    }
    Ok(chunks)
}

fn extract_ooxml_chunks(
    bytes: &[u8],
    target: &str,
    kind: &str,
) -> std::result::Result<Vec<ExtractedChunk>, &'static str> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "malformed_container")?;
    validate_zip_limits(&mut archive)?;
    let mut file = archive.by_name(target).map_err(|_| "malformed_container")?;
    let xml = read_zip_entry(&mut file)?;
    let text = xml_text(&xml)?;
    Ok(split_text_chunks(&text, json!({kind: 1})))
}

fn extract_ooxml_slides(bytes: &[u8]) -> std::result::Result<Vec<ExtractedChunk>, &'static str> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "malformed_container")?;
    validate_zip_limits(&mut archive)?;
    let mut chunks = Vec::new();
    let mut slide_index = 0usize;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| "malformed_container")?;
        let name = file.name().to_string();
        if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
            slide_index += 1;
            let xml = read_zip_entry(&mut file)?;
            chunks.extend(split_text_chunks(
                &xml_text(&xml)?,
                json!({"slide": slide_index}),
            ));
        }
    }
    if chunks.is_empty() {
        return Err("malformed_container");
    }
    Ok(chunks)
}

fn extract_ooxml_workbook_chunks(
    bytes: &[u8],
) -> std::result::Result<Vec<ExtractedChunk>, &'static str> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| "malformed_container")?;
    validate_zip_limits(&mut archive)?;
    let mut chunks = Vec::new();
    let mut sheet_index = 0usize;
    if let Ok(mut shared_strings) = archive.by_name("xl/sharedStrings.xml") {
        let xml = read_zip_entry(&mut shared_strings)?;
        chunks.extend(split_text_chunks(
            &xml_text(&xml)?,
            json!({"sheet": "shared_strings"}),
        ));
    }
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|_| "malformed_container")?;
        let name = file.name().to_string();
        if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
            sheet_index += 1;
            let xml = read_zip_entry(&mut file)?;
            chunks.extend(split_text_chunks(
                &xml_text(&xml)?,
                json!({"sheet": sheet_index}),
            ));
        }
    }
    if chunks.is_empty() {
        return Err("malformed_container");
    }
    Ok(chunks)
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
    let context = SessionContext::new();
    if !register_asset_text_table(&context, op, ws_path, "__ugoite_internal_asset_text").await? {
        return Ok(None);
    }
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''");
    let sql = format!("SELECT asset_id FROM __ugoite_internal_asset_text WHERE status = 'ready' AND text IS NOT NULL AND lower(text) LIKE lower('%{escaped}%') ESCAPE '\\'");
    let batches = context.sql(&sql).await?.collect().await?;
    let mut matches = HashSet::new();
    for batch in batches {
        let values = batch
            .column_by_name("asset_id")
            .context("AssetText provider omitted asset_id")?;
        let values = values
            .as_any()
            .downcast_ref::<StringArray>()
            .context("AssetText asset_id has invalid type")?;
        for index in 0..values.len() {
            if !values.is_null(index) {
                matches.insert(values.value(index).to_string());
            }
        }
    }
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
    let stale = head.head.producer_fingerprint != asset_text_producer_fingerprint()
        || head.head.definition_fingerprint != asset_text_definition_fingerprint()
        || head.head.compatibility_epoch != ASSET_TEXT_COMPATIBILITY_EPOCH;
    Ok(json!({
        "state": "ready",
        "current_producer_fingerprint": asset_text_producer_fingerprint(),
        "materialized_producer_fingerprint": head.head.producer_fingerprint,
        "compatibility_epoch": head.head.compatibility_epoch,
        "stale": stale,
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
        let chunks = extract_pdf_chunks(&pdf).expect("valid PDF fixture");
        assert!(chunks.iter().any(|chunk| chunk.text.contains("Investment")));
        assert_eq!(chunks[0].locator, json!({"page": 1}));
    }

    #[test]
    fn parser_limits_reject_hostile_pdf_and_xml_work() {
        let hostile_pdf = b"/Type /Page ".repeat(MAX_PDF_PAGES + 1);
        assert!(matches!(
            extract_pdf_chunks(&hostile_pdf),
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
    fn extracted_text_limit_is_shared_by_plain_text_dispatch() {
        let bytes = vec![b'x'; MAX_EXTRACTED_TEXT_BYTES + 1];
        assert!(matches!(
            extract_chunks(&Dispatch::PlainText(parser_identity("plain_text")), &bytes),
            Err("parser_limit")
        ));
    }

    #[tokio::test]
    async fn empty_space_rebuild_publishes_only_a_derived_head() -> anyhow::Result<()> {
        let op = opendal::Operator::new(opendal::services::Memory::default())?.finish();
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
    async fn text_asset_materializes_and_is_searchable() -> anyhow::Result<()> {
        let op = opendal::Operator::new(opendal::services::Memory::default())?.finish();
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
}
