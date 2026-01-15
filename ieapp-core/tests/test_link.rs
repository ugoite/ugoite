mod common;
use _ieapp_core::{link, note, workspace};
use common::setup_operator;
use uuid::Uuid;

async fn create_test_note(
    op: &opendal::Operator,
    ws_path: &str,
    note_id: &str,
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

    note::create_note(op, ws_path, note_id, "content", "author", &MockIntegrity).await?;
    Ok(())
}

#[tokio::test]
/// REQ-LNK-001
async fn test_link_req_lnk_001_create_link_bidirectional() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-links-ws";
    workspace::create_workspace(&op, ws_id, "/tmp").await?;
    let ws_path = format!("workspaces/{}", ws_id);

    // Create two notes
    create_test_note(&op, &ws_path, "note1").await?;
    create_test_note(&op, &ws_path, "note2").await?;

    let link_id = Uuid::new_v4().to_string();
    let link = link::create_link(&op, &ws_path, "note1", "note2", "related", &link_id).await?;

    assert_eq!(link.source, "note1");
    assert_eq!(link.target, "note2");
    assert_eq!(link.kind, "related");
    assert_eq!(link.id, link_id);

    // Verify persistence in note1
    let meta1_path = format!("{}/notes/note1/meta.json", ws_path);
    let bytes1 = op.read(&meta1_path).await?;
    let meta1: note::NoteMeta = serde_json::from_slice(&bytes1.to_vec())?;
    assert!(meta1
        .links
        .iter()
        .any(|l| l.id == link_id && l.target == "note2"));

    // Verify persistence in note2
    let meta2_path = format!("{}/notes/note2/meta.json", ws_path);
    let bytes2 = op.read(&meta2_path).await?;
    let meta2: note::NoteMeta = serde_json::from_slice(&bytes2.to_vec())?;
    assert!(meta2
        .links
        .iter()
        .any(|l| l.id == link_id && l.target == "note1")); // Reciprocal

    Ok(())
}

#[tokio::test]
/// REQ-LNK-002
async fn test_link_req_lnk_002_list_links() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-links-list-ws";
    workspace::create_workspace(&op, ws_id, "/tmp").await?;
    let ws_path = format!("workspaces/{}", ws_id);

    create_test_note(&op, &ws_path, "noteA").await?;
    create_test_note(&op, &ws_path, "noteB").await?;

    // Create a link
    let link_id = "link-123";
    link::create_link(&op, &ws_path, "noteA", "noteB", "parent", link_id).await?;

    let all_links = link::list_links(&op, &ws_path).await?;
    assert_eq!(all_links.len(), 1);
    assert_eq!(all_links[0].id, link_id);

    Ok(())
}

#[tokio::test]
/// REQ-LNK-003
async fn test_link_req_lnk_003_delete_link() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-links-del-ws";
    workspace::create_workspace(&op, ws_id, "/tmp").await?;
    let ws_path = format!("workspaces/{}", ws_id);

    create_test_note(&op, &ws_path, "noteX").await?;
    create_test_note(&op, &ws_path, "noteY").await?;

    let link_id = "link-to-delete";
    link::create_link(&op, &ws_path, "noteX", "noteY", "next", link_id).await?;

    // Verify exists
    let links_before = link::list_links(&op, &ws_path).await?;
    assert!(!links_before.is_empty());

    // Delete
    link::delete_link(&op, &ws_path, link_id).await?;

    // Verify gone
    let links_after = link::list_links(&op, &ws_path).await?;
    assert!(links_after.is_empty());

    // Check individual notes
    let meta_x_path = format!("{}/notes/noteX/meta.json", ws_path);
    let bytes_x = op.read(&meta_x_path).await?;
    let meta_x: note::NoteMeta = serde_json::from_slice(&bytes_x.to_vec())?;
    assert!(meta_x.links.is_empty());

    Ok(())
}
