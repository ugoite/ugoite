//! Iceberg-native persistence and query boundary.
//!
//! One [`IcebergWorkspace`] represents one Ugoite Space namespace. Production
//! callers inject a durable Catalog; MemoryCatalog belongs in tests only.

mod migration;

pub use migration::{MigrationFormReport, MigrationManifest, MigrationReport};

use anyhow::{anyhow, Context, Result};
use arrow_array::{Array, FixedSizeBinaryArray, Int64Array, RecordBatch, StringArray};
use datafusion::execution::context::SessionContext;
use iceberg::memory::{MemoryCatalogBuilder, MEMORY_CATALOG_WAREHOUSE};
use iceberg::spec::{
    ListType, NestedField, PrimitiveType, Schema, SortOrder, StructType, Type, UnboundPartitionSpec,
};
use iceberg::transaction::{ApplyTransactionAction, Transaction};
use iceberg::writer::file_writer::{FileWriter, FileWriterBuilder, ParquetWriterBuilder};
use iceberg::{Catalog, CatalogBuilder, NamespaceIdent, TableCreation, TableIdent};
use iceberg_catalog_rest::{
    RestCatalogBuilder, REST_CATALOG_PROP_URI, REST_CATALOG_PROP_WAREHOUSE,
};
use iceberg_datafusion::IcebergCatalogProvider;
use parquet::file::properties::WriterProperties;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use ugoite_domain::entry::{EntryOperation, EntryRevision};
use ugoite_domain::form::{Compatibility, FieldType, FormChangeSet, FormDefinition};
use ugoite_domain::id::{FormId, RevisionId, SpaceId};
use uuid::Uuid;

const FORM_DEFINITION_PROPERTY: &str = "ugoite.form.definition.v1";
const FORM_ID_PROPERTY: &str = "ugoite.form.id";
const FORM_NAME_PROPERTY: &str = "ugoite.form.name";
const FORM_VERSION_PROPERTY: &str = "ugoite.form.version";
const FORM_FIELD_MAPPING_PROPERTY: &str = "ugoite.form.field-id-map.v1";
const TARGET_FILE_SIZE_PROPERTY: &str = "write.target-file-size-bytes";
const FIRST_FORM_FIELD_ID: i32 = 100;

#[derive(Debug, Clone)]
pub struct IcebergWorkspace {
    catalog: Arc<dyn Catalog>,
    namespace: NamespaceIdent,
    warehouse: String,
    write: WriteConfig,
}

#[derive(Debug, Clone, Copy)]
pub struct WriteConfig {
    pub target_file_size_bytes: u64,
    pub max_rows_per_file: usize,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            target_file_size_bytes: 128 * 1024 * 1024,
            max_rows_per_file: 100_000,
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
    pub async fn new(
        catalog: Arc<dyn Catalog>,
        space_id: SpaceId,
        warehouse: impl Into<String>,
        write: WriteConfig,
    ) -> Result<Self> {
        let namespace = NamespaceIdent::new(format!("space_{}", space_id.as_uuid().simple()));
        if !catalog.namespace_exists(&namespace).await? {
            catalog.create_namespace(&namespace, HashMap::new()).await?;
        }
        Ok(Self {
            catalog,
            namespace,
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

    pub async fn rest_catalog(
        uri: &str,
        warehouse: &str,
        properties: impl IntoIterator<Item = (String, String)>,
    ) -> Result<Arc<dyn Catalog>> {
        let mut props = HashMap::from([
            (REST_CATALOG_PROP_URI.to_string(), uri.to_string()),
            (
                REST_CATALOG_PROP_WAREHOUSE.to_string(),
                warehouse.to_string(),
            ),
        ]);
        props.extend(properties);
        Ok(Arc::new(
            RestCatalogBuilder::default().load("ugoite", props).await?,
        ))
    }

    pub fn namespace(&self) -> &NamespaceIdent {
        &self.namespace
    }
    pub fn catalog(&self) -> Arc<dyn Catalog> {
        self.catalog.clone()
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
        for (index, field) in form.fields.iter().enumerate() {
            if mapping.get(&field.id.get().to_string()) != Some(&physical_field_id(index)) {
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
        // iceberg-rust 0.8 does not expose a public schema-update transaction.
        // Metadata-only changes are still committed atomically; schema-bearing
        // changes are rejected instead of rebuilding or rewriting the table.
        if form_schema(&current)? != form_schema(&evolved)? {
            return Err(anyhow!("the configured Iceberg catalog does not expose schema evolution; no data was rewritten"));
        }
        let table = self
            .catalog
            .load_table(&self.form_ident(current.id))
            .await?;
        let tx = Transaction::new(&table);
        let mut action = tx.update_table_properties();
        for (key, value) in form_properties(&evolved, self.write)? {
            action = action.set(key, value);
        }
        action.apply(tx)?.commit(self.catalog.as_ref()).await?;
        Ok(evolved)
    }

    pub async fn append_record_batches(
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
        let mut current = self.latest_revisions(&table).await?;
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
        let mut data_files = Vec::new();
        for group in split_batches(batches, self.write.max_rows_per_file)? {
            let output_path = format!(
                "{}/data/{}.parquet",
                table.metadata().location(),
                Uuid::new_v4()
            );
            let output = table.file_io().new_output(&output_path)?;
            let props = WriterProperties::builder()
                .set_max_row_group_size(self.write.max_rows_per_file)
                .build();
            let mut writer =
                ParquetWriterBuilder::new(props, table.metadata().current_schema().clone())
                    .build(output)
                    .await?;
            for batch in group {
                writer.write(&batch).await?;
            }
            for builder in writer.close().await? {
                data_files.push(builder.build()?);
            }
        }
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

    async fn latest_revisions(
        &self,
        table: &iceberg::table::Table,
    ) -> Result<std::collections::HashMap<ugoite_domain::id::EntryId, LatestRevision>> {
        let scan = table
            .scan()
            .select(vec!["entry_id", "revision_id", "entry_version"])
            .build()?;
        let tasks = scan.plan_files().await?;
        let reader = iceberg::arrow::ArrowReaderBuilder::new(table.file_io().clone()).build();
        let mut stream = reader.read(tasks)?;
        let mut latest = std::collections::HashMap::new();
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
                let replace = latest
                    .get(&entry_id)
                    .is_none_or(|known: &LatestRevision| version > known.entry_version);
                if replace {
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
            "{}/{}",
            self.warehouse.trim_end_matches('/'),
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

fn physical_field_id(index: usize) -> i32 {
    13 + i32::try_from(index).expect("Form field count exceeds Iceberg field ID range")
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
    for (index, field) in form.fields.iter().enumerate() {
        fields.push(Arc::new(NestedField::new(
            physical_field_id(index),
            field.name.clone(),
            iceberg_type(&field.field_type),
            field.required,
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

fn iceberg_type(kind: &FieldType) -> Type {
    match kind {
        FieldType::Boolean => Type::Primitive(PrimitiveType::Boolean),
        FieldType::Integer => Type::Primitive(PrimitiveType::Int),
        FieldType::Long => Type::Primitive(PrimitiveType::Long),
        FieldType::Float => Type::Primitive(PrimitiveType::Float),
        FieldType::Double => Type::Primitive(PrimitiveType::Double),
        FieldType::Date
        | FieldType::Time
        | FieldType::Timestamp
        | FieldType::TimestampTz
        | FieldType::TimestampNs
        | FieldType::TimestampTzNs
        | FieldType::Uuid
        | FieldType::Binary
        | FieldType::String
        | FieldType::Markdown
        | FieldType::Sql
        | FieldType::RowReference => Type::Primitive(PrimitiveType::String),
        FieldType::List => Type::List(ListType::new(Arc::new(NestedField::new(
            1_000_000,
            "element",
            Type::Primitive(PrimitiveType::String),
            false,
        )))),
        FieldType::ObjectList => {
            let fields = vec![
                Arc::new(NestedField::new(
                    1_000_001,
                    "type",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
                Arc::new(NestedField::new(
                    1_000_002,
                    "name",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
                Arc::new(NestedField::new(
                    1_000_003,
                    "description",
                    Type::Primitive(PrimitiveType::String),
                    false,
                )),
            ];
            Type::List(ListType::new(Arc::new(NestedField::new(
                1_000_000,
                "element",
                Type::Struct(StructType::new(fields)),
                false,
            ))))
        }
    }
}

fn form_properties(form: &FormDefinition, write: WriteConfig) -> Result<HashMap<String, String>> {
    let field_mapping = form
        .fields
        .iter()
        .enumerate()
        .map(|(index, field)| (field.id.get().to_string(), physical_field_id(index)))
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

fn split_batches(batches: Vec<RecordBatch>, max_rows: usize) -> Result<Vec<Vec<RecordBatch>>> {
    if max_rows == 0 {
        return Err(anyhow!("max_rows_per_file must be greater than zero"));
    }
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut rows = 0;
    for batch in batches {
        let mut offset = 0;
        while offset < batch.num_rows() {
            if rows == max_rows {
                groups.push(std::mem::take(&mut current));
                rows = 0;
            }
            let count = (batch.num_rows() - offset).min(max_rows - rows);
            current.push(batch.slice(offset, count));
            rows += count;
            offset += count;
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    Ok(groups)
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
