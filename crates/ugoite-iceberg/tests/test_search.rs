mod common;
use common::setup_operator;
use std::collections::BTreeMap;
use ugoite_domain::change::ChangeCommand;
use ugoite_domain::entry::{EntryMetadata, EntryOperation, EntryRevision, FieldValue};
use ugoite_iceberg::{entry, form, iceberg_store, publication_context_for_change, search, space};
use uuid::Uuid;

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

async fn append_entries_at_search_cap(op: &opendal::Operator, ws_path: &str) -> anyhow::Result<()> {
    let workspace = iceberg_store::native_mutation_workspace(op, ws_path).await?;
    let form = workspace
        .list_forms()
        .await?
        .into_iter()
        .find(|form| form.name == "Entry")
        .expect("Entry form");
    let field_id = form.fields.first().expect("Entry field").id;
    let revisions = (0..ugoite_iceberg::MAX_NORMAL_READ_ROWS)
        .map(|index| {
            let timestamp = i64::try_from(index).unwrap_or_default() + 1;
            EntryRevision {
                form_id: form.id,
                entry_id: Uuid::from_u128(100_000 + index as u128).into(),
                revision_id: Uuid::from_u128(200_000 + index as u128).into(),
                parent_revision_id: None,
                entry_version: 1,
                change_id: "search-cap-change".to_owned(),
                expected_version: None,
                operation: EntryOperation::Upsert,
                committed_at_micros: timestamp,
                author_id: "author".to_owned(),
                form_version: form.version,
                source_kind: "test".to_owned(),
                source_id: None,
                entry: EntryMetadata {
                    external_id: format!("cap-{index:05}"),
                    title: format!("Match {index:05}"),
                    created_at_micros: timestamp,
                    updated_at_micros: timestamp,
                    updated_by: "author".to_owned(),
                    ..EntryMetadata::default()
                },
                values: BTreeMap::from([(
                    field_id,
                    FieldValue::String(format!("searchable content {index}")),
                )]),
                extra_attributes: BTreeMap::new(),
                extension_metadata: BTreeMap::new(),
            }
        })
        .collect::<Vec<_>>();
    let command = ChangeCommand {
        change_id: "search-cap-change".to_owned(),
        run_id: None,
        actor_principal_id: "author".to_owned(),
        message: Some("populate search cap boundary".to_owned()),
        reverts_change_id: None,
        created_at_micros: 1,
    };
    workspace
        .commit(publication_context_for_change(
            &command,
            "test.search.cap",
            &revisions,
        )?)?
        .append_revisions(form.id, revisions)
        .await?;
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
/// REQ-SRCH-002
async fn test_search_req_srch_002_stale_derived_fallback() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = "spaces/search-stale-derived";
    space::create_space(&op, "search-stale-derived", "/tmp").await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "template": "# Entry\n\n## Body\n",
            "fields": {"Body": {"type": "markdown"}},
        }),
    )
    .await?;
    ugoite_iceberg::derived_relation::rebuild_asset_text(&op, ws_path).await?;
    entry::create_entry(
        &op,
        ws_path,
        "stale-derived-entry",
        "---\nform: Entry\n---\n# Stale derived entry\n\n## Body\nstale fallback needle",
        "author",
        &ugoite_iceberg::integrity::FakeIntegrityProvider,
    )
    .await?;

    let results = search::search_entries(
        &op,
        ws_path,
        "stale fallback needle",
        ugoite_iceberg::MAX_NORMAL_READ_ROWS,
    )
    .await?;
    assert_eq!(
        results
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>(),
        ["stale-derived-entry"]
    );
    Ok(())
}

#[tokio::test]
/// REQ-SRCH-002
async fn test_search_req_srch_002_corrupt_derived_fallback() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = "spaces/search-corrupt-derived";
    space::create_space(&op, "search-corrupt-derived", "/tmp").await?;
    create_test_entry(
        &op,
        ws_path,
        "corrupt-derived-entry",
        "corrupt fallback needle",
    )
    .await?;
    ugoite_iceberg::derived_relation::rebuild_asset_text(&op, ws_path).await?;
    let head_path = format!(
        "{ws_path}/_ugoite/derived/relations/{}/head.json",
        ugoite_domain::derived_relation::DerivedRelationId::ASSET_TEXT
    );
    op.write(&head_path, b"{not valid json".to_vec()).await?;

    let results = search::search_entries(
        &op,
        ws_path,
        "corrupt fallback needle",
        ugoite_iceberg::MAX_NORMAL_READ_ROWS,
    )
    .await?;
    assert_eq!(
        results
            .iter()
            .map(|result| result.id.as_str())
            .collect::<Vec<_>>(),
        ["corrupt-derived-entry"]
    );
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

#[tokio::test]
/// Lane 2 PR6: invalid queries fail with canonical codes before any Storage
/// access. The workspace path does not exist, so any Storage-first behavior
/// would surface a different error.
async fn search_rejects_invalid_queries_before_storage_access() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let missing_ws = "spaces/search-admission-missing";
    for (query, code) in [
        ("", "SEARCH_QUERY_EMPTY"),
        ("   ", "SEARCH_QUERY_EMPTY"),
        (
            &"x".repeat(ugoite_core::query::MAX_SEARCH_QUERY_BYTES + 1),
            "INVALID_INPUT",
        ),
    ] {
        let error = search::search_entries(&op, missing_ws, query, 10)
            .await
            .expect_err("invalid query must fail");
        let app_error = error
            .downcast_ref::<ugoite_core::error::AppError>()
            .expect("typed AppError");
        assert_eq!(app_error.code_str(), code, "query {query:?}");
    }
    Ok(())
}

#[tokio::test]
/// Issue 2247: keyword search applies the same Unicode normalization to
/// stored Entry content and the query.
async fn search_matches_unicode_compatibility_and_composed_forms() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = "spaces/search-unicode";
    space::create_space(&op, "search-unicode", "/tmp").await?;
    create_test_entry(
        &op,
        ws_path,
        "unicode-entry",
        "Ｕｇｏｉｔｅ 日本語の世界 Cafe\u{301} 😀",
    )
    .await?;

    for query in ["ugoite", "café", "世界", "😀"] {
        let results =
            search::search_entries(&op, ws_path, query, ugoite_iceberg::MAX_NORMAL_READ_ROWS)
                .await?;
        assert_eq!(
            results
                .iter()
                .map(|result| result.id.as_str())
                .collect::<Vec<_>>(),
            ["unicode-entry"],
            "query {query:?} should match normalized Entry content"
        );
    }

    Ok(())
}

#[tokio::test]
async fn search_pagination_returns_the_terminal_row_at_the_normal_read_cap() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "search-cap-boundary", "/tmp").await?;
    let ws_path = "spaces/search-cap-boundary";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Entry",
            "fields": {"Body": {"type": "markdown"}}
        }),
    )
    .await?;
    append_entries_at_search_cap(&op, ws_path).await?;

    let cap = ugoite_iceberg::MAX_NORMAL_READ_ROWS;
    let terminal = search::search_entries_paged(&op, ws_path, "searchable", 2, cap - 1).await?;
    assert_eq!(
        terminal.len(),
        1,
        "the final in-cap page must return its row"
    );
    assert_eq!(terminal[0].id, format!("cap-{:05}", cap - 1));

    let after_cap = search::search_entries_paged(&op, ws_path, "searchable", 2, cap).await?;
    assert!(after_cap.is_empty(), "the page after the cap must be empty");
    Ok(())
}
