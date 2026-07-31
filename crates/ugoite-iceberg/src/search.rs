use anyhow::Result;
use opendal::Operator;
use std::collections::HashSet;

use crate::entry;
pub use ugoite_domain::search::SearchResult;

/// Hybrid keyword search using index and content fallback.
pub async fn search_entries(
    op: &Operator,
    ws_path: &str,
    query: &str,
) -> Result<Vec<SearchResult>> {
    search_entries_with_authorized_ids(op, ws_path, query, None).await
}

pub async fn search_entries_authorized(
    op: &Operator,
    ws_path: &str,
    query: &str,
    readable_entry_ids: &HashSet<String>,
) -> Result<Vec<SearchResult>> {
    search_entries_with_authorized_ids(op, ws_path, query, Some(readable_entry_ids)).await
}

async fn search_entries_with_authorized_ids(
    op: &Operator,
    ws_path: &str,
    query: &str,
    readable_entry_ids: Option<&HashSet<String>>,
) -> Result<Vec<SearchResult>> {
    let query = query.to_lowercase();
    let rows = entry::list_entry_rows(op, ws_path).await?;
    let mut results = Vec::new();
    for (_form_name, row) in rows {
        if row.deleted || readable_entry_ids.is_some_and(|allowed| !allowed.contains(&row.entry_id))
        {
            continue;
        }
        let dump = serde_json::to_string(&row)?.to_lowercase();
        if dump.contains(&query) {
            results.push(SearchResult {
                id: row.entry_id,
                title: row.title,
                form: row.form,
                created_at: row.created_at,
                updated_at: row.updated_at,
                properties: entry::merge_entry_fields(&row.fields, &row.extra_attributes),
                tags: row.tags,
                links: row.links,
                assets: row.assets,
                checksum: row.integrity.checksum,
            });
        }
    }
    Ok(results)
}
