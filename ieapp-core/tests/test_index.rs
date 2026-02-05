mod common;
use _ieapp_core::{entry, form, index, link, space};
use common::setup_operator;

#[tokio::test]
/// REQ-IDX-001
async fn test_index_req_idx_001_reindex_writes_index_files() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    index::reindex_all(&op, ws_path).await?;

    // Indexes are derived from Iceberg; no on-disk index files are created
    assert!(!op.exists(&format!("{}/index/index.json", ws_path)).await?);

    Ok(())
}

#[test]
/// REQ-IDX-001
fn test_index_req_idx_001_extract_properties_returns_object() {
    let markdown = "# Title";
    let props = index::extract_properties(markdown);
    assert!(props.is_object());
}

#[tokio::test]
/// REQ-IDX-002
async fn test_index_req_idx_002_validate_properties() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-space", "/tmp").await?;
    let ws_path = "spaces/test-space";

    // Setup form definition
    let class_def = r#"{
        "name": "Meeting",
        "fields": [
            {"name": "Date", "type": "date", "required": true}
        ]
    }"#;
    let class_def_value = serde_json::from_str::<serde_json::Value>(class_def)?;
    _ieapp_core::form::upsert_form(&op, ws_path, &class_def_value).await?;

    // Invalid property (wrong type/missing)
    let props = serde_json::json!({
        "Date": "invalid-date"
    });

    // Assuming validate_properties returns Result<Vec<String>> (list of warnings) or similar
    // We stub the expectation that it should fail or warn
    let class_def_value = serde_json::from_str::<serde_json::Value>(class_def)?;
    let (_casted, _warnings) = index::validate_properties(&props, &class_def_value)?;

    Ok(())
}

#[tokio::test]
/// REQ-IDX-003
async fn test_index_req_idx_003_query_index() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-ws", "/tmp").await?;
    let ws_path = "spaces/test-ws";

    index::reindex_all(&op, ws_path).await?;
    let results = index::query_index(&op, ws_path, "{}").await?;
    assert!(results.is_empty());
    Ok(())
}

#[tokio::test]
/// REQ-IDX-004
async fn test_index_req_idx_004_inverted_index_generation() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-ws", "/tmp").await?;
    let ws_path = "spaces/test-ws";

    index::reindex_all(&op, ws_path).await?;
    assert!(
        !op.exists(&format!("{}/index/inverted_index.json", ws_path))
            .await?
    );
    Ok(())
}

#[tokio::test]
/// REQ-IDX-008
async fn test_index_req_idx_008_query_sql() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-ws", "/tmp").await?;
    let ws_path = "spaces/test-sql-ws";

    struct MockIntegrity;
    impl _ieapp_core::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    let class_def = serde_json::json!({
        "name": "Meeting",
        "template": "# Meeting\n\n## Date\n\n## Topic\n",
        "fields": {
            "Date": {"type": "date"},
            "Topic": {"type": "string"}
        }
    });
    form::upsert_form(&op, ws_path, &class_def).await?;

    let entry_one = "---\nform: Meeting\n---\n# Entry 1\n\n## Date\n2025-01-01\n\n## Topic\nalpha";
    entry::create_entry(&op, ws_path, "entry-1", entry_one, "author", &MockIntegrity).await?;
    let entry_two = "---\nform: Meeting\n---\n# Entry 2\n\n## Date\n2025-02-10\n\n## Topic\nbeta";
    entry::create_entry(&op, ws_path, "entry-2", entry_two, "author", &MockIntegrity).await?;

    let payload = serde_json::json!({
        "$sql": "SELECT * FROM Meeting WHERE Date >= '2025-02-01'"
    })
    .to_string();
    let results = index::query_index(&op, ws_path, &payload).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["id"].as_str(), Some("entry-2"));

    Ok(())
}

#[test]
/// REQ-IDX-005
fn test_index_req_idx_005_word_count() {
    let content = "One two three";
    let count = index::compute_word_count(content);
    assert_eq!(count, 3);
}

#[tokio::test]
/// REQ-IDX-009
async fn test_index_req_idx_009_query_sql_joins() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "test-sql-join", "/tmp").await?;
    let ws_path = "spaces/test-sql-join";

    struct MockIntegrity;
    impl _ieapp_core::integrity::IntegrityProvider for MockIntegrity {
        fn checksum(&self, data: &str) -> String {
            format!("chk-{}", data.len())
        }

        fn signature(&self, _data: &str) -> String {
            "mock-signature".to_string()
        }
    }

    let class_def = serde_json::json!({
        "name": "Entry",
        "fields": {
            "Body": {"type": "markdown"}
        }
    });
    form::upsert_form(&op, ws_path, &class_def).await?;

    let entry_one = "---\nform: Entry\n---\n# Entry 1\n\n## Body\nAlpha";
    let entry_two = "---\nform: Entry\n---\n# Entry 2\n\n## Body\nBeta";
    let entry_three = "---\nform: Entry\n---\n# Entry 3\n\n## Body\nGamma";
    entry::create_entry(&op, ws_path, "entry-1", entry_one, "author", &MockIntegrity).await?;
    entry::create_entry(&op, ws_path, "entry-2", entry_two, "author", &MockIntegrity).await?;
    entry::create_entry(
        &op,
        ws_path,
        "entry-3",
        entry_three,
        "author",
        &MockIntegrity,
    )
    .await?;

    link::create_link(&op, ws_path, "entry-1", "entry-2", "reference", "link-1").await?;

    let payload = serde_json::json!({
        "$sql": "SELECT * FROM entries e JOIN links l ON e.id = l.source WHERE l.target = 'entry-2'"
    })
    .to_string();
    let results = index::query_index(&op, ws_path, &payload).await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["e"]["id"].as_str(), Some("entry-1"));
    assert_eq!(results[0]["l"]["target"].as_str(), Some("entry-2"));

    let payload_right = serde_json::json!({
        "$sql": "SELECT * FROM links l RIGHT JOIN entries e ON e.id = l.source WHERE e.id = 'entry-3'"
    })
    .to_string();
    let results_right = index::query_index(&op, ws_path, &payload_right).await?;
    assert_eq!(results_right.len(), 1);
    assert_eq!(results_right[0]["e"]["id"].as_str(), Some("entry-3"));
    assert!(results_right[0]["l"].is_null());

    let payload_full = serde_json::json!({
        "$sql": "SELECT * FROM entries e FULL JOIN links l ON e.id = l.source WHERE e.id = 'entry-3'"
    })
    .to_string();
    let results_full = index::query_index(&op, ws_path, &payload_full).await?;
    assert_eq!(results_full.len(), 1);
    assert_eq!(results_full[0]["e"]["id"].as_str(), Some("entry-3"));
    assert!(results_full[0]["l"].is_null());

    let payload_using = serde_json::json!({
        "$sql": "SELECT * FROM entries e JOIN entries f USING (id) WHERE e.id = 'entry-1'"
    })
    .to_string();
    let results_using = index::query_index(&op, ws_path, &payload_using).await?;
    assert_eq!(results_using.len(), 1);
    assert_eq!(results_using[0]["e"]["id"].as_str(), Some("entry-1"));
    assert_eq!(results_using[0]["f"]["id"].as_str(), Some("entry-1"));

    Ok(())
}

#[test]
/// REQ-IDX-010
fn test_index_req_idx_010_rich_content_parsing() -> anyhow::Result<()> {
    let class_def = serde_json::json!({
        "name": "Meeting",
        "fields": {
            "Done": {"type": "boolean"},
            "Count": {"type": "integer"},
            "Rate": {"type": "float"},
            "Event": {"type": "timestamp"},
            "EventTz": {"type": "timestamp_tz"},
            "EventNs": {"type": "timestamp_ns"},
            "EventTzNs": {"type": "timestamp_tz_ns"},
            "Time": {"type": "time"},
            "Uid": {"type": "uuid"},
            "Blob": {"type": "binary"},
            "Items": {"type": "list"}
        }
    });

    let markdown = "---\nclass: Meeting\n---\n# Title\n\n## Done\ntrue\n\n## Count\n42\n\n## Rate\n3.14\n\n## Event\n2025-01-02T03:04:05Z\n\n## EventTz\n2025-01-02T12:04:05+09:00\n\n## EventNs\n2025-01-02T03:04:05.123456789Z\n\n## EventTzNs\n2025-01-02T12:04:05.123456789+09:00\n\n## Time\n13:45:30.123456\n\n## Uid\nA7F9F5D2-8B7E-4DB1-9B0A-0E9A2B3F4C5D\n\n## Blob\nhex:64617461\n\n## Items\n- Alpha\n- Beta\n";
    let props = index::extract_properties(markdown);
    let (casted, warnings) = index::validate_properties(&props, &class_def)?;
    assert!(warnings.is_empty());

    let casted_obj = casted.as_object().unwrap();
    assert_eq!(casted_obj.get("Done").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(casted_obj.get("Count").and_then(|v| v.as_i64()), Some(42));
    let rate = casted_obj.get("Rate").and_then(|v| v.as_f64()).unwrap();
    assert!((rate - 3.14).abs() < 0.0001);
    assert_eq!(
        casted_obj.get("Event").and_then(|v| v.as_str()),
        Some("2025-01-02T03:04:05+00:00")
    );
    assert_eq!(
        casted_obj.get("EventTz").and_then(|v| v.as_str()),
        Some("2025-01-02T03:04:05+00:00")
    );
    assert_eq!(
        casted_obj.get("EventNs").and_then(|v| v.as_str()),
        Some("2025-01-02T03:04:05.123456789+00:00")
    );
    assert_eq!(
        casted_obj.get("EventTzNs").and_then(|v| v.as_str()),
        Some("2025-01-02T03:04:05.123456789+00:00")
    );
    assert_eq!(
        casted_obj.get("Time").and_then(|v| v.as_str()),
        Some("13:45:30.123456")
    );
    assert_eq!(
        casted_obj.get("Uid").and_then(|v| v.as_str()),
        Some("a7f9f5d2-8b7e-4db1-9b0a-0e9a2b3f4c5d")
    );
    let blob = casted_obj.get("Blob").and_then(|v| v.as_str()).unwrap();
    assert!(blob.starts_with("base64:"));
    let items = casted_obj.get("Items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_str(), Some("Alpha"));
    assert_eq!(items[1].as_str(), Some("Beta"));

    Ok(())
}
