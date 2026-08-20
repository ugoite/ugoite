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
use futures::TryStreamExt;
use opendal::Operator;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use ugoite_core::query::EntryScope;
use ugoite_domain::entry::AssetReference;
use uuid::Uuid;

use crate::authorization::{
    effective_actions_for_state, AuthorizationState, ResourceKind, ResourceRef,
};
use crate::entry;
use crate::index::AuthorizedAssetReferenceRow;
pub use ugoite_domain::search::KeywordSearchResult;

const ASSET_TEXT_SEARCH_PAGE_SIZE: usize = 2_048;
// This is an internal maintenance/search bound rather than the public Entry
// response ceiling. The bounded DataFusion memory pool and timeout remain the
// actual protection for unusually large current Forms.
const ASSET_TEXT_SEARCH_MAX_MEMORY_BYTES: usize = 64 * 1024 * 1024;
const ASSET_TEXT_SEARCH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct AssetAuthorization {
    state: Arc<AuthorizationState>,
    principal_ids: Arc<Vec<Uuid>>,
}

impl AssetAuthorization {
    pub(crate) fn new(state: AuthorizationState, principal_ids: &[Uuid]) -> Self {
        Self {
            state: Arc::new(state),
            principal_ids: Arc::new(principal_ids.to_vec()),
        }
    }

    fn allows(&self, entry_id: &str, asset_id: &str) -> Result<bool> {
        let parent = ResourceRef {
            kind: ResourceKind::Entry,
            id: entry_id.to_string(),
            parent: None,
        };
        let asset = ResourceRef {
            kind: ResourceKind::Asset,
            id: asset_id.to_string(),
            parent: Some(Box::new(parent)),
        };
        self.principal_ids
            .iter()
            .try_fold(true, |allowed, principal_id| {
                if !allowed {
                    return Ok(false);
                }
                Ok(
                    effective_actions_for_state(&self.state, *principal_id, Some(&asset))?
                        .contains(&ugoite_domain::identity::Action::Read),
                )
            })
    }
}

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
    search_entries_with_scopes_after_authorized(
        op,
        ws_path,
        query,
        relation_scopes,
        limit,
        after,
        None,
    )
    .await
}

pub(crate) async fn search_entries_with_scopes_after_authorized(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &std::collections::BTreeMap<String, EntryScope>,
    limit: usize,
    after: Option<(&str, &str, &str)>,
    asset_authorization: Option<AssetAuthorization>,
) -> Result<Vec<KeywordSearchResult>> {
    if query.len() > crate::index::ASSET_TEXT_SEARCH_MAX_QUERY_BYTES {
        anyhow::bail!("AssetText search query exceeds the configured byte limit");
    }
    if limit > crate::MAX_NORMAL_READ_ROWS {
        anyhow::bail!(
            "AssetText search result limit exceeds {} rows",
            crate::MAX_NORMAL_READ_ROWS
        );
    }
    let result_budget = crate::index::AssetTextSearchBudget::new();
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
        result_budget.reserve(result.id.len() + result.title.len() + result.form.len())?;
        results.insert((result.form.clone(), result.id.clone()), result);
    }

    // AssetText is joined only after the provider-side authorized current
    // Entry scan. The join itself runs in DataFusion, so a match after the
    // normal 10k response window is still eligible and no matching-asset
    // HashSet or fixed-size payload scan is used.
    if !query.trim().is_empty() {
        if let Some(asset_results) = asset_text_search_authorized(
            op,
            ws_path,
            query,
            relation_scopes,
            limit,
            after,
            asset_authorization,
            result_budget.clone(),
        )
        .await?
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
    asset_authorization: Option<AssetAuthorization>,
    budget: crate::index::AssetTextSearchBudget,
) -> Result<Option<Vec<KeywordSearchResult>>> {
    if limit == 0 {
        return Ok(Some(Vec::new()));
    }
    let budget_checkpoint = budget.checkpoint();
    match tokio::time::timeout(
        ASSET_TEXT_SEARCH_TIMEOUT,
        asset_text_search_authorized_inner(
            op,
            ws_path,
            query,
            relation_scopes,
            limit,
            after,
            asset_authorization,
            budget.clone(),
        ),
    )
    .await
    {
        Ok(result) => {
            if result.is_err() {
                budget.restore(budget_checkpoint);
            }
            result
        }
        Err(_) => {
            budget.restore(budget_checkpoint);
            // The caller already has the authorized native Entry candidates.
            // AssetText is an optimization, so a slow derived join must leave
            // those valid results available instead of turning search into an
            // outage or returning an incomplete derived result set.
            Ok(None)
        }
    }
}

async fn asset_text_search_authorized_inner(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
    limit: usize,
    after: Option<(&str, &str, &str)>,
    asset_authorization: Option<AssetAuthorization>,
    budget: crate::index::AssetTextSearchBudget,
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
    let fallback_form_names = asset_form_names.clone();
    let fallback_asset_reference_fields = asset_reference_fields.clone();
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
                authorized_context.clone(),
                authorized_forms.clone(),
                after,
                asset_authorization.clone(),
                budget.clone(),
            )),
        )
        .is_err()
    {
        return Ok(None);
    }
    let budget_checkpoint = budget.checkpoint();
    let matches = match tokio::time::timeout(ASSET_TEXT_SEARCH_TIMEOUT, async {
        let frame = context.sql(&sql).await?;
        let mut stream = frame.execute_stream().await?;
        let mut results = BTreeMap::new();
        while let Some(batch) = stream.try_next().await? {
            // Do not retain an arbitrarily large DataFusion output batch while
            // converting its strings into owned search results. The shared
            // budget accounts for retained result strings below.
            if batch.get_array_memory_size() > crate::index::ASSET_TEXT_SEARCH_MAX_BYTES {
                return Err(anyhow::anyhow!(
                    "AssetText search batch exceeds the byte limit"
                ));
            }
            merge_asset_search_batches(&mut results, vec![batch], after, &budget)?;
        }
        Ok::<_, anyhow::Error>(results)
    })
    .await
    {
        Ok(Ok(matches)) => matches,
        // Derived provider/planning failures, memory exhaustion, and an
        // overlong authorized scan all degrade to the authoritative typed
        // current-state scan. A failed join attempt must not consume the
        // fallback's result budget.
        Ok(Err(_)) | Err(_) => {
            budget.restore(budget_checkpoint);
            // A single Entry may contain more AssetReferences than the
            // DataFusion join page intentionally accepts. Do not degrade to
            // typed-field search (which would silently omit a valid hit).
            return fallback_asset_text_search(
                op,
                ws_path,
                &authorized_context,
                &authorized_forms,
                &fallback_form_names,
                &fallback_asset_reference_fields,
                query,
                limit,
                after,
                asset_authorization,
                budget.clone(),
            )
            .await;
        }
    };
    let results = matches;
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
    budget: &crate::index::AssetTextSearchBudget,
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
            let id = entry_id.value(index);
            let title = title.value(index);
            let form = form.value(index);
            budget.reserve(id.len() + title.len() + form.len())?;
            let result = KeywordSearchResult {
                id: id.to_string(),
                title: title.to_string(),
                form: form.to_string(),
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

#[allow(clippy::too_many_arguments)]
async fn fallback_asset_text_search(
    op: &Operator,
    ws_path: &str,
    authorized_context: &Arc<crate::query_context::AuthorizedQueryContext>,
    authorized_forms: &Arc<HashMap<String, Value>>,
    form_names: &[String],
    asset_reference_fields: &BTreeMap<String, Vec<AssetReferenceField>>,
    query: &str,
    limit: usize,
    after: Option<(&str, &str, &str)>,
    asset_authorization: Option<AssetAuthorization>,
    budget: crate::index::AssetTextSearchBudget,
) -> Result<Option<Vec<KeywordSearchResult>>> {
    let Some(matching_assets) =
        crate::derived_relation::asset_text_search_matches(op, ws_path, query).await?
    else {
        return Ok(None);
    };
    let mut results = Vec::new();
    let mut seen = HashSet::new();
    for form_name in form_names {
        let field_names = asset_reference_fields
            .get(form_name)
            .into_iter()
            .flatten()
            .map(|field| field.name.clone())
            .collect::<BTreeSet<_>>();
        if field_names.is_empty() {
            continue;
        }
        let mut after_entry_id = None;
        loop {
            let rows = crate::index::query_asset_reference_rows_authorized_in_context(
                authorized_context,
                authorized_forms,
                form_name,
                &field_names,
                after_entry_id.as_deref(),
                ASSET_TEXT_SEARCH_PAGE_SIZE,
                &budget,
            )
            .await?;
            if rows.is_empty() {
                break;
            }
            let page_complete = rows.len() < ASSET_TEXT_SEARCH_PAGE_SIZE;
            after_entry_id = rows.last().map(|row| row.entry_id.clone());
            for row in rows {
                if row.deleted
                    || !is_after_cursor(
                        &KeywordSearchResult {
                            id: row.entry_id.clone(),
                            title: row.title.clone(),
                            form: form_name.clone(),
                            created_at: row.created_at,
                            updated_at: row.updated_at,
                        },
                        after,
                    )
                    || !row_references_matching_asset(
                        &row.fields,
                        asset_reference_fields.get(form_name).into_iter().flatten(),
                        &matching_assets,
                        &row.entry_id,
                        asset_authorization.as_ref(),
                    )
                {
                    continue;
                }
                let result = KeywordSearchResult {
                    id: row.entry_id,
                    title: row.title,
                    form: form_name.clone(),
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                };
                let key = (result.form.clone(), result.id.clone());
                if seen.insert(key) {
                    budget.reserve(result.id.len() + result.title.len() + result.form.len())?;
                    results.push(result);
                    results.sort_by(|left, right| {
                        left.title
                            .cmp(&right.title)
                            .then_with(|| left.id.cmp(&right.id))
                            .then_with(|| left.form.cmp(&right.form))
                    });
                    results.truncate(limit);
                }
            }
            if page_complete {
                break;
            }
        }
    }
    Ok(Some(results))
}

fn row_references_matching_asset<'a>(
    fields: &Value,
    asset_fields: impl IntoIterator<Item = &'a AssetReferenceField>,
    matching_assets: &HashSet<String>,
    entry_id: &str,
    asset_authorization: Option<&AssetAuthorization>,
) -> bool {
    let Some(fields) = fields.as_object() else {
        return false;
    };
    asset_fields.into_iter().any(|field| {
        fields.get(&field.name).is_some_and(|value| {
            value_contains_matching_asset(value, matching_assets, entry_id, asset_authorization)
        })
    })
}

fn value_contains_matching_asset(
    value: &Value,
    matching_assets: &HashSet<String>,
    entry_id: &str,
    asset_authorization: Option<&AssetAuthorization>,
) -> bool {
    match value {
        Value::Array(values) => values.iter().any(|value| {
            value_contains_matching_asset(value, matching_assets, entry_id, asset_authorization)
        }),
        Value::Object(object) => {
            object
                .get("asset_id")
                .and_then(Value::as_str)
                .is_some_and(|asset_id| {
                    matching_assets.contains(asset_id)
                        && asset_authorization.is_none_or(|authorization| {
                            authorization.allows(entry_id, asset_id).unwrap_or(false)
                        })
                })
                || object.values().any(|value| {
                    value_contains_matching_asset(
                        value,
                        matching_assets,
                        entry_id,
                        asset_authorization,
                    )
                })
        }
        _ => false,
    }
}

fn authorized_asset_reference_batch(
    authorized_rows: &[(String, AuthorizedAssetReferenceRow)],
    asset_reference_fields: &BTreeMap<String, Vec<AssetReferenceField>>,
    asset_authorization: Option<&AssetAuthorization>,
    budget: &crate::index::AssetTextSearchBudget,
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
                            append_asset_reference(
                                value,
                                &mut ids,
                                &row.entry_id,
                                asset_authorization,
                            )?;
                        }
                    }
                } else {
                    append_asset_reference(value, &mut ids, &row.entry_id, asset_authorization)?;
                }
            }
        }
        ids.sort();
        ids.dedup();
        for asset_id in ids {
            budget.reserve(
                form_name.len()
                    + row.entry_id.len()
                    + row.title.len()
                    + asset_id.len()
                    + std::mem::size_of::<AuthorizedAssetReferenceRow>(),
            )?;
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

fn append_asset_reference(
    value: &Value,
    output: &mut Vec<String>,
    entry_id: &str,
    asset_authorization: Option<&AssetAuthorization>,
) -> Result<()> {
    let Ok(reference) = serde_json::from_value::<AssetReference>(value.clone()) else {
        return Ok(());
    };
    reference
        .validate()
        .map_err(|error| anyhow::anyhow!("invalid persisted AssetReference: {error}"))?;
    if asset_authorization.is_some_and(|authorization| {
        !authorization
            .allows(entry_id, &reference.asset_id)
            .unwrap_or(false)
    }) {
        return Ok(());
    }
    if output.len() >= crate::index::MAX_ASSET_REFERENCES_PER_ENTRY {
        anyhow::bail!(
            "authorized Entry contains more than {} AssetReferences",
            crate::index::MAX_ASSET_REFERENCES_PER_ENTRY
        );
    }
    output.push(reference.asset_id);
    Ok(())
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
    asset_authorization: Option<AssetAuthorization>,
    budget: crate::index::AssetTextSearchBudget,
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
        asset_authorization: Option<AssetAuthorization>,
        budget: crate::index::AssetTextSearchBudget,
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
            asset_authorization,
            budget,
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
            self.asset_authorization.clone(),
            self.budget.clone(),
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
    asset_authorization: Option<AssetAuthorization>,
    budget: crate::index::AssetTextSearchBudget,
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
        asset_authorization: Option<AssetAuthorization>,
        budget: crate::index::AssetTextSearchBudget,
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
            asset_authorization,
            budget,
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
    asset_authorization: Option<AssetAuthorization>,
    budget: crate::index::AssetTextSearchBudget,
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
                        self.asset_authorization.as_ref(),
                        &self.budget,
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
                &self.budget,
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
            asset_authorization: self.asset_authorization.clone(),
            budget: self.budget.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn direct_asset_search_rejects_an_oversized_query_before_storage_access() {
        let operator =
            opendal::Operator::new(opendal::services::Memory::default()).expect("memory operator");
        let error = search_entries_with_scopes(
            &operator,
            "spaces/missing",
            &"x".repeat(crate::index::ASSET_TEXT_SEARCH_MAX_QUERY_BYTES + 1),
            &BTreeMap::new(),
            10,
        )
        .await
        .expect_err("oversized direct query must be rejected");
        assert!(error.to_string().contains("query exceeds"));
    }

    #[test]
    fn asset_search_budget_rejects_bytes_beyond_the_shared_limit() {
        let budget = crate::index::AssetTextSearchBudget::new();
        budget
            .reserve(crate::index::ASSET_TEXT_SEARCH_MAX_BYTES)
            .expect("limit-sized result is accepted");
        let error = budget
            .reserve(1)
            .expect_err("result bytes beyond the limit must fail");
        assert!(error.to_string().contains("byte limit"));
    }
}
