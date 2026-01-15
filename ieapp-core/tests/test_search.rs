mod common;
use _ieapp_core::{note, search, workspace};
use common::setup_operator;

async fn create_test_note(
    op: &opendal::Operator,
    ws_path: &str,
    note_id: &str,
    content: &str,
) -> anyhow::Result<()> {
    // Mock integrity provider
    struct MockIntegrity;
    impl _ieapp_core::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }
        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    note::create_note(op, ws_path, note_id, content, "author", &MockIntegrity).await?;
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
    workspace::create_workspace(&op, ws_id, "/tmp").await?;
    let ws_path = format!("workspaces/{}", ws_id);

    create_test_note(&op, &ws_path, "note1", "This is a secret project").await?;
    create_test_note(&op, &ws_path, "note2", "Public information here").await?;
    create_test_note(&op, &ws_path, "note3", "Another project update").await?;

    // Search for "project"
    let results = search::search_notes(&op, &ws_path, "project").await?;
    assert_eq!(results.len(), 2);

    // Check results contain expected notes
    let found_ids: Vec<String> = results.into_iter().map(|s| s.id).collect();
    assert!(found_ids.contains(&"note1".to_string()));
    assert!(found_ids.contains(&"note3".to_string()));
    assert!(!found_ids.contains(&"note2".to_string()));

    Ok(())
}

#[tokio::test]
/// REQ-SRCH-002
async fn test_search_req_srch_002_fallback_scan() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-search-ws";
    workspace::create_workspace(&op, ws_id, "/tmp").await?;
    let ws_path = format!("workspaces/{}", ws_id);

    // Create notes with distinct content
    create_test_note(&op, &ws_path, "note1", "Unicorns exist").await?;
    create_test_note(&op, &ws_path, "note2", "Dragons fly").await?;
    create_test_note(&op, &ws_path, "note3", "Unicorns and Dragons").await?;

    // Search for "Unicorns" (case-insensitive ideally)
    let results = search::search_notes(&op, &ws_path, "unicorns").await?;

    // Expect note1 and note3
    assert_eq!(results.len(), 2);
    let ids: Vec<String> = results.iter().map(|r| r.id.clone()).collect();
    assert!(ids.contains(&"note1".to_string()));
    assert!(ids.contains(&"note3".to_string()));
    assert!(!ids.contains(&"note2".to_string()));

    Ok(())
}
