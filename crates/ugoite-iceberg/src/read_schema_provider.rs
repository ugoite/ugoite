//! A static Iceberg provider with an explicit snapshot and read schema.
//!
//! `iceberg-datafusion` 0.10 binds the Arrow schema of a static provider to the
//! selected snapshot.  Ugoite needs the two coordinates to be independent for
//! metadata-only Form evolution: the snapshot remains immutable, while the
//! current Iceberg schema supplies field names and typed nulls.  This small
//! provider extension keeps that distinction at the provider boundary.  It
//! delegates storage scanning to the upstream provider and leaves filtering,
//! projection, sorting, and limits to DataFusion.

use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use datafusion::arrow::array::{new_null_array, ArrayRef, RecordBatch};
use datafusion::arrow::datatypes::SchemaRef;
use datafusion::catalog::Session;
use datafusion::datasource::{TableProvider, TableType};
use datafusion::error::Result as DfResult;
use datafusion::execution::TaskContext;
use datafusion::logical_expr::dml::InsertOp;
use datafusion::logical_expr::{Expr, TableProviderFilterPushDown};
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
    SendableRecordBatchStream,
};
use futures::{StreamExt, TryStreamExt};
use iceberg::arrow::schema_to_arrow_schema;
use iceberg::table::Table;
use iceberg_datafusion::IcebergStaticTableProvider;

/// A read provider whose snapshot coordinate and read schema are explicit.
#[derive(Debug)]
pub(crate) struct CurrentSchemaTableProvider {
    source: Arc<dyn TableProvider>,
    schema: SchemaRef,
    source_names_by_read_field: Vec<Option<String>>,
}

impl CurrentSchemaTableProvider {
    pub(crate) async fn try_new(table: Table, snapshot_id: i64) -> Result<Self> {
        let snapshot = table
            .metadata()
            .snapshot_by_id(snapshot_id)
            .context("current Iceberg snapshot is missing")?;
        let source_schema = snapshot
            .schema(table.metadata())
            .context("load Iceberg snapshot schema")?
            .as_ref()
            .clone();
        let read_schema = table.metadata().current_schema().as_ref().clone();
        let schema = Arc::new(
            schema_to_arrow_schema(&read_schema).context("convert current Iceberg read schema")?,
        );
        let source_names_by_read_field = read_schema
            .as_struct()
            .fields()
            .iter()
            .map(|field| {
                source_schema
                    .field_by_id(field.id)
                    .map(|source_field| source_field.name.clone())
            })
            .collect();
        let source = Arc::new(
            IcebergStaticTableProvider::try_new_from_table_snapshot(table, snapshot_id)
                .await
                .context("open immutable Iceberg snapshot provider")?,
        );

        Ok(Self {
            source,
            schema,
            source_names_by_read_field,
        })
    }
}

#[async_trait]
impl TableProvider for CurrentSchemaTableProvider {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    async fn scan(
        &self,
        state: &dyn Session,
        projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        _limit: Option<usize>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        // The source provider is deliberately asked for every source column
        // and no pushed-down predicate.  DataFusion evaluates the authorized
        // relation, predicates, projection and bounds above this one fixed
        // snapshot after the field-ID projection has been applied.
        let source_plan = self.source.scan(state, None, &[], None).await?;
        let read_field_indices = projection
            .cloned()
            .unwrap_or_else(|| (0..self.schema.fields().len()).collect());
        let output_schema = if projection.is_some() {
            Arc::new(self.schema.project(&read_field_indices)?)
        } else {
            self.schema.clone()
        };
        let source_names = read_field_indices
            .iter()
            .map(|index| self.source_names_by_read_field[*index].clone())
            .collect();
        Ok(Arc::new(ReadSchemaProjectionExec::new(
            source_plan,
            output_schema,
            source_names,
        )))
    }

    fn supports_filters_pushdown(
        &self,
        filters: &[&Expr],
    ) -> DfResult<Vec<TableProviderFilterPushDown>> {
        Ok(vec![
            TableProviderFilterPushDown::Unsupported;
            filters.len()
        ])
    }

    async fn insert_into(
        &self,
        _state: &dyn Session,
        _input: Arc<dyn ExecutionPlan>,
        _insert_op: InsertOp,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        Err(datafusion::error::DataFusionError::Plan(
            "write operations are not supported on a static read provider".to_string(),
        ))
    }
}

/// Translate top-level columns by stable Iceberg field ID and materialize
/// fields that did not exist in the selected snapshot as typed Arrow nulls.
#[derive(Debug)]
struct ReadSchemaProjectionExec {
    child: Arc<dyn ExecutionPlan>,
    schema: SchemaRef,
    plan_properties: Arc<PlanProperties>,
    source_names: Vec<Option<String>>,
}

impl ReadSchemaProjectionExec {
    fn new(
        child: Arc<dyn ExecutionPlan>,
        schema: SchemaRef,
        source_names: Vec<Option<String>>,
    ) -> Self {
        let plan_properties = Arc::new(PlanProperties::new(
            EquivalenceProperties::new(schema.clone()),
            Partitioning::UnknownPartitioning(child.properties().partitioning.partition_count()),
            EmissionType::Incremental,
            Boundedness::Bounded,
        ));
        Self {
            child,
            schema,
            plan_properties,
            source_names,
        }
    }

    fn project_batch(&self, batch: RecordBatch) -> DfResult<RecordBatch> {
        let columns = self
            .source_names
            .iter()
            .enumerate()
            .map(|(output_index, source_name)| {
                let arrow_field = self.schema.field(output_index);
                source_name
                    .as_deref()
                    .and_then(|name| batch.column_by_name(name).cloned())
                    .unwrap_or_else(|| new_null_array(arrow_field.data_type(), batch.num_rows()))
            })
            .collect::<Vec<ArrayRef>>();
        RecordBatch::try_new(self.schema.clone(), columns).map_err(|error| {
            datafusion::error::DataFusionError::Execution(format!(
                "project Iceberg snapshot by stable field ID: {error}"
            ))
        })
    }
}

impl ExecutionPlan for ReadSchemaProjectionExec {
    fn name(&self) -> &str {
        "IcebergReadSchemaProjection"
    }

    fn downcast_delegate(&self) -> Option<&dyn ExecutionPlan> {
        // Keep the upstream IcebergTableScan visible to the existing closed
        // query-boundary validator while this node only changes field-ID
        // projection and typed-null materialization.
        Some(self.child.as_ref())
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        vec![&self.child]
    }

    fn with_new_children(
        self: Arc<Self>,
        mut children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        if children.len() != 1 {
            return Err(datafusion::error::DataFusionError::Internal(
                "Iceberg read-schema projection expects one child".to_string(),
            ));
        }
        Ok(Arc::new(Self::new(
            children.remove(0),
            self.schema.clone(),
            self.source_names.clone(),
        )))
    }

    fn properties(&self) -> &Arc<PlanProperties> {
        &self.plan_properties
    }

    fn execute(
        &self,
        partition: usize,
        context: Arc<TaskContext>,
    ) -> DfResult<SendableRecordBatchStream> {
        let schema = self.schema.clone();
        let projected = self.clone_for_stream();
        let stream = self
            .child
            .execute(partition, context)?
            .map_ok(move |batch| projected.project_batch(batch));
        let stream = stream.map(|result| match result {
            Ok(Ok(batch)) => Ok(batch),
            Ok(Err(error)) => Err(error),
            Err(error) => Err(error),
        });
        Ok(Box::pin(RecordBatchStreamAdapter::new(schema, stream)))
    }
}

impl ReadSchemaProjectionExec {
    fn clone_for_stream(&self) -> Self {
        Self {
            child: self.child.clone(),
            schema: self.schema.clone(),
            plan_properties: self.plan_properties.clone(),
            source_names: self.source_names.clone(),
        }
    }
}

impl DisplayAs for ReadSchemaProjectionExec {
    fn fmt_as(
        &self,
        _format: DisplayFormatType,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        write!(formatter, "IcebergReadSchemaProjection")
    }
}
