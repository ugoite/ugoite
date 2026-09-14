mod common;
use common::setup_operator;
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::query::EntryScope;
use ugoite_core::structured_search::{SearchCondition, SearchOperator, StructuredSearch};
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::{entry, form, space, structured_search};

async fn setup_task_space(op: &opendal::Operator, ws_id: &str) -> anyhow::Result<String> {
    space::create_space(op, ws_id, "/tmp").await?;
    let ws_path = format!("spaces/{ws_id}");
    form::upsert_form(
        op,
        &ws_path,
        &serde_json::json!({
            "name": "Task",
            "fields": {
                "summary": {"type": "string"},
                "status": {"type": "string"},
                "done": {"type": "boolean"},
                "priority": {"type": "integer"},
                "score": {"type": "float"},
                "due": {"type": "date"},
                "remind_at": {"type": "timestamp_tz"}
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;
    Ok(ws_path)
}

fn fields(pairs: Vec<(&str, serde_json::Value)>) -> BTreeMap<String, serde_json::Value> {
    pairs
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn criteria(conditions: Vec<(&str, SearchOperator, serde_json::Value)>) -> StructuredSearch {
    StructuredSearch {
        form: "Task".to_owned(),
        updated_from: None,
        updated_to: None,
        conditions: conditions
            .into_iter()
            .map(|(field, operator, value)| SearchCondition {
                field: field.to_owned(),
                operator,
                value,
            })
            .collect(),
        limit: Some(100),
        offset: None,
    }
}

#[tokio::test]
async fn structured_search_filters_by_typed_conditions() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = setup_task_space(&op, "structured-search-basic").await?;
    let integrity = FakeIntegrityProvider;

    entry::create_structured_entry_with_scopes_and_change(
        &op,
        &ws_path,
        "task-open",
        Some("Release".to_owned()),
        "Task".to_owned(),
        Vec::new(),
        fields(vec![
            ("summary", serde_json::json!("release v1")),
            ("status", serde_json::json!("open")),
            ("done", serde_json::json!(false)),
            ("priority", serde_json::json!(3)),
            ("score", serde_json::json!(1.5)),
            ("due", serde_json::json!("2026-09-10")),
            ("remind_at", serde_json::json!("2026-09-10T09:00:00Z")),
        ]),
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        &ws_path,
        "task-closed",
        Some("Cleanup".to_owned()),
        "Task".to_owned(),
        Vec::new(),
        fields(vec![
            ("summary", serde_json::json!("cleanup")),
            ("status", serde_json::json!("closed")),
            ("done", serde_json::json!(true)),
            ("priority", serde_json::json!(7)),
            ("score", serde_json::json!(9.5)),
            ("due", serde_json::json!("2026-09-12")),
            ("remind_at", serde_json::json!("2026-09-12T09:00:00Z")),
        ]),
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let open = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "status",
            SearchOperator::Equals,
            serde_json::json!("open"),
        )]),
    )
    .await?;
    assert_eq!(open.len(), 1, "equals must filter to one Entry: {open:?}");

    let high_priority = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "priority",
            SearchOperator::Gte,
            serde_json::json!(5),
        )]),
    )
    .await?;
    assert_eq!(high_priority.len(), 1);

    // Core direct and authorized paths return the same set/order.
    let scopes: BTreeMap<String, EntryScope> = [("task".to_owned(), EntryScope::AllCurrent)]
        .into_iter()
        .collect();
    let authorized = structured_search::search_structured_with_scopes(
        &op,
        &ws_path,
        &criteria(vec![(
            "status",
            SearchOperator::Equals,
            serde_json::json!("open"),
        )]),
        &scopes,
    )
    .await?;
    assert_eq!(open, authorized);

    // The authorized page executor owns pagination. The second row must be
    // the same row selected by the full ordered result, not a second slice of
    // an already paginated SQL result.
    let all = structured_search::search_structured(&op, &ws_path, &criteria(Vec::new())).await?;
    assert_eq!(all.len(), 2);
    let mut second_page = criteria(Vec::new());
    second_page.limit = Some(1);
    second_page.offset = Some(1);
    let page = structured_search::search_structured(&op, &ws_path, &second_page).await?;
    assert_eq!(page, vec![all[1].clone()]);
    Ok(())
}

#[tokio::test]
async fn structured_search_contains_escapes_special_chars() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = setup_task_space(&op, "structured-search-escape").await?;
    let integrity = FakeIntegrityProvider;

    for (id, title) in [
        ("task-percent", "100% done_release"),
        ("task-underscore", "under_score release"),
        ("task-quote", "it's a \"release\""),
        ("task-backslash", "path\\to\\release"),
        ("task-plain", "ordinary work"),
    ] {
        entry::create_structured_entry_with_scopes_and_change(
            &op,
            &ws_path,
            id,
            Some(title.to_owned()),
            "Task".to_owned(),
            Vec::new(),
            fields(vec![
                ("summary", serde_json::json!(title)),
                ("status", serde_json::json!("open")),
                ("done", serde_json::json!(false)),
                ("priority", serde_json::json!(1)),
                ("score", serde_json::json!(1.0)),
                ("due", serde_json::json!("2026-09-10")),
                ("remind_at", serde_json::json!("2026-09-10T09:00:00Z")),
            ]),
            BTreeMap::new(),
            "author",
            &integrity,
            None,
            None,
        )
        .await?;
    }

    // Literal % must not act as a wildcard.
    let percent = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "summary",
            SearchOperator::Contains,
            serde_json::json!("100%"),
        )]),
    )
    .await?;
    assert_eq!(percent.len(), 1, "percent must be literal: {percent:?}");

    let underscore = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "summary",
            SearchOperator::Contains,
            serde_json::json!("under_score"),
        )]),
    )
    .await?;
    assert_eq!(underscore.len(), 1, "underscore must be literal");

    let quote = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "summary",
            SearchOperator::Contains,
            serde_json::json!("\"release\""),
        )]),
    )
    .await?;
    assert_eq!(quote.len(), 1, "quote must be literal");

    let backslash = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "summary",
            SearchOperator::Contains,
            serde_json::json!("path\\to"),
        )]),
    )
    .await?;
    assert_eq!(backslash.len(), 1, "backslash must be literal");

    // A bare wildcard character must match only literal occurrences.
    let wildcard = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "summary",
            SearchOperator::Contains,
            serde_json::json!("%"),
        )]),
    )
    .await?;
    assert_eq!(wildcard.len(), 1, "bare % matches only literal percent");
    Ok(())
}

#[tokio::test]
async fn structured_search_preserves_timestamp_kind_precision_and_predicates() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    let ws_path = "spaces/structured-search-timestamps";
    space::create_space(&op, "structured-search-timestamps", "/tmp").await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Temporal",
            "fields": {
                "wall": {"type": "timestamp"},
                "instant": {"type": "timestamp_tz"},
                "wall_ns": {"type": "timestamp_ns"},
                "instant_ns": {"type": "timestamp_tz_ns"}
            }
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;

    for (entry_id, second) in [("temporal-a", "00"), ("temporal-b", "01")] {
        entry::create_structured_entry_with_scopes_and_change(
            &op,
            ws_path,
            entry_id,
            Some(entry_id.to_owned()),
            "Temporal".to_owned(),
            Vec::new(),
            fields(vec![
                (
                    "wall",
                    serde_json::json!(format!("2026-09-10T09:00:{second}.123456789")),
                ),
                (
                    "instant",
                    serde_json::json!(format!("2026-09-10T09:00:{second}.123456789+09:00")),
                ),
                (
                    "wall_ns",
                    serde_json::json!(format!("2026-09-10T09:00:{second}.123456789")),
                ),
                (
                    "instant_ns",
                    serde_json::json!(format!("2026-09-10T09:00:{second}.123456789+09:00")),
                ),
            ]),
            BTreeMap::new(),
            "author",
            &integrity,
            None,
            None,
        )
        .await?;
    }

    let timestamp_cases = [
        (
            "wall",
            "2026-09-10T09:00:00.123456789",
            "2026-09-10T09:00:00.123456789",
        ),
        (
            "instant",
            "2026-09-10T00:00:00.123456Z",
            "2026-09-10T09:00:00.123456789+09:00",
        ),
        (
            "wall_ns",
            "2026-09-10T09:00:00.123456789",
            "2026-09-10T09:00:00.123456789",
        ),
        (
            "instant_ns",
            "2026-09-10T00:00:00.123456789Z",
            "2026-09-10T09:00:00.123456789+09:00",
        ),
    ];

    for (field_name, equivalent_value, stored_value) in timestamp_cases {
        let equals = StructuredSearch {
            form: "Temporal".to_owned(),
            updated_from: None,
            updated_to: None,
            conditions: vec![SearchCondition {
                field: field_name.to_owned(),
                operator: SearchOperator::Equals,
                value: serde_json::json!(equivalent_value),
            }],
            limit: Some(100),
            offset: None,
        };
        let rows = structured_search::search_structured(&op, ws_path, &equals).await?;
        assert_eq!(
            rows.iter()
                .map(|row| row["_ugoite_id"].as_str().unwrap_or_default())
                .collect::<Vec<_>>(),
            ["temporal-a"],
            "{field_name} must compare its canonical value"
        );

        let greater_than = StructuredSearch {
            conditions: vec![SearchCondition {
                field: field_name.to_owned(),
                operator: SearchOperator::Gt,
                value: serde_json::json!(stored_value),
            }],
            ..equals
        };
        let rows = structured_search::search_structured(&op, ws_path, &greater_than).await?;
        assert_eq!(
            rows.iter()
                .map(|row| row["_ugoite_id"].as_str().unwrap_or_default())
                .collect::<Vec<_>>(),
            ["temporal-b"],
            "{field_name} must preserve ordered predicate semantics"
        );
    }
    Ok(())
}

#[tokio::test]
async fn structured_search_rejects_invalid_before_execution() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = setup_task_space(&op, "structured-search-admission").await?;

    // Unknown field.
    let error = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "missing",
            SearchOperator::Equals,
            serde_json::json!("x"),
        )]),
    )
    .await
    .expect_err("unknown field must fail");
    assert!(
        error.to_string().contains("UNKNOWN_FORM_FIELDS")
            || error.to_string().contains("was not found")
    );

    // Invalid operator for type.
    let error = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "summary",
            SearchOperator::Gt,
            serde_json::json!("x"),
        )]),
    )
    .await
    .expect_err("invalid operator must fail");
    assert!(error.to_string().contains("not supported"));

    // Invalid typed value.
    let error = structured_search::search_structured(
        &op,
        &ws_path,
        &criteria(vec![(
            "priority",
            SearchOperator::Equals,
            serde_json::json!("not-a-number"),
        )]),
    )
    .await
    .expect_err("invalid value must fail");
    assert!(!error.to_string().is_empty());
    Ok(())
}

#[tokio::test]
async fn structured_search_applies_permission_filtering_first() -> anyhow::Result<()> {
    let op = setup_operator()?;
    let ws_path = setup_task_space(&op, "structured-search-authz").await?;
    let integrity = FakeIntegrityProvider;
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        &ws_path,
        "task-1",
        Some("Hello".to_owned()),
        "Task".to_owned(),
        Vec::new(),
        fields(vec![
            ("summary", serde_json::json!("hello")),
            ("status", serde_json::json!("open")),
            ("done", serde_json::json!(false)),
            ("priority", serde_json::json!(1)),
            ("score", serde_json::json!(1.0)),
            ("due", serde_json::json!("2026-09-10")),
            ("remind_at", serde_json::json!("2026-09-10T09:00:00Z")),
        ]),
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    // Empty scopes: unauthorized Form returns empty without leaking existence.
    let empty_scopes: BTreeMap<String, EntryScope> = BTreeMap::new();
    let denied = structured_search::search_structured_with_scopes(
        &op,
        &ws_path,
        &criteria(vec![(
            "status",
            SearchOperator::Equals,
            serde_json::json!("open"),
        )]),
        &empty_scopes,
    )
    .await?;
    assert!(denied.is_empty(), "unauthorized form must return empty");

    // Only scope with empty allow-list also returns empty.
    let only_empty: BTreeMap<String, EntryScope> =
        [("task".to_owned(), EntryScope::Only(BTreeSet::new()))]
            .into_iter()
            .collect();
    let filtered = structured_search::search_structured_with_scopes(
        &op,
        &ws_path,
        &criteria(vec![(
            "status",
            SearchOperator::Equals,
            serde_json::json!("open"),
        )]),
        &only_empty,
    )
    .await?;
    assert!(filtered.is_empty());

    // An unknown Form is intentionally indistinguishable from an inaccessible
    // Form on the authorized boundary.
    let unknown = structured_search::search_structured_with_scopes(
        &op,
        &ws_path,
        &StructuredSearch {
            form: "Missing".to_owned(),
            ..criteria(Vec::new())
        },
        &BTreeMap::from([("task".to_owned(), EntryScope::AllCurrent)]),
    )
    .await?;
    assert!(unknown.is_empty());
    Ok(())
}
