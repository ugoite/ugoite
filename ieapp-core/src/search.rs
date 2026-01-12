use anyhow::Result;
use opendal::Operator;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchResult {
    pub id: String,
}

/// Hybrid keyword search using index and content fallback.
pub async fn search_notes(op: &Operator, ws_path: &str, query: &str) -> Result<Vec<SearchResult>> {
    let query = query.to_lowercase();
    let mut results = Vec::new();

    // TODO: Use index/inverted_index if available (REQ-SRCH-001)

    // Fallback: Scan all notes (REQ-SRCH-002)
    let notes_dir = format!("{}/notes/", ws_path);
    if !op.exists(&notes_dir).await? {
        return Ok(Vec::new());
    }

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
                    // Python logic: if token in json.dumps(content_json).lower()
                    let dump = serde_json::to_string(&content_json)?.to_lowercase();
                    if dump.contains(&query) {
                        // Extract note_id from path: workspaces/ws/notes/{id}/
                        let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
                        if let Some(id) = parts.last() {
                            results.push(SearchResult { id: id.to_string() });
                        }
                    }
                }
            }
        }
    }

    Ok(results)
}
