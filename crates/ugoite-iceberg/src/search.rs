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
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use ugoite_core::query::EntryScope;
use ugoite_domain::entry::AssetReference;

use crate::entry;
use crate::index::AuthorizedAssetReferenceRow;
pub use ugoite_domain::search::KeywordSearchResult;

const ASSET_TEXT_SEARCH_PAGE_SIZE: usize = 2_048;
// This is an internal maintenance/search bound rather than the public Entry
// response ceiling. The bounded DataFusion memory pool and timeout remain the
// actual protection for unusually large current Forms.
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
    match tokio::time::timeout(
        ASSET_TEXT_SEARCH_TIMEOUT,
        asset_text_search_authorized_inner(op, ws_path, query, relation_scopes, limit, after),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Ok(None),
    }
}

async fn asset_text_search_authorized_inner(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
    limit: usize,
    after: Option<(&str, &str, &str)>,
) -> Result<Option<Vec<KeywordSearchResult>>> {
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
    let (authorized_context, authorized_forms) =
        match crate::index::authorized_asset_reference_query_context(op, ws_path, relation_scopes)
            .await
        {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
    // Use the same stable Form snapshot returned with the authorized context
    // for both schema selection and AssetReference extraction. The helper
    // rejects a definition change observed across context construction, so a
    // scalar/list edit cannot silently drop references from this join.
    let asset_reference_fields = load_asset_reference_fields(&authorized_forms)?;
    // Do not make the provider walk current rows for Forms that cannot emit an
    // AssetReference or are absent from the authorization context. This keeps
    // the derived join both authorization-safe and proportional to the
    // authorized AssetReference-bearing Forms, rather than every Form in the
    // Space.
    let asset_form_names = authorized_forms
        .keys()
        .filter(|&form_name| {
            let Some(fields) = asset_reference_fields.get(form_name) else {
                return false;
            };
            if fields.is_empty() {
                return false;
            }
            let Some(form) = authorized_forms.get(form_name) else {
                return false;
            };
            let relation = form.get("sql_relation").and_then(Value::as_str);
            relation_scopes.contains_key(&form_name.to_ascii_lowercase())
                || relation.is_some_and(|relation| {
                    relation_scopes.contains_key(&relation.to_ascii_lowercase())
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    if asset_form_names.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let authorized_context = Arc::new(authorized_context);
    let authorized_forms = Arc::new(authorized_forms);
    if context
        .register_table(
            "__ugoite_authorized_asset_refs",
            Arc::new(AuthorizedAssetReferenceProvider::new(
                op.clone(),
                ws_path.to_string(),
                relation_scopes.clone(),
                asset_form_names,
                asset_reference_fields,
                authorized_context,
                authorized_forms,
                after,
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
        Ok(Err(_)) => return Ok(None),
        Err(_) => return Ok(None),
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
    authorized_rows: &[(String, AuthorizedAssetReferenceRow)],
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

fn load_asset_reference_fields(
    forms: &HashMap<String, Value>,
) -> Result<BTreeMap<String, Vec<AssetReferenceField>>> {
    let mut fields = BTreeMap::new();
    for (form_name, definition) in forms {
        let asset_fields = definition
            .get("fields")
            .and_then(Value::as_object)
            .map(|form_fields| {
                form_fields
                    .iter()
                    .filter_map(|(field_name, field)| {
                        let field_type = field.get("type").and_then(Value::as_str)?;
                        if field_type == "asset_reference" {
                            return Some(AssetReferenceField {
                                name: field_name.clone(),
                                list: false,
                            });
                        }
                        if field_type == "list"
                            && field
                                .get("items")
                                .and_then(Value::as_object)
                                .and_then(|items| items.get("type"))
                                .and_then(Value::as_str)
                                == Some("asset_reference")
                        {
                            return Some(AssetReferenceField {
                                name: field_name.clone(),
                                list: true,
                            });
                        }
                        None
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        fields.insert(form_name.clone(), asset_fields);
    }
    Ok(fields)
}

fn append_asset_reference(value: &Value, output: &mut Vec<String>) {
    let Ok(reference) = serde_json::from_value::<AssetReference>(value.clone()) else {
        return;
    };
    output.push(reference.asset_id);
}

struct AuthorizedAssetReferenceProvider {
    operator: Operator,
    workspace_path: String,
    relation_scopes: BTreeMap<String, EntryScope>,
    form_names: Vec<String>,
    asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
    authorized_context: Arc<crate::query_context::AuthorizedQueryContext>,
    authorized_forms: Arc<HashMap<String, Value>>,
    initial_after: Option<(String, String, String)>,
    schema: Arc<Schema>,
}

impl fmt::Debug for AuthorizedAssetReferenceProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedAssetReferenceProvider")
            .field("operator", &self.operator)
            .field("workspace_path", &self.workspace_path)
            .field("relation_scopes", &self.relation_scopes)
            .field("form_names", &self.form_names)
            .field("asset_reference_fields", &self.asset_reference_fields)
            .field("initial_after", &self.initial_after)
            .field("schema", &self.schema)
            .finish()
    }
}

impl AuthorizedAssetReferenceProvider {
    #[allow(clippy::too_many_arguments)]
    fn new(
        operator: Operator,
        workspace_path: String,
        relation_scopes: BTreeMap<String, EntryScope>,
        form_names: Vec<String>,
        asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
        authorized_context: Arc<crate::query_context::AuthorizedQueryContext>,
        authorized_forms: Arc<HashMap<String, Value>>,
        after: Option<(&str, &str, &str)>,
    ) -> Self {
        Self {
            operator,
            workspace_path,
            relation_scopes,
            form_names,
            asset_reference_fields,
            authorized_context,
            authorized_forms,
            initial_after: after.map(|(title, entry_id, form)| {
                (title.to_string(), entry_id.to_string(), form.to_string())
            }),
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
            self.authorized_context.clone(),
            self.authorized_forms.clone(),
            self.initial_after.clone(),
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

struct AuthorizedAssetReferenceExec {
    operator: Operator,
    workspace_path: String,
    relation_scopes: BTreeMap<String, EntryScope>,
    form_names: Vec<String>,
    asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
    authorized_context: Arc<crate::query_context::AuthorizedQueryContext>,
    authorized_forms: Arc<HashMap<String, Value>>,
    initial_after: Option<(String, String, String)>,
    schema: Arc<Schema>,
    properties: Arc<PlanProperties>,
}

impl fmt::Debug for AuthorizedAssetReferenceExec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AuthorizedAssetReferenceExec")
            .field("operator", &self.operator)
            .field("workspace_path", &self.workspace_path)
            .field("relation_scopes", &self.relation_scopes)
            .field("form_names", &self.form_names)
            .field("asset_reference_fields", &self.asset_reference_fields)
            .field("initial_after", &self.initial_after)
            .field("schema", &self.schema)
            .field("properties", &self.properties)
            .finish()
    }
}

impl AuthorizedAssetReferenceExec {
    #[allow(clippy::too_many_arguments)]
    fn new(
        operator: Operator,
        workspace_path: String,
        relation_scopes: BTreeMap<String, EntryScope>,
        form_names: Vec<String>,
        asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
        authorized_context: Arc<crate::query_context::AuthorizedQueryContext>,
        authorized_forms: Arc<HashMap<String, Value>>,
        initial_after: Option<(String, String, String)>,
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
            authorized_context,
            authorized_forms,
            initial_after,
            schema,
            properties,
        }
    }
}

struct AuthorizedAssetReferenceStreamState {
    form_names: Vec<String>,
    asset_reference_fields: BTreeMap<String, Vec<AssetReferenceField>>,
    authorized_context: Arc<crate::query_context::AuthorizedQueryContext>,
    authorized_forms: Arc<HashMap<String, Value>>,
    initial_after: Option<(String, String, String)>,
    form_index: usize,
    after_entry_id: Option<String>,
    current_page_complete: bool,
    current_rows: Option<Vec<(String, AuthorizedAssetReferenceRow)>>,
    current_offset: usize,
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
                self.current_rows = None;
                self.current_offset = 0;
                if self.current_page_complete {
                    self.form_index += 1;
                    self.after_entry_id = None;
                    self.current_page_complete = false;
                }
                continue;
            }
            let form_index = self.form_index;
            let Some(form_name) = self.form_names.get(form_index).cloned() else {
                return Ok(None);
            };
            let asset_field_names = self
                .asset_reference_fields
                .get(&form_name)
                .into_iter()
                .flatten()
                .map(|field| field.name.clone())
                .collect::<BTreeSet<_>>();
            let rows = crate::index::query_asset_reference_rows_authorized_in_context(
                &self.authorized_context,
                &self.authorized_forms,
                &form_name,
                &asset_field_names,
                self.after_entry_id.as_deref(),
                ASSET_TEXT_SEARCH_PAGE_SIZE,
            )
            .await
            .map_err(|error| datafusion::error::DataFusionError::Execution(error.to_string()))?;
            if rows.is_empty() {
                self.form_index += 1;
                self.after_entry_id = None;
                continue;
            }
            self.current_page_complete = rows.len() < ASSET_TEXT_SEARCH_PAGE_SIZE;
            self.after_entry_id = rows.last().map(|row| row.entry_id.clone());
            let rows = rows
                .into_iter()
                .filter(|row| {
                    self.initial_after
                        .as_ref()
                        .is_none_or(|(title, entry_id, form)| {
                            (
                                row.title.as_str(),
                                row.entry_id.as_str(),
                                form_name.as_str(),
                            ) > (title.as_str(), entry_id.as_str(), form.as_str())
                        })
                })
                .map(|row| (form_name.clone(), row))
                .collect::<Vec<_>>();
            if rows.is_empty() {
                if self.current_page_complete {
                    self.form_index += 1;
                    self.after_entry_id = None;
                    self.current_page_complete = false;
                }
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
            form_names: self.form_names.clone(),
            asset_reference_fields: self.asset_reference_fields.clone(),
            authorized_context: self.authorized_context.clone(),
            authorized_forms: self.authorized_forms.clone(),
            initial_after: self.initial_after.clone(),
            form_index: 0,
            after_entry_id: None,
            current_page_complete: false,
            current_rows: None,
            current_offset: 0,
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
