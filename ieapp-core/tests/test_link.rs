mod common;
use _ieapp_core::{entry, form, link, space};
use common::setup_operator;
use uuid::Uuid;

async fn create_test_entry(
    op: &opendal::Operator,
    ws_path: &str,
    entry_id: &str,
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

    let form_def = serde_json::json!({
        "name": "Entry",
        "template": "# Entry\n\n## Body\n",
        "fields": {"Body": {"type": "markdown"}},
    });
    form::upsert_form(op, ws_path, &form_def).await?;
    let content = "---\nform: Entry\n---\n# content\n";
    entry::create_entry(op, ws_path, entry_id, content, "author", &MockIntegrity).await?;
    Ok(())
}

#[tokio::test]
/// REQ-LNK-001
async fn test_link_req_lnk_001_create_link_bidirectional() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-links-ws";
    space::create_space(&op, ws_id, "/tmp").await?;
    let ws_path = format!("spaces/{}", ws_id);

    // Create two entries
    create_test_entry(&op, &ws_path, "entry1").await?;
    create_test_entry(&op, &ws_path, "entry2").await?;

    let link_id = Uuid::new_v4().to_string();
    let link = link::create_link(&op, &ws_path, "entry1", "entry2", "related", &link_id).await?;

    assert_eq!(link.source, "entry1");
    assert_eq!(link.target, "entry2");
    assert_eq!(link.kind, "related");
    assert_eq!(link.id, link_id);

    // Verify persistence in note1
    let entry1 = entry::get_entry(&op, &ws_path, "entry1").await?;
    let links1 = entry1.get("links").and_then(|v| v.as_array()).unwrap();
    assert!(links1.iter().any(|l| {
        l.get("id").and_then(|v| v.as_str()) == Some(link_id.as_str())
            && l.get("target").and_then(|v| v.as_str()) == Some("entry2")
    }));

    // Verify persistence in note2
    let entry2 = entry::get_entry(&op, &ws_path, "entry2").await?;
    let links2 = entry2.get("links").and_then(|v| v.as_array()).unwrap();
    assert!(links2.iter().any(|l| {
        l.get("id").and_then(|v| v.as_str()) == Some(link_id.as_str())
            && l.get("target").and_then(|v| v.as_str()) == Some("entry1")
    })); // Reciprocal

    Ok(())
}

#[tokio::test]
/// REQ-LNK-002
async fn test_link_req_lnk_002_list_links() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_id = "test-links-list-ws";
    space::create_space(&op, ws_id, "/tmp").await?;
    let ws_path = format!("spaces/{}", ws_id);

    create_test_entry(&op, &ws_path, "entryA").await?;
    create_test_entry(&op, &ws_path, "entryB").await?;

    // Create a link
    let link_id = "link-123";
    link::create_link(&op, &ws_path, "entryA", "entryB", "parent", link_id).await?;

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
    space::create_space(&op, ws_id, "/tmp").await?;
    let ws_path = format!("spaces/{}", ws_id);

    create_test_entry(&op, &ws_path, "entryX").await?;
    create_test_entry(&op, &ws_path, "entryY").await?;

    let link_id = "link-to-delete";
    link::create_link(&op, &ws_path, "entryX", "entryY", "next", link_id).await?;

    // Verify exists
    let links_before = link::list_links(&op, &ws_path).await?;
    assert!(!links_before.is_empty());

    // Delete
    link::delete_link(&op, &ws_path, link_id).await?;

    // Verify gone
    let links_after = link::list_links(&op, &ws_path).await?;
    assert!(links_after.is_empty());

    // Check individual entries
    let entry_x = entry::get_entry(&op, &ws_path, "entryX").await?;
    let links_x = entry_x.get("links").and_then(|v| v.as_array()).unwrap();
    assert!(links_x.is_empty());

    Ok(())
}
