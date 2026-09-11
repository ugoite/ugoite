mod common;
use common::setup_operator;
use std::collections::BTreeMap;
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::{entry, form, space};

async fn ensure_note_form(op: &opendal::Operator, ws_path: &str) -> anyhow::Result<()> {
    form::upsert_form(
        op,
        ws_path,
        &serde_json::json!({
            "name": "Note",
            "fields": {
                "Body": {"type": "string"},
                "Done": {"type": "boolean"},
                "Count": {"type": "integer"},
                "Labels": {"type": "list"}
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;
    Ok(())
}

fn durable_view(value: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "title": value.get("title"),
        "form": value.get("form"),
        "tags": value.get("tags"),
        "frontmatter": value.get("frontmatter"),
        "sections": value.get("sections"),
    })
}

#[tokio::test]
async fn legacy_and_structured_creates_reach_the_same_durable_outcome() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "structured-parity", "/tmp").await?;
    let ws_path = "spaces/structured-parity";
    ensure_note_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let markdown = "---\nform: Note\ntags:\n  - inbox\n---\n# Shopping\n\n## Body\nBuy milk.\n\n## Done\nyes\n\n## Count\n42\n\n## Labels\n- alpha\n- beta\n";
    entry::create_entry(&op, ws_path, "legacy-note", markdown, "author", &integrity).await?;

    let mut fields = BTreeMap::new();
    fields.insert(
        "Body".to_string(),
        serde_json::Value::String("Buy milk.".to_string()),
    );
    fields.insert("Done".to_string(), serde_json::Value::Bool(true));
    fields.insert("Count".to_string(), serde_json::Value::Number(42.into()));
    fields.insert("Labels".to_string(), serde_json::json!(["alpha", "beta"]));
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "structured-note",
        Some("Shopping".to_string()),
        "Note".to_string(),
        vec!["inbox".to_string()],
        fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let legacy = entry::get_entry(&op, ws_path, "legacy-note").await?;
    let structured = entry::get_entry(&op, ws_path, "structured-note").await?;
    assert_eq!(
        durable_view(&legacy),
        durable_view(&structured),
        "legacy Markdown and structured creates must agree"
    );

    // Stored 0.1 representation round-trips through the same draft boundary:
    // reopen, mutate via the other path, and compare again.
    let structured_current = entry::get_entry(&op, ws_path, "structured-note").await?;
    let revision_id = structured_current["revision_id"]
        .as_str()
        .expect("revision_id")
        .to_string();
    let updated_markdown = "---\nform: Note\ntags:\n  - inbox\n  - edited\n---\n# Shopping\n\n## Body\nBuy oat milk.\n\n## Done\ntrue\n\n## Count\n42\n\n## Labels\n- alpha\n- beta\n";
    entry::update_entry(
        &op,
        ws_path,
        "structured-note",
        updated_markdown,
        Some(&revision_id),
        "author",
        &integrity,
    )
    .await?;
    let after_legacy_update = entry::get_entry(&op, ws_path, "structured-note").await?;
    assert_eq!(
        after_legacy_update["tags"],
        serde_json::json!(["inbox", "edited"])
    );
    assert_eq!(
        after_legacy_update["sections"]["Body"],
        serde_json::Value::String("Buy oat milk.".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn structured_update_replaces_fields_like_legacy_update() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "structured-update", "/tmp").await?;
    let ws_path = "spaces/structured-update";
    ensure_note_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let markdown = "---\nform: Note\n---\n# T\n\n## Body\nhello\n\n## Done\ntrue\n\n## Count\n1\n\n## Labels\n- a\n";
    entry::create_entry(&op, ws_path, "note-1", markdown, "author", &integrity).await?;
    let current = entry::get_entry(&op, ws_path, "note-1").await?;
    let revision_id = current["revision_id"]
        .as_str()
        .expect("revision")
        .to_string();

    let mut fields = BTreeMap::new();
    fields.insert(
        "Body".to_string(),
        serde_json::Value::String("edited".to_string()),
    );
    fields.insert("Done".to_string(), serde_json::Value::Bool(false));
    fields.insert("Count".to_string(), serde_json::Value::Number(2.into()));
    fields.insert("Labels".to_string(), serde_json::json!(["a", "b"]));
    entry::update_structured_entry_authorized_with_change(
        &op,
        ws_path,
        "note-1",
        Some("T".to_string()),
        Some("Note".to_string()),
        Some(vec![]),
        fields,
        BTreeMap::new(),
        Some(&revision_id),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let updated = entry::get_entry(&op, ws_path, "note-1").await?;
    assert_eq!(
        updated["sections"]["Body"],
        serde_json::Value::String("edited".to_string())
    );
    assert_eq!(
        updated["sections"]["Done"],
        serde_json::Value::String("false".to_string())
    );
    assert_eq!(updated["tags"], serde_json::json!([]));
    Ok(())
}

#[tokio::test]
async fn unknown_fields_and_validation_agree_across_both_paths() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "structured-errors", "/tmp").await?;
    let ws_path = "spaces/structured-errors";
    ensure_note_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    // Unknown field via legacy.
    let bad_markdown = "---\nform: Note\n---\n# T\n\n## Body\nx\n\n## Nope\n1\n";
    let legacy_error = entry::create_entry(
        &op,
        ws_path,
        "bad-legacy",
        bad_markdown,
        "author",
        &integrity,
    )
    .await
    .expect_err("unknown legacy field must fail");
    let legacy_code = legacy_error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("typed")
        .code();
    assert_eq!(
        legacy_code,
        ugoite_core::error::ErrorCode::UnknownFormFields
    );

    // Unknown field via structured.
    let mut fields = BTreeMap::new();
    fields.insert(
        "Body".to_string(),
        serde_json::Value::String("x".to_string()),
    );
    fields.insert("Nope".to_string(), serde_json::Value::Number(1.into()));
    let structured_error = entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "bad-structured",
        Some("T".to_string()),
        "Note".to_string(),
        vec![],
        fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await
    .expect_err("unknown structured field must fail");
    let structured_code = structured_error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("typed")
        .code();
    assert_eq!(
        structured_code,
        ugoite_core::error::ErrorCode::UnknownFormFields
    );

    // Type error via both paths.
    let bad_type_markdown = "---\nform: Note\n---\n# T\n\n## Body\nx\n\n## Count\nnot-a-number\n";
    let legacy_type_error = entry::create_entry(
        &op,
        ws_path,
        "bad-type-legacy",
        bad_type_markdown,
        "author",
        &integrity,
    )
    .await
    .expect_err("invalid legacy type must fail");
    assert_eq!(
        legacy_type_error
            .downcast_ref::<ugoite_core::error::AppError>()
            .expect("typed")
            .code(),
        ugoite_core::error::ErrorCode::FormValidationFailed
    );
    Ok(())
}
