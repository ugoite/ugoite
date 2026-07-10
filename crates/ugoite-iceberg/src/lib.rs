//! Iceberg-native persistence and query boundary.
//!
//! One [`IcebergWorkspace`] represents one Ugoite Space namespace. Production
//! callers inject a durable Catalog; MemoryCatalog belongs in tests only.

mod migration;

pub use migration::{MigrationFormReport, MigrationManifest, MigrationReport};

use anyhow::{anyhow, Context, Result};
use arrow_array::RecordBatch;
use datafusion::execution::context::SessionContext;
use iceberg::memory::{MemoryCatalogBuilder, MEMORY_CATALOG_WAREHOUSE};
use iceberg::spec::{NestedField, PrimitiveType, Schema, SortOrder, Type, UnboundPartitionSpec};
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
use ugoite_domain::entry::EntryRevision;
use ugoite_domain::form::{Compatibility, FieldType, FormChangeSet, FormDefinition};
use ugoite_domain::id::{FormId, RevisionId, SpaceId};
use uuid::Uuid;

const FORM_DEFINITION_PROPERTY: &str = "ugoite.form.definition.v1";
const FORM_ID_PROPERTY: &str = "ugoite.form.id";
const FORM_NAME_PROPERTY: &str = "ugoite.form.name";
const FORM_VERSION_PROPERTY: &str = "ugoite.form.version";
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
        let form = self.load_form(form_id).await?;
        for revision in revisions {
            revision.validate(&form, None).or_else(|error| {
                if matches!(error, ugoite_domain::entry::RevisionError::VersionConflict) {
                    Ok(())
                } else {
                    Err(error)
                }
            })?;
        }
        let table = self.catalog.load_table(&self.form_ident(form_id)).await?;
        let mut data_files = Vec::new();
        for group in split_batches(batches, self.write.max_rows_per_file) {
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

pub fn physical_form_name(form_id: FormId) -> String {
    format!("form_{}", form_id.as_uuid().simple())
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
    ];
    for field in &form.fields {
        fields.push(Arc::new(NestedField::new(
            field.id.get(),
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
    Type::Primitive(match kind {
        FieldType::Boolean => PrimitiveType::Boolean,
        FieldType::Integer => PrimitiveType::Int,
        FieldType::Long => PrimitiveType::Long,
        FieldType::Float => PrimitiveType::Float,
        FieldType::Double => PrimitiveType::Double,
        FieldType::Date => PrimitiveType::Date,
        FieldType::Time => PrimitiveType::Time,
        FieldType::Timestamp => PrimitiveType::Timestamp,
        FieldType::TimestampTz => PrimitiveType::Timestamptz,
        FieldType::TimestampNs => PrimitiveType::TimestampNs,
        FieldType::TimestampTzNs => PrimitiveType::TimestamptzNs,
        FieldType::Uuid => PrimitiveType::Uuid,
        FieldType::Binary => PrimitiveType::Binary,
        FieldType::String
        | FieldType::Markdown
        | FieldType::Sql
        | FieldType::List
        | FieldType::ObjectList
        | FieldType::RowReference => PrimitiveType::String,
    })
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

fn split_batches(batches: Vec<RecordBatch>, max_rows: usize) -> Vec<Vec<RecordBatch>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    let mut rows = 0;
    for batch in batches {
        if !current.is_empty() && rows + batch.num_rows() > max_rows {
            groups.push(std::mem::take(&mut current));
            rows = 0;
        }
        rows += batch.num_rows();
        current.push(batch);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}
