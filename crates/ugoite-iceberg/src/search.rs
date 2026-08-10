use anyhow::Result;
use opendal::Operator;
use ugoite_core::query::EntryScope;

use crate::entry;
pub use ugoite_domain::search::KeywordSearchResult;

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
    let rows = crate::index::query_entry_rows_authorized(
        op,
        ws_path,
        relation_scopes,
        None,
        Some(query),
        limit,
        0,
    )
    .await?;
    let mut results = Vec::with_capacity(rows.len());
    for (form_name, row) in rows {
        if row.deleted {
            continue;
        }
        results.push(KeywordSearchResult {
            id: row.entry_id,
            title: row.title,
            form: form_name,
            created_at: row.created_at,
            updated_at: row.updated_at,
        });
    }
    Ok(results)
}
