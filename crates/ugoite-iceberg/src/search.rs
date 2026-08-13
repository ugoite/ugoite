use anyhow::{Context, Result};
use arrow_array::builder::{Float64Builder, StringBuilder};
use arrow_array::{Array, Float64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use datafusion::datasource::MemTable;
use datafusion::prelude::SessionContext;
use opendal::Operator;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use ugoite_core::query::EntryScope;

use crate::entry;
pub use ugoite_domain::search::KeywordSearchResult;

const ASSET_TEXT_SEARCH_PAGE_SIZE: usize = 2_048;

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
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
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
    let context = SessionContext::new();
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
    let mut authorized_batches = Vec::new();
    // Scope keys are normalized Form or relation names, while the provider
    // query's form filter must use the authoritative display name.
    for form_name in crate::entry::list_form_names(op, ws_path).await? {
        let mut offset = 0;
        loop {
            let authorized_rows = crate::index::query_entry_rows_authorized_page(
                op,
                ws_path,
                relation_scopes,
                &form_name,
                ASSET_TEXT_SEARCH_PAGE_SIZE,
                offset,
            )
            .await?;
            if authorized_rows.is_empty() {
                break;
            }
            let batch = authorized_asset_reference_batch(&authorized_rows)?;
            if batch.num_rows() > 0 {
                authorized_batches.push(batch);
            }
            let page_len = authorized_rows.len();
            offset = offset.saturating_add(page_len);
            if page_len < ASSET_TEXT_SEARCH_PAGE_SIZE {
                break;
            }
        }
    }
    if authorized_batches.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let schema = authorized_batches[0].schema();
    context.deregister_table("__ugoite_authorized_asset_refs")?;
    context.register_table(
        "__ugoite_authorized_asset_refs",
        Arc::new(MemTable::try_new(schema, vec![authorized_batches])?),
    )?;
    let frame = match context.sql(&sql).await {
        Ok(frame) => frame,
        // Derived provider/planning failures degrade to the authoritative
        // typed search path. Errors while reading authorized Entry pages above
        // are intentionally still propagated.
        Err(_) => return Ok(None),
    };
    let batches = match frame.collect().await {
        Ok(batches) => batches,
        Err(_) => return Ok(None),
    };
    let mut results = Vec::new();
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
                results.push(result);
            }
        }
    }
    Ok(Some(results))
}

fn authorized_asset_reference_batch(
    authorized_rows: &[(String, entry::EntryRow)],
) -> Result<RecordBatch> {
    let schema = Arc::new(Schema::new(vec![
        Field::new("form", DataType::Utf8, false),
        Field::new("entry_id", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("created_at", DataType::Float64, false),
        Field::new("updated_at", DataType::Float64, false),
        Field::new("asset_id", DataType::Utf8, false),
    ]));
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
        collect_asset_ids(&row.fields, &mut ids);
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

fn collect_asset_ids(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_asset_ids(value, output)),
        Value::Object(object) => {
            if let Some(asset_id) = object.get("asset_id").and_then(Value::as_str) {
                output.push(asset_id.to_string());
            } else {
                object
                    .values()
                    .for_each(|value| collect_asset_ids(value, output));
            }
        }
        _ => {}
    }
}
