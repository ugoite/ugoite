use anyhow::Result;
use opendal::Operator;
use ugoite_core::query::EntryScope;

use crate::entry;
pub use ugoite_domain::search::KeywordSearchResult;

/// Hybrid keyword search using index and content fallback.
pub async fn search_entries(
    op: &Operator,
    ws_path: &str,
    query: &str,
) -> Result<Vec<KeywordSearchResult>> {
    let query = query.to_lowercase();
    let rows = entry::list_entry_rows(op, ws_path).await?;
    let mut results = Vec::new();
    for (_form_name, row) in rows {
        if row.deleted {
            continue;
        }
        if entry::row_contains_query(&row, &query) {
            results.push(KeywordSearchResult {
                id: row.entry_id,
                title: row.title,
                form: row.form,
                created_at: row.created_at,
                updated_at: row.updated_at,
            });
        }
    }
    Ok(results)
}

/// Searches the current, already-authorized typed Entry rows. The DataFusion
/// latest-state plan and Entry scope run before this small bounded text
/// predicate; search never serializes a revision or uses a history fallback.
pub async fn search_entries_with_scopes(
    op: &Operator,
    ws_path: &str,
    query: &str,
    relation_scopes: &std::collections::BTreeMap<String, EntryScope>,
) -> Result<Vec<KeywordSearchResult>> {
    let query = query.trim().to_lowercase();
    let mut results = Vec::new();
    for (form_name, row) in entry::list_entry_rows_authorized(op, ws_path, relation_scopes).await? {
        if row.deleted || !entry::row_contains_query(&row, &query) {
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
