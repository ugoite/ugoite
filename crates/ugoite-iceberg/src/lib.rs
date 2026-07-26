//! Iceberg-native persistence and query boundary.
//!
//! One [`IcebergWorkspace`] represents one Ugoite Space namespace. Production
//! callers inject a durable Catalog; MemoryCatalog belongs in tests only.

mod migration;
mod space_catalog;

pub mod asset;
pub mod audit;
pub mod authorization;
pub mod entry;
pub mod form;
pub mod iceberg_store;
pub mod index;
pub mod integrity;
pub mod link;
pub mod materialized_view;
pub mod preferences;
pub mod sample_data;
pub mod saved_sql;
pub mod search;
pub mod service;
pub mod space;
pub mod sql;
pub mod sql_session;
pub mod storage;

pub use migration::{MigrationFormReport, MigrationManifest, MigrationReport};
pub use space_catalog::{PublicationContext, SpaceCatalog};

use anyhow::{anyhow, Context, Result};
use arrow_array::builder::{
    BinaryBuilder, FixedSizeBinaryBuilder, ListBuilder, StringBuilder, StructBuilder,
};
use arrow_array::{
    Array, ArrayRef, BooleanArray, Date32Array, FixedSizeBinaryArray, Float32Array, Float64Array,
    Int32Array, Int64Array, RecordBatch, StringArray, Time64MicrosecondArray,
    TimestampMicrosecondArray, TimestampNanosecondArray,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use datafusion::execution::context::SessionContext;
use iceberg::expr::Reference;
use iceberg::memory::{MemoryCatalogBuilder, MEMORY_CATALOG_WAREHOUSE};
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
use iceberg::{Catalog, CatalogBuilder, NamespaceIdent, TableCreation, TableIdent};
use iceberg_datafusion::IcebergCatalogProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use ugoite_domain::entry::{EntryOperation, EntryRevision, FieldValue};
use ugoite_domain::form::{Compatibility, FieldType, FormChangeSet, FormDefinition, FormField};
use ugoite_domain::id::{FormId, RevisionId, SpaceId};
use ugoite_storage::SpaceCatalogStore;
use uuid::Uuid;

const FORM_DEFINITION_PROPERTY: &str = "ugoite.form.definition.v1";
const FORM_ID_PROPERTY: &str = "ugoite.form.id";
const FORM_NAME_PROPERTY: &str = "ugoite.form.name";
const FORM_VERSION_PROPERTY: &str = "ugoite.form.version";
const FORM_FIELD_MAPPING_PROPERTY: &str = "ugoite.form.field-id-map.v1";
const TARGET_FILE_SIZE_PROPERTY: &str = "write.target-file-size-bytes";
const FIRST_FORM_FIELD_ID: i32 = 100;
const NESTED_FIELD_ID_BASE: i32 = 1_000_000;

#[derive(Debug, Clone)]
pub struct IcebergWorkspace {
    catalog: Arc<dyn Catalog>,
    namespace: NamespaceIdent,
    space_id: SpaceId,
    warehouse: String,
    write: WriteConfig,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SchemaCommitCapability {
    MetadataOnly,
    AtomicSchemaEvolution,
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
    pub snapshot_id: i64,
    pub committed_revision_ids: Vec<RevisionId>,
    pub committed_at_micros: i64,
    pub data_file_count: usize,
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
    pub async fn open_space(
        store: SpaceCatalogStore,
        space_id: SpaceId,
        write: WriteConfig,
    ) -> Result<Self> {
        let warehouse = store.warehouse_uri();
        Self::new(
            Arc::new(SpaceCatalog::new(store, space_id)?),
            space_id,
            warehouse,
            write,
        )
        .await
    }

    pub async fn new(
        catalog: Arc<dyn Catalog>,
        space_id: SpaceId,
        warehouse: impl Into<String>,
        write: WriteConfig,
    ) -> Result<Self> {
        let namespace = namespace_for_space(space_id);
        if !catalog.namespace_exists(&namespace).await? {
            catalog.create_namespace(&namespace, HashMap::new()).await?;
        }
        Ok(Self {
            catalog,
            namespace,
            space_id,
            warehouse: warehouse.into(),
            write,
        })
    }

    pub async fn memory_for_tests(space_id: SpaceId, warehouse: impl Into<String>) -> Result<Self> {
        let warehouse = warehouse.into();
        let catalog = MemoryCatalogBuilder::default()
            .load(
                "ugoite-test",
                HashMap::from([(MEMORY_CATALOG_WAREHOUSE.to_string(), warehouse.clone())]),
            )
            .await?;
        Self::new(
            Arc::new(catalog),
            space_id,
            warehouse,
            WriteConfig::default(),
        )
        .await
    }

    pub fn namespace(&self) -> &NamespaceIdent {
        &self.namespace
    }
    pub fn catalog(&self) -> Arc<dyn Catalog> {
        self.catalog.clone()
    }

    pub fn schema_commit_capability(&self) -> SchemaCommitCapability {
        SchemaCommitCapability::AtomicSchemaEvolution
    }

    pub async fn create_form(&self, form: &FormDefinition) -> Result<()> {
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
        self.catalog.create_table(&self.namespace, creation).await?;
        Ok(())
    }

    pub async fn load_form(&self, form_id: FormId) -> Result<FormDefinition> {
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
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
        let mapping_raw = table
            .metadata()
            .properties()
            .get(FORM_FIELD_MAPPING_PROPERTY)
            .context("Iceberg table is missing Form field ID mapping")?;
        let mapping: HashMap<String, i32> = serde_json::from_str(mapping_raw)?;
        for field in &form.fields {
            if mapping.get(&field.id.get().to_string()) != Some(&physical_field_id(field)) {
                return Err(anyhow!(
                    "Form field ID mapping does not match Iceberg schema"
                ));
            }
        }
        Ok(form)
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

    pub async fn evolve_form(&self, changes: &FormChangeSet) -> Result<FormDefinition> {
        let current = self.load_form(changes.form_id).await?;
        if changes.expected_version != Some(current.version) {
            return Err(anyhow!("Form version conflict"));
        }
        match changes.compatibility(&current)? {
            Compatibility::Breaking => {
                return Err(anyhow!(
                    "breaking Form change requires an explicit major migration"
                ))
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
        if form_schema(&current)? != form_schema(&evolved)? {
            let current_fields = current
                .fields
                .iter()
                .map(|field| field.id)
                .collect::<std::collections::HashSet<_>>();
            if evolved.fields.len() < current.fields.len()
                || current.fields.iter().any(|field| {
                    evolved.fields.iter().find(|next| next.id == field.id) != Some(field)
                })
            {
                return Err(anyhow!(
                    "Iceberg schema evolution supports additive Form fields only"
                ));
            }
            let mut schema_action = Transaction::new(&table).update_schema();
            for field in evolved
                .fields
                .iter()
                .filter(|field| !current_fields.contains(&field.id))
            {
                schema_action = schema_action.add_column(AddColumn::optional(
                    &field.name,
                    iceberg_type(&field.field_type, field.id.get()),
                ));
            }
            let transaction = schema_action.apply(Transaction::new(&table))?;
            let mut properties = transaction.update_table_properties();
            for (key, value) in form_properties(&evolved, self.write)? {
                properties = properties.set(key, value);
            }
            properties
                .apply(transaction)?
                .commit(self.catalog.as_ref())
                .await?;
            return self.load_form(changes.form_id).await;
        }
        let tx = Transaction::new(&table);
        let mut action = tx.update_table_properties();
        for (key, value) in form_properties(&evolved, self.write)? {
            action = action.set(key, value);
        }
        action.apply(tx)?.commit(self.catalog.as_ref()).await?;
        Ok(evolved)
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
            .collect::<std::collections::HashSet<_>>();
        let mut current = self.latest_revisions(&table, Some(&entry_ids)).await?;
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
            current.insert(
                revision.entry_id,
                LatestRevision {
                    revision_id: revision.revision_id,
                    entry_version: revision.entry_version,
                },
            );
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
        let updated = action.apply(tx)?.commit(self.catalog.as_ref()).await?;
        let snapshot_id = updated
            .metadata()
            .current_snapshot()
            .context("append commit did not create a snapshot")?
            .snapshot_id();
        Ok(CommitReceipt {
            snapshot_id,
            committed_revision_ids: ids,
            committed_at_micros,
            data_file_count: data_files.len(),
        })
    }

    pub async fn append_revisions(
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

    async fn latest_revisions(
        &self,
        table: &iceberg::table::Table,
        entry_ids: Option<&std::collections::HashSet<ugoite_domain::id::EntryId>>,
    ) -> Result<std::collections::HashMap<ugoite_domain::id::EntryId, LatestRevision>> {
        let mut scan = table
            .scan()
            .select(vec!["entry_id", "revision_id", "entry_version"]);
        if let Some(entry_ids) = entry_ids {
            scan = scan.with_filter(
                Reference::new("entry_id").is_in(
                    entry_ids
                        .iter()
                        .map(|entry_id| Datum::uuid(entry_id.as_uuid())),
                ),
            );
        }
        let scan = scan.build()?;
        let mut stream = scan.to_arrow().await?;
        let mut latest: std::collections::HashMap<ugoite_domain::id::EntryId, LatestRevision> =
            std::collections::HashMap::new();
        while let Some(batch) = futures::TryStreamExt::try_next(&mut stream).await? {
            let entry_ids = batch
                .column_by_name("entry_id")
                .context("Iceberg table is missing entry_id")?;
            let revision_ids = batch
                .column_by_name("revision_id")
                .context("Iceberg table is missing revision_id")?;
            let versions = batch
                .column_by_name("entry_version")
                .and_then(|array| array.as_any().downcast_ref::<Int64Array>())
                .context("entry_version must be an int64 column")?;
            for row in 0..batch.num_rows() {
                let entry_id = uuid_at(entry_ids, row)?;
                let revision_id =
                    ugoite_domain::id::RevisionId::from_uuid(uuid_at(revision_ids, row)?.as_uuid());
                let version = u64::try_from(versions.value(row))
                    .map_err(|_| anyhow!("entry_version must be non-negative"))?;
                if let Some(known) = latest.get(&entry_id) {
                    if version == known.entry_version && revision_id != known.revision_id {
                        return Err(anyhow!(
                            "entry revision conflict: entry {entry_id} has multiple revisions at version {version}"
                        ));
                    }
                }
                if latest
                    .get(&entry_id)
                    .is_none_or(|known: &LatestRevision| version > known.entry_version)
                {
                    latest.insert(
                        entry_id,
                        LatestRevision {
                            revision_id,
                            entry_version: version,
                        },
                    );
                }
            }
        }
        Ok(latest)
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

#[derive(Debug, Clone, Copy)]
struct LatestRevision {
    revision_id: ugoite_domain::id::RevisionId,
    entry_version: u64,
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
    ];
    for field in &form.fields {
        fields.push(Arc::new(NestedField::new(
            physical_field_id(field),
            field.name.clone(),
            iceberg_type(&field.field_type, physical_field_id(field)),
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

fn iceberg_type(kind: &FieldType, parent_id: i32) -> Type {
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
        FieldType::List => Type::List(ListType::new(Arc::new(NestedField::new(
            nested_field_id(parent_id, 0),
            "element",
            Type::Primitive(PrimitiveType::String),
            false,
        )))),
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

fn nested_field_id(parent_id: i32, offset: i32) -> i32 {
    NESTED_FIELD_ID_BASE + parent_id * 10 + offset
}

fn form_properties(form: &FormDefinition, write: WriteConfig) -> Result<HashMap<String, String>> {
    let field_mapping = form
        .fields
        .iter()
        .map(|field| (field.id.get().to_string(), physical_field_id(field)))
        .collect::<HashMap<_, _>>();
    Ok(HashMap::from([
        (
            FORM_DEFINITION_PROPERTY.into(),
            serde_json::to_string(form)?,
        ),
        (FORM_ID_PROPERTY.into(), form.id.to_string()),
        (FORM_NAME_PROPERTY.into(), form.name.clone()),
        (FORM_VERSION_PROPERTY.into(), form.version.get().to_string()),
        (
            FORM_FIELD_MAPPING_PROPERTY.into(),
            serde_json::to_string(&field_mapping)?,
        ),
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
        FieldType::List => {
            let element_field = match arrow_field.data_type() {
                arrow_schema::DataType::List(element) => element.clone(),
                other => return Err(anyhow!("list field has invalid Arrow type: {other:?}")),
            };
            let mut builder = ListBuilder::new(StringBuilder::new()).with_field(element_field);
            for value in values {
                if let Some(FieldValue::List(items)) = value {
                    for item in items {
                        if let FieldValue::String(item) = item {
                            builder.values().append_value(item);
                        }
                    }
                    builder.append(true);
                } else {
                    builder.append(false);
                }
            }
            Ok(Arc::new(builder.finish()))
        }
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
            let mut builder = BinaryBuilder::with_capacity(values.len(), 0);
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
