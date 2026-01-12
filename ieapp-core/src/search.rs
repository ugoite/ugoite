use anyhow::Result;
use opendal::Operator;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchResult {
    pub id: String,
}

/// Hybrid keyword search using index and content fallback.
pub async fn search_notes(op: &Operator, ws_path: &str, query: &str) -> Result<Vec<SearchResult>> {
    let query = query.to_lowercase();
    let mut found_ids = HashSet::new();

    // 1. Inverted Index (REQ-SRCH-001)
    let inverted_path = format!("{}/index/inverted_index.json", ws_path);
    if op.exists(&inverted_path).await? {
        let bytes = op.read(&inverted_path).await?;
        if let Ok(inverted) =
            serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&bytes.to_vec())
        {
            for (term, ids_val) in inverted {
                if term.contains(&query) {
                    if let Some(ids) = ids_val.as_array() {
                        for id_val in ids {
                            if let Some(id) = id_val.as_str() {
                                found_ids.insert(id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Fallback: Scan all notes (REQ-SRCH-002) if no results found via index
    if found_ids.is_empty() {
        let notes_dir = format!("{}/notes/", ws_path);
        if op.exists(&notes_dir).await? {
            let ds = op.list(&notes_dir).await?;
            for entry in ds {
                let path = entry.path();
                if path.ends_with('/') {
                    let content_path = format!("{}content.json", path);
                    if op.exists(&content_path).await? {
                        let bytes = op.read(&content_path).await?;
                        if let Ok(content_json) =
                            serde_json::from_slice::<serde_json::Value>(&bytes.to_vec())
                        {
                            // Simple check: json dump contains query?
                            let dump = serde_json::to_string(&content_json)?.to_lowercase();
                            if dump.contains(&query) {
                                // Extract note_id from path: workspaces/ws/notes/{id}/
                                let parts: Vec<&str> =
                                    path.trim_end_matches('/').split('/').collect();
                                if let Some(id) = parts.last() {
                                    found_ids.insert(id.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let results = found_ids
        .into_iter()
        .map(|id| SearchResult { id })
        .collect();
    Ok(results)
}
