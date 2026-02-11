mod common;
use _ugoite_core::{entry, form, search, space};
use common::setup_operator;

async fn create_test_entry(
    op: &opendal::Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
) -> anyhow::Result<()> {
    // Mock integrity provider
    struct MockIntegrity;
    impl _ugoite_core::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }
        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    let form_def = serde_json::json!({
        "name": "Entry",
        "template": "# Entry\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}},
    });
    form::upsert_form(op, ws_path, &form_def).await?;
    let markdown = format!(
        "---\nform: Entry\n---\n# {}\n\n## Body\n{}",
        entry_id, content
    );
    entry::create_entry(op, ws_path, entry_id, &markdown, "author", &MockIntegrity).await?;
    Ok(())
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct SearchResultWrapper {
    id: String,
    // Add logic to extract matches if search struct exposes more
}

#[tokio::test]
/// REQ-SRCH-001
async fn test_search_req_srch_001_keyword_search() -> anyhow::Result<()> {
    // Basic search functionality - currently effectively same as scan
    // since we haven't implemented full indexing yet
    let op = setup_operator()?;
    let ws_id = "test-search-ws-keyword";
    space::create_space(&op, ws_id, "/tmp").await?;
    let ws_path = format!("spaces/{}", ws_id);

    create_test_entry(&op, &ws_path, "entry1", "This is a secret project").await?;
    create_test_entry(&op, &ws_path, "entry2", "Public information here").await?;
    create_test_entry(&op, &ws_path, "entry3", "Another project update").await?;

    // Search for "project"
    let results = search::search_entries(&op, &ws_path, "project").await?;
    assert_eq!(results.len(), 2);

    // Check results contain expected entries
    let found_ids: Vec<String> = results.into_iter().map(|s| s.id).collect();
    assert!(found_ids.contains(&"entry1".to_string()));
    assert!(found_ids.contains(&"entry3".to_string()));
    assert!(!found_ids.contains(&"entry2".to_string()));

    Ok(())
}

#[tokio::test]
/// REQ-SRCH-002
async fn test_search_req_srch_002_fallback_scan() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-search-ws";
    space::create_space(&op, ws_id, "/tmp").await?;
    let ws_path = format!("spaces/{}", ws_id);

    // Create entries with distinct content
    create_test_entry(&op, &ws_path, "entry1", "Unicorns exist").await?;
    create_test_entry(&op, &ws_path, "entry2", "Dragons fly").await?;
    create_test_entry(&op, &ws_path, "entry3", "Unicorns and Dragons").await?;

    // Search for "Unicorns" (case-insensitive ideally)
    let results = search::search_entries(&op, &ws_path, "unicorns").await?;

    // Expect entry1 and entry3
    assert_eq!(results.len(), 2);
    let ids: Vec<String> = results.iter().map(|r| r.id.clone()).collect();
    assert!(ids.contains(&"entry1".to_string()));
    assert!(ids.contains(&"entry3".to_string()));
    assert!(!ids.contains(&"entry2".to_string()));

    Ok(())
}
