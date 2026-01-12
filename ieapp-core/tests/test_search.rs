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

#[tokio::test]
/// REQ-SRCH-002
async fn test_search_req_srch_002_fallback_scan() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-search-ws";
    workspace::create_workspace(&op, ws_id).await?;
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
