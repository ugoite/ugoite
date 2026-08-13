use anyhow::{Context, Result};
use arrow_array::builder::{Float64Builder, StringBuilder};
use arrow_array::{Array, Float64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use datafusion::datasource::{TableProvider, TableType};
use datafusion::error::Result as DfResult;
use datafusion::execution::TaskContext;
use datafusion::logical_expr::{Expr, TableProviderFilterPushDown};
use datafusion::physical_expr::EquivalenceProperties;
use datafusion::physical_plan::execution_plan::{Boundedness, EmissionType};
use datafusion::physical_plan::stream::RecordBatchStreamAdapter;
use datafusion::physical_plan::{
    DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
    SendableRecordBatchStream,
};
use opendal::Operator;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;
use ugoite_core::query::EntryScope;
use ugoite_domain::entry::AssetReference;
use ugoite_domain::form::FieldType;

use crate::entry;
pub use ugoite_domain::search::KeywordSearchResult;

const ASSET_TEXT_SEARCH_PAGE_SIZE: usize = 2_048;
const ASSET_TEXT_SEARCH_MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const ASSET_TEXT_SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Keyword search over one bounded, authorized current-state DataFusion
/// payload plan.
pub async fn search_entries(
    op: &Operator,
    ws_path: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<KeywordSearchResult>> {
    let relation_scopes = entry::list_form_names(op, ws_path)
        .await?
        .into_iter()
        .map(|form_name| (form_name.to_ascii_lowercase(), EntryScope::AllCurrent))
        .collect();
    search_entries_with_scopes(op, ws_path, query, &relation_scopes, limit).await
}

/// Searches typed/system columns in one globally ordered DataFusion candidate
/// plan. Search intentionally excludes `extra_attributes` and opaque asset or
/// object-list structs; the searchable typed column set is defined by the
/// Form field type in the plan builder.
pub async fn search_entries_with_scopes(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &std::collections::BTreeMap<String, EntryScope>,
    limit: usize,
) -> Result<Vec<KeywordSearchResult>> {
    search_entries_with_scopes_after(op, ws_path, query, relation_scopes, limit, None).await
}

pub async fn search_entries_with_scopes_after(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &std::collections::BTreeMap<String, EntryScope>,
    limit: usize,
    after: Option<(&str, &str, &str)>,
) -> Result<Vec<KeywordSearchResult>> {
    let candidates = crate::index::query_entry_candidates_authorized_after(
        op,
        ws_path,
        relation_scopes,
        None,
        Some(query),
        limit,
        after,
    )
    .await?;
    let mut results = BTreeMap::new();
    for candidate in candidates {
        let result = KeywordSearchResult {
            id: candidate.entry_id,
            title: candidate.title,
            form: candidate.form_name,
            created_at: candidate.created_at,
            updated_at: candidate.updated_at,
        };
        results.insert((result.form.clone(), result.id.clone()), result);
    }

    // AssetText is joined only after the provider-side authorized current
    // Entry scan. The join itself runs in DataFusion, so a match after the
    // normal 10k response window is still eligible and no matching-asset
    // HashSet or fixed-size payload scan is used.
    if !query.trim().is_empty() {
        if let Some(asset_results) =
            asset_text_search_authorized(op, ws_path, query, relation_scopes, limit, after).await?
        {
            for result in asset_results {
                if is_after_cursor(&result, after) {
                    results.insert((result.form.clone(), result.id.clone()), result);
                }
            }
        }
    }

    let mut results = results.into_values().collect::<Vec<_>>();
    results.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.form.cmp(&right.form))
    });
    results.truncate(limit);
    Ok(results)
}

fn is_after_cursor(result: &KeywordSearchResult, after: Option<(&str, &str, &str)>) -> bool {
    after.is_none_or(|(title, id, form)| {
        (
            result.title.as_str(),
            result.id.as_str(),
            result.form.as_str(),
        ) > (title, id, form)
    })
}

fn sql_string_literal(value: &str) -> String {
    // Ordinary DataFusion string literals preserve backslashes.  Doubling one
    // here would change cursor ordering for Entry IDs or titles containing a
    // backslash; only the SQL quote itself needs escaping.
    format!("'{}'", value.replace('\'', "''"))
}

async fn asset_text_search_authorized(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
    limit: usize,
    after: Option<(&str, &str, &str)>,
) -> Result<Option<Vec<KeywordSearchResult>>> {
    if limit == 0 {
        return Ok(Some(Vec::new()));
    }
    // The authorized-reference provider deliberately cannot push the AssetText
    // predicate down into the authoritative Form tables: doing so would make
    // authorization depend on derived data. Bound the remaining join with the
    // same DataFusion memory pool and timeout used by authorized SQL queries.
    let context =
        match crate::query_context::bounded_session_context(&ugoite_core::query::QueryLimits {
            max_memory_bytes: ASSET_TEXT_SEARCH_MAX_MEMORY_BYTES,
            max_rows: crate::MAX_NORMAL_READ_ROWS,
            timeout: ASSET_TEXT_SEARCH_TIMEOUT,
            max_concurrency: 1,
            allowed_functions: BTreeSet::from(["lower".to_string()]),
        }) {
            Ok(context) => context,
            Err(_) => return Ok(None),
        };
    let registered = match crate::derived_relation::register_asset_text_table(
        &context,
        op,
        ws_path,
        "__ugoite_internal_asset_text",
    )
    .await
    {
        Ok(registered) => registered,
        // AssetText is optional derived data. A missing, stale, corrupt, or
        // temporarily unavailable build must not make the authoritative typed
        // Entry search fail.
        Err(_) => return Ok(None),
    };
    if !registered {
        return Ok(None);
    }
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
        .replace('\'', "''");
    let after_predicate = after
        .map(|(title, entry_id, form)| {
            format!(
                " AND (e.title > {title} OR (e.title = {title} AND e.entry_id > {entry_id}) OR (e.title = {title} AND e.entry_id = {entry_id} AND e.form > {form}))",
                title = sql_string_literal(title),
                entry_id = sql_string_literal(entry_id),
                form = sql_string_literal(form),
            )
        })
        .unwrap_or_default();
    let sql = format!(
        "SELECT DISTINCT e.form, e.entry_id, e.title, e.created_at, e.updated_at FROM __ugoite_authorized_asset_refs e INNER JOIN __ugoite_internal_asset_text a ON e.asset_id = a.asset_id WHERE a.status = 'ready' AND a.text IS NOT NULL AND lower(a.text) LIKE lower('%{escaped}%') ESCAPE '\\'{after_predicate} ORDER BY e.title, e.entry_id, e.form LIMIT {limit}"
    );
    // The provider streams bounded authorization pages into one DataFusion
    // scan. This keeps the authorization source bounded per batch while the
    // AssetText provider is planned and scanned only once for the whole join.
    let form_names = match crate::entry::list_form_names(op, ws_path).await {
        Ok(form_names) => form_names,
        Err(_) => return Ok(None),
    };
    let asset_reference_fields = match load_asset_reference_fields(op, ws_path, &form_names).await {
        Ok(fields) => fields,
        Err(_) => return Ok(None),
    };
    if context
        .register_table(
            "__ugoite_authorized_asset_refs",
            Arc::new(AuthorizedAssetReferenceProvider::new(
                op.clone(),
                ws_path.to_string(),
                relation_scopes.clone(),
                form_names,
                asset_reference_fields,
            )),
        )
        .is_err()
    {
        return Ok(None);
    }
    let matches = match tokio::time::timeout(ASSET_TEXT_SEARCH_TIMEOUT, async {
        let frame = context.sql(&sql).await?;
        frame.collect().await
    })
    .await
    {
        Ok(Ok(matches)) => matches,
        // Derived provider/planning failures, memory exhaustion, and an
        // overlong authorized scan all degrade to the authoritative typed
        // search path. The DataFusion memory pool bounds sort/join state while
        // this timeout bounds total work even when the current Entry set is
        // much larger than the requested result page.
        _ => return Ok(None),
    };
    let mut results = BTreeMap::new();
    merge_asset_search_batches(&mut results, matches, after)?;
    let mut results = results.into_values().collect::<Vec<_>>();
    results.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
            .then_with(|| left.form.cmp(&right.form))
    });
    results.truncate(limit);
    Ok(Some(results))
}

fn merge_asset_search_batches(
    results: &mut BTreeMap<(String, String), KeywordSearchResult>,
    batches: Vec<RecordBatch>,
    after: Option<(&str, &str, &str)>,
) -> Result<()> {
    for batch in batches {
        let form = batch
            .column_by_name("form")
            .context("asset search join omitted form")?
            .as_any()
            .downcast_ref::<StringArray>()
            .context("asset search form has invalid type")?;
        let entry_id = batch
            .column_by_name("entry_id")
            .context("asset search join omitted entry_id")?
            .as_any()
            .downcast_ref::<StringArray>()
            .context("asset search entry_id has invalid type")?;
        let title = batch
            .column_by_name("title")
            .context("asset search join omitted title")?
            .as_any()
            .downcast_ref::<StringArray>()
            .context("asset search title has invalid type")?;
        let created = batch
            .column_by_name("created_at")
            .context("asset search join omitted created_at")?
            .as_any()
            .downcast_ref::<Float64Array>()
            .context("asset search created_at has invalid type")?;
        let updated = batch
            .column_by_name("updated_at")
            .context("asset search join omitted updated_at")?
            .as_any()
            .downcast_ref::<Float64Array>()
            .context("asset search updated_at has invalid type")?;
        for index in 0..batch.num_rows() {
            let result = KeywordSearchResult {
                id: entry_id.value(index).to_string(),
                title: title.value(index).to_string(),
                form: form.value(index).to_string(),
                created_at: created.value(index),
                updated_at: updated.value(index),
            };
            if is_after_cursor(&result, after) {
                results.insert((result.form.clone(), result.id.clone()), result);
            }
        }
    }
    Ok(())
}

fn authorized_asset_reference_batch(
    authorized_rows: &[(String, entry::EntryRow)],
    asset_reference_fields: &BTreeMap<String, Vec<AssetReferenceField>>,
) -> Result<RecordBatch> {
    let schema = authorized_asset_reference_schema();
    let mut forms = StringBuilder::new();
    let mut entry_ids = StringBuilder::new();
    let mut titles = StringBuilder::new();
    let mut created_at = Float64Builder::new();
    let mut updated_at = Float64Builder::new();
    let mut asset_ids = StringBuilder::new();
    for (form_name, row) in authorized_rows {
        if row.deleted {
            continue;
        }
        let mut ids = Vec::new();
        if let Some(fields) = asset_reference_fields.get(form_name) {
            let values = row.fields.as_object();
            for field in fields {
                let Some(value) = values.and_then(|values| values.get(&field.name)) else {
                    continue;
                };
                if field.list {
                    if let Value::Array(values) = value {
                        for value in values {
                            append_asset_reference(value, &mut ids);
                        }
                    }
                } else {
                    append_asset_reference(value, &mut ids);
                }
            }
        }
        ids.sort();
        ids.dedup();
        for asset_id in ids {
            forms.append_value(form_name);
            entry_ids.append_value(&row.entry_id);
            titles.append_value(&row.title);
            created_at.append_value(row.created_at);
            updated_at.append_value(row.updated_at);
            asset_ids.append_value(asset_id);
        }
    }
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(forms.finish()),
            Arc::new(entry_ids.finish()),
            Arc::new(titles.finish()),
            Arc::new(created_at.finish()),
            Arc::new(updated_at.finish()),
            Arc::new(asset_ids.finish()),
        ],
    )
    .context("build authorized AssetReference join page")
}

fn authorized_asset_reference_schema() -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("form", DataType::Utf8, false),
        Field::new("entry_id", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("created_at", DataType::Float64, false),
        Field::new("updated_at", DataType::Float64, false),
        Field::new("asset_id", DataType::Utf8, false),
    ]))
}

#[derive(Clone, Debug)]
struct AssetReferenceField {
    name: String,
    list: bool,
}

async fn load_asset_reference_fields(
    op: &Operator,
    ws_path: &str,
    form_names: &[String],
) -> Result<BTreeMap<String, Vec<AssetReferenceField>>> {
    let mut fields = BTreeMap::new();
    for form_name in form_names {
        let definition = crate::iceberg_store::load_domain_form(op, ws_path, form_name).await?;
        let asset_fields = definition
            .fields
            .iter()
            .filter_map(|field| match &field.field_type {
                FieldType::AssetReference => Some(AssetReferenceField {
                    name: field.name.clone(),
                    list: false,
                }),
                FieldType::List
                    if field
                        .list_item
                        .as_ref()
                        .is_some_and(|item| item.field_type == FieldType::AssetReference) =>
                {
                    Some(AssetReferenceField {
                        name: field.name.clone(),
                        list: true,
                    })
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        fields.insert(definition.name.to_string(), asset_fields);
    }
    Ok(fields)
}

fn append_asset_reference(value: &Value, output: &mut Vec<String>) {
    let Ok(reference) = serde_json::from_value::<AssetReference>(value.clone()) else {
        return;
    };
    output.push(reference.asset_id);
}

#[derive(Debug)]
struct AuthorizedAssetReferenceProvider {
    operator: Operator,
    workspace_path: String,
    relation_scopes: BTreeMap<String, EntryScope>,
    form_names: Vec<String>,
    asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
    schema: Arc<Schema>,
}

impl AuthorizedAssetReferenceProvider {
    fn new(
        operator: Operator,
        workspace_path: String,
        relation_scopes: BTreeMap<String, EntryScope>,
        form_names: Vec<String>,
        asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
    ) -> Self {
        Self {
            operator,
            workspace_path,
            relation_scopes,
            form_names,
            asset_reference_fields,
            schema: authorized_asset_reference_schema(),
        }
    }
}

#[async_trait]
impl TableProvider for AuthorizedAssetReferenceProvider {
    fn schema(&self) -> Arc<Schema> {
        self.schema.clone()
    }

    fn table_type(&self) -> TableType {
        TableType::Base
    }

    async fn scan(
        &self,
        _state: &dyn datafusion::catalog::Session,
        _projection: Option<&Vec<usize>>,
        _filters: &[Expr],
        _limit: Option<usize>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        Ok(Arc::new(AuthorizedAssetReferenceExec::new(
            self.operator.clone(),
            self.workspace_path.clone(),
            self.relation_scopes.clone(),
            self.form_names.clone(),
            self.asset_reference_fields.clone(),
            self.schema.clone(),
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
}

#[derive(Debug)]
struct AuthorizedAssetReferenceExec {
    operator: Operator,
    workspace_path: String,
    relation_scopes: BTreeMap<String, EntryScope>,
    form_names: Vec<String>,
    asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
    schema: Arc<Schema>,
    properties: Arc<PlanProperties>,
}

impl AuthorizedAssetReferenceExec {
    fn new(
        operator: Operator,
        workspace_path: String,
        relation_scopes: BTreeMap<String, EntryScope>,
        form_names: Vec<String>,
        asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
        schema: Arc<Schema>,
    ) -> Self {
        let properties = Arc::new(PlanProperties::new(
            EquivalenceProperties::new(schema.clone()),
            Partitioning::UnknownPartitioning(1),
            EmissionType::Incremental,
            Boundedness::Bounded,
        ));
        Self {
            operator,
            workspace_path,
            relation_scopes,
            form_names,
            asset_reference_fields,
            schema,
            properties,
        }
    }
}

#[derive(Debug)]
struct AuthorizedAssetReferenceStreamState {
    operator: Operator,
    workspace_path: String,
    relation_scopes: BTreeMap<String, EntryScope>,
    form_names: Vec<String>,
    asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
    form_index: usize,
    current_rows: Option<Vec<(String, entry::EntryRow)>>,
    current_offset: usize,
    current_after: Option<(String, String, String)>,
}

impl AuthorizedAssetReferenceStreamState {
    async fn next_batch(&mut self) -> DfResult<Option<RecordBatch>> {
        loop {
            if let Some(rows) = self.current_rows.as_ref() {
                if self.current_offset < rows.len() {
                    let start = self.current_offset;
                    let end = (start + ASSET_TEXT_SEARCH_PAGE_SIZE).min(rows.len());
                    let batch = authorized_asset_reference_batch(
                        &rows[start..end],
                        &self.asset_reference_fields,
                    )
                    .map_err(|error| {
                        datafusion::error::DataFusionError::Execution(error.to_string())
                    })?;
                    self.current_offset = end;
                    if batch.num_rows() == 0 {
                        continue;
                    }
                    return Ok(Some(batch));
                }
                if let Some((form_name, row)) = rows.last() {
                    self.current_after =
                        Some((row.title.clone(), row.entry_id.clone(), form_name.clone()));
                }
                self.current_rows = None;
                self.current_offset = 0;
                if self.current_after.is_none() {
                    self.form_index += 1;
                }
                continue;
            }
            let Some(form_name) = self.form_names.get(self.form_index).cloned() else {
                return Ok(None);
            };
            // Load one bounded keyset page. This keeps authorization work and
            // Rust allocations bounded even for very large Forms, without
            // the quadratic rescans of OFFSET pagination.
            let after = self
                .current_after
                .as_ref()
                .map(|(title, entry_id, form)| (title.as_str(), entry_id.as_str(), form.as_str()));
            let rows = crate::index::query_entry_rows_authorized_after(
                &self.operator,
                &self.workspace_path,
                &self.relation_scopes,
                &form_name,
                after,
                ASSET_TEXT_SEARCH_PAGE_SIZE,
            )
            .await
            .map_err(|error| datafusion::error::DataFusionError::Execution(error.to_string()))?;
            if rows.is_empty() {
                self.current_after = None;
                self.form_index += 1;
            } else {
                self.current_rows = Some(rows);
            }
        }
    }
}

impl ExecutionPlan for AuthorizedAssetReferenceExec {
    fn name(&self) -> &str {
        "AuthorizedAssetReferenceScan"
    }

    fn properties(&self) -> &Arc<PlanProperties> {
        &self.properties
    }

    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        Vec::new()
    }

    fn with_new_children(
        self: Arc<Self>,
        children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        if children.is_empty() {
            Ok(self)
        } else {
            Err(datafusion::error::DataFusionError::Internal(
                "authorized AssetReference scan is a leaf".to_string(),
            ))
        }
    }

    fn execute(
        &self,
        partition: usize,
        _context: Arc<TaskContext>,
    ) -> DfResult<SendableRecordBatchStream> {
        if partition != 0 {
            return Err(datafusion::error::DataFusionError::Execution(
                "authorized AssetReference scan has one partition".to_string(),
            ));
        }
        let state = AuthorizedAssetReferenceStreamState {
            operator: self.operator.clone(),
            workspace_path: self.workspace_path.clone(),
            relation_scopes: self.relation_scopes.clone(),
            form_names: self.form_names.clone(),
            asset_reference_fields: self.asset_reference_fields.clone(),
            form_index: 0,
            current_rows: None,
            current_offset: 0,
            current_after: None,
        };
        let schema = self.schema.clone();
        let stream = futures::stream::try_unfold(state, |mut state| async move {
            match state.next_batch().await? {
                Some(batch) => Ok(Some((batch, state))),
                None => Ok(None),
            }
        });
        Ok(Box::pin(RecordBatchStreamAdapter::new(schema, stream)))
    }
}

impl DisplayAs for AuthorizedAssetReferenceExec {
    fn fmt_as(
        &self,
        _format: DisplayFormatType,
        formatter: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        formatter.write_str("AuthorizedAssetReferenceScan")
    }
}
