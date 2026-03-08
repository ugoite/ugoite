use anyhow::Result;
use opendal::Operator;
use std::collections::HashSet;

use crate::entry;
pub use ugoite_minimum::search::SearchResult;

/// Hybrid keyword search using index and content fallback.
pub async fn search_entries(
    op: &Operator,
    ws_path: &str,
    query: &str,
) -> Result<Vec<SearchResult>> {
    let query = query.to_lowercase();
    let mut found_ids = HashSet::new();

    let rows = entry::list_entry_rows(op, ws_path).await?;
    for (_form_name, row) in rows {
        if row.deleted {
            continue;
        }
        let dump = serde_json::to_string(&row)?.to_lowercase();
        if dump.contains(&query) {
            found_ids.insert(row.entry_id);
        }
    }

    let results = found_ids
        .into_iter()
        .map(|id| SearchResult { id })
        .collect();
    Ok(results)
}
