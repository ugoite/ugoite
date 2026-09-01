mod common;
use common::setup_operator;
use ugoite_iceberg::{entry, form, search, space};

async fn create_test_entry(
    op: &opendal::Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
) -> anyhow::Result<()> {
    // Mock integrity provider
    struct MockIntegrity;
    impl ugoite_iceberg::integrity::IntegrityProvider for MockIntegrity {
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
    let tags = if entry_id == "entry1" {
        "tags: [release]"
    } else {
        ""
    };
    let markdown = format!(
        "---\nform: Entry\n{}\n---\n# {}\n\n## Body\n{}",
        tags, entry_id, content
    );
    entry::create_entry(op, ws_path, entry_id, &markdown, "author", &MockIntegrity).await?;
    Ok(())
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
    let results = search::search_entries(
        &op,
        &ws_path,
        "project",
        ugoite_iceberg::MAX_NORMAL_READ_ROWS,
    )
    .await?;
    assert_eq!(results.len(), 2);

    // Check results contain expected entries
    let found_ids: Vec<String> = results.iter().map(|s| s.id.clone()).collect();
    assert!(found_ids.contains(&"entry1".to_string()));
    assert!(found_ids.contains(&"entry3".to_string()));
    assert!(!found_ids.contains(&"entry2".to_string()));
    let first = results.iter().find(|result| result.id == "entry1").unwrap();
    assert_eq!(first.title, "entry1");
    assert_eq!(first.form, "Entry");

    let tag_results = search::search_entries(
        &op,
        &ws_path,
        "release",
        ugoite_iceberg::MAX_NORMAL_READ_ROWS,
    )
    .await?;
    assert_eq!(
        tag_results
            .iter()
            .map(|result| &result.id)
            .collect::<Vec<_>>(),
        [&"entry1".to_string()]
    );

    let limited = search::search_entries(&op, &ws_path, "project", 1).await?;
    assert_eq!(limited.len(), 1);

    let relation_scopes = form::list_forms(&op, &ws_path)
        .await?
        .into_iter()
        .filter_map(|form| {
            form.get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .map(|form_name| {
            (
                form_name.to_ascii_lowercase(),
                ugoite_core::query::EntryScope::AllCurrent,
            )
        })
        .collect();
    let first_page = search::search_entries_with_scopes_after(
        &op,
        &ws_path,
        "project",
        &relation_scopes,
        1,
        None,
    )
    .await?;
    let first = first_page.first().expect("first search page");
    let second_page = search::search_entries_with_scopes_after(
        &op,
        &ws_path,
        "project",
        &relation_scopes,
        1,
        Some((&first.title, &first.id, &first.form)),
    )
    .await?;
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].id, "entry3");

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
    let results = search::search_entries(
        &op,
        &ws_path,
        "unicorns",
        ugoite_iceberg::MAX_NORMAL_READ_ROWS,
    )
    .await?;

    // Expect entry1 and entry3
    assert_eq!(results.len(), 2);
    let ids: Vec<String> = results.iter().map(|r| r.id.clone()).collect();
    assert!(ids.contains(&"entry1".to_string()));
    assert!(ids.contains(&"entry3".to_string()));
    assert!(!ids.contains(&"entry2".to_string()));

    Ok(())
}

#[tokio::test]
/// Issue 2135: an empty Form with unsupported projection types must not break
/// keyword search for an ordinary Form in the same Space.
async fn search_ignores_incompatible_empty_form() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = "spaces/search-complex-form";
    space::create_space(&op, "search-complex-form", "/tmp").await?;
    create_test_entry(&op, ws_path, "ordinary-entry", "ordinary searchable text").await?;

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Complex",
            "fields": {
                "Day": {"type": "date"},
                "Time": {"type": "time"},
                "When": {"type": "timestamp_tz"},
                "Identifier": {"type": "uuid"},
                "Labels": {"type": "list", "items": {"type": "string"}},
                "Blob": {"type": "binary"},
                "Objects": {"type": "object_list"}
            }
        }),
    )
    .await?;

    let results = search::search_entries(
        &op,
        ws_path,
        "ordinary searchable",
        ugoite_iceberg::MAX_NORMAL_READ_ROWS,
    )
    .await?;
    assert_eq!(
        results
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>(),
        ["ordinary-entry"]
    );
    Ok(())
}

#[tokio::test]
/// Issue 2155: supported fields remain searchable in a mixed Form.
async fn search_preserves_supported_fields_in_mixed_form() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = "spaces/search-mixed-form";
    space::create_space(&op, "search-mixed-form", "/tmp").await?;
    let form_def = serde_json::json!({
        "name": "Mixed",
        "fields": {
            "Notes": {"type": "markdown"},
            "Attachment": {"type": "asset_reference"}
        }
    });
    form::upsert_form(&op, ws_path, &form_def).await?;
    entry::create_entry(
        &op,
        ws_path,
        "mixed-entry",
        "---\nform: Mixed\n---\n# Mixed entry\n\n## Notes\nmixed-form-search-needle",
        "author",
        &ugoite_iceberg::integrity::FakeIntegrityProvider,
    )
    .await?;

    let results = search::search_entries(
        &op,
        ws_path,
        "mixed-form-search-needle",
        ugoite_iceberg::MAX_NORMAL_READ_ROWS,
    )
    .await?;
    assert_eq!(
        results
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>(),
        ["mixed-entry"]
    );
    Ok(())
}
