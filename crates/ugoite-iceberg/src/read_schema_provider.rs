//! A static Iceberg provider with an explicit snapshot and read schema.
//!
//! `iceberg-datafusion` 0.10 binds the Arrow schema of a static provider to the
//! selected snapshot.  Ugoite needs the two coordinates to be independent for
//! metadata-only Form evolution: the snapshot remains immutable, while the
//! current Iceberg schema supplies field names and typed nulls.  This small
//! provider extension keeps that distinction at the provider boundary.  It
//! delegates storage scanning and safe system-predicate pushdown to the
//! upstream provider, while DataFusion retains latest-state and payload
//! semantics.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use datafusion::arrow::array::{new_null_array, ArrayRef, RecordBatch};
use datafusion::arrow::datatypes::SchemaRef;
use datafusion::catalog::Session;
use datafusion::common::tree_node::{Transformed, TreeNode};
use datafusion::common::Column;
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
    source_schema: SchemaRef,
    schema: SchemaRef,
    source_names_by_read_field: Vec<Option<String>>,
    source_indices_by_read_field: Vec<Option<usize>>,
    read_indices_by_name: HashMap<String, usize>,
    read_field_is_system: Vec<bool>,
}

impl CurrentSchemaTableProvider {
    pub(crate) async fn try_new(table: Table, snapshot_id: i64) -> Result<Self> {
        let snapshot = table
            .metadata()
            .snapshot_by_id(snapshot_id)
            .context("current Iceberg snapshot is missing")?;
        let snapshot_schema = snapshot
            .schema(table.metadata())
            .context("load Iceberg snapshot schema")?
            .as_ref()
            .clone();
        let read_schema = table.metadata().current_schema().as_ref().clone();
        let schema = Arc::new(
            schema_to_arrow_schema(&read_schema).context("convert current Iceberg read schema")?,
        );
        let source_schema = Arc::new(
            schema_to_arrow_schema(&snapshot_schema).context("convert Iceberg snapshot schema")?,
        );
        let source_names_by_read_field: Vec<Option<String>> = read_schema
            .as_struct()
            .fields()
            .iter()
            .map(|field| {
                snapshot_schema
                    .field_by_id(field.id)
                    .map(|source_field| source_field.name.clone())
            })
            .collect();
        let read_field_is_system = read_schema
            .as_struct()
            .fields()
            .iter()
            .map(|field| field.id < crate::FIRST_FORM_FIELD_ID)
            .collect();
        let source_indices_by_read_field = source_names_by_read_field
            .iter()
            .map(|source_name| {
                source_name
                    .as_deref()
                    .and_then(|name| source_schema.index_of(name).ok())
            })
            .collect();
        let read_indices_by_name = schema
            .fields()
            .iter()
            .enumerate()
            .map(|(index, field)| (field.name().clone(), index))
            .collect();
        let source = Arc::new(
            IcebergStaticTableProvider::try_new_from_table_snapshot(table, snapshot_id)
                .await
                .context("open immutable Iceberg snapshot provider")?,
        );

        Ok(Self {
            source,
            source_schema,
            schema,
            source_names_by_read_field,
            source_indices_by_read_field,
            read_indices_by_name,
            read_field_is_system,
        })
    }

    /// Rewrite a safe predicate expressed against the current read schema to
    /// the physical names in the selected snapshot. Form-owned predicates are
    /// deliberately left above this provider: the normal read plan applies
    /// them after latest-revision selection, and pushing them into the raw
    /// revision scan would change that semantic. A field added after the
    /// snapshot has no source column and likewise stays above this provider,
    /// where its typed null can be evaluated correctly.
    fn translate_filter(&self, filter: &Expr) -> DfResult<Option<Expr>> {
        if filter.column_refs().iter().any(|column| {
            self.read_indices_by_name
                .get(&column.name)
                .is_none_or(|index| {
                    !self.read_field_is_system[*index]
                        || self.source_names_by_read_field[*index].is_none()
                })
        }) {
            return Ok(None);
        }

        let translated = filter
            .clone()
            .transform(|expr| {
                if let Expr::Column(column) = &expr {
                    if let Some(index) = self.read_indices_by_name.get(&column.name) {
                        let source_name = self.source_names_by_read_field[*index]
                            .as_ref()
                            .expect("translated filter fields are present in the snapshot");
                        let mut source_column: Column = column.clone();
                        source_column.name = source_name.clone();
                        return Ok(Transformed::yes(Expr::Column(source_column)));
                    }
                    // A column outside the read schema cannot be proven to
                    // have the same meaning in this snapshot.
                    return Ok(Transformed::no(expr));
                }
                Ok(Transformed::no(expr))
            })?
            .data;
        Ok(Some(translated))
    }

    fn translated_filters(
        &self,
        filters: &[Expr],
    ) -> DfResult<Vec<Option<(Expr, TableProviderFilterPushDown)>>> {
        let candidates = filters
            .iter()
            .map(|filter| self.translate_filter(filter))
            .collect::<DfResult<Vec<_>>>()?;
        let translated = candidates.iter().flatten().collect::<Vec<_>>();
        let support = self
            .source
            .supports_filters_pushdown(&translated.to_vec())?;
        let mut support = support.into_iter();
        Ok(candidates
            .into_iter()
            .map(|filter| {
                filter.map(|filter| {
                    (
                        filter,
                        support
                            .next()
                            .expect("Iceberg provider returns one status per filter"),
                    )
                })
            })
            .collect())
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
        filters: &[Expr],
        limit: Option<usize>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        let translated_filters = self.translated_filters(filters)?;
        let pushed_filters = translated_filters
            .iter()
            .flatten()
            .filter(|(_, support)| *support != TableProviderFilterPushDown::Unsupported)
            .map(|(filter, _)| filter.clone())
            .collect::<Vec<_>>();
        let can_push_limit = filters.is_empty()
            || translated_filters.iter().all(|candidate| {
                candidate
                    .as_ref()
                    .is_some_and(|(_, support)| *support == TableProviderFilterPushDown::Exact)
            });

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

        let source_projection = projection.map(|_| {
            let mut source_indices = read_field_indices
                .iter()
                .filter_map(|index| self.source_indices_by_read_field[*index])
                .collect::<Vec<_>>();
            for filter in &pushed_filters {
                for column in filter.column_refs() {
                    if let Ok(index) = self.source_schema.index_of(&column.name) {
                        source_indices.push(index);
                    }
                }
            }
            source_indices.sort_unstable();
            source_indices.dedup();
            source_indices
        });
        let source_projection = source_projection.filter(|indices| !indices.is_empty());
        let source_plan = self
            .source
            .scan(
                state,
                source_projection.as_ref(),
                &pushed_filters,
                can_push_limit.then_some(limit).flatten(),
            )
            .await?;
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
        let translated = filters
            .iter()
            .map(|filter| self.translate_filter(filter))
            .collect::<DfResult<Vec<_>>>()?;
        let source_filters = translated.iter().flatten().collect::<Vec<_>>();
        let source_support = self
            .source
            .supports_filters_pushdown(&source_filters.to_vec())?;
        let mut source_support = source_support.into_iter();
        translated
            .into_iter()
            .map(|filter| {
                Ok(
                    filter.map_or(TableProviderFilterPushDown::Unsupported, |_| {
                        source_support
                            .next()
                            .expect("Iceberg provider returns one status per filter")
                    }),
                )
            })
            .collect()
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
        project_read_batch(&self.schema, &self.source_names, &batch)
    }
}

fn project_read_batch(
    schema: &SchemaRef,
    source_names: &[Option<String>],
    batch: &RecordBatch,
) -> DfResult<RecordBatch> {
    let columns = source_names
        .iter()
        .enumerate()
        .map(|(output_index, source_name)| {
            let arrow_field = schema.field(output_index);
            match source_name {
                None => Ok(new_null_array(arrow_field.data_type(), batch.num_rows())),
                Some(name) => batch.column_by_name(name).cloned().ok_or_else(|| {
                    datafusion::error::DataFusionError::Execution(format!(
                        "Iceberg snapshot scan omitted expected source column '{name}'"
                    ))
                }),
            }
        })
        .collect::<DfResult<Vec<ArrayRef>>>()?;
    RecordBatch::try_new(schema.clone(), columns).map_err(|error| {
        datafusion::error::DataFusionError::Execution(format!(
            "project Iceberg snapshot by stable field ID: {error}"
        ))
    })
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

#[cfg(test)]
mod tests {
    use super::project_read_batch;
    use datafusion::arrow::array::{Int32Array, StringArray};
    use datafusion::arrow::datatypes::{DataType, Field, Schema};
    use std::sync::Arc;

    #[test]
    fn only_fields_absent_from_the_snapshot_materialize_as_typed_null() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "added",
            DataType::List(Arc::new(Field::new("item", DataType::Int32, true))),
            true,
        )]));
        let source_schema = Arc::new(Schema::new(vec![Field::new(
            "unrelated",
            DataType::Int32,
            true,
        )]));
        let batch = datafusion::arrow::record_batch::RecordBatch::try_new(
            source_schema,
            vec![Arc::new(Int32Array::from(vec![1]))],
        )
        .expect("test batch");

        let projected = project_read_batch(&schema, &[None], &batch).expect("typed null");
        assert_eq!(projected.column(0).data_type(), schema.field(0).data_type());
        assert_eq!(projected.column(0).null_count(), 1);
    }

    #[test]
    fn expected_source_column_omission_is_a_query_error() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "current",
            DataType::Int32,
            true,
        )]));
        let missing_batch_schema = Arc::new(Schema::new(vec![Field::new(
            "other",
            DataType::Int32,
            true,
        )]));
        let missing_batch = datafusion::arrow::record_batch::RecordBatch::try_new(
            missing_batch_schema,
            vec![Arc::new(Int32Array::from(vec![1]))],
        )
        .expect("test batch");
        let error = project_read_batch(&schema, &[Some("current".into())], &missing_batch)
            .expect_err("a source column that disappeared is not schema evolution");
        assert!(error
            .to_string()
            .contains("expected source column 'current'"));

        let wrong_type_schema = Arc::new(Schema::new(vec![Field::new(
            "current",
            DataType::Utf8,
            true,
        )]));
        let wrong_type_batch = datafusion::arrow::record_batch::RecordBatch::try_new(
            wrong_type_schema,
            vec![Arc::new(StringArray::from(vec!["wrong"]))],
        )
        .expect("test batch");
        assert!(
            project_read_batch(&schema, &[Some("current".into())], &wrong_type_batch)
                .expect_err("a source type mismatch must fail")
                .to_string()
                .contains("stable field ID")
        );
    }
}
