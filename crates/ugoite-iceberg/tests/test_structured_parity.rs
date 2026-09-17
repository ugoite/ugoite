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

    // Same type error via the structured path: identical code, no persistence.
    let mut bad_type_fields = BTreeMap::new();
    bad_type_fields.insert(
        "Body".to_string(),
        serde_json::Value::String("x".to_string()),
    );
    bad_type_fields.insert(
        "Count".to_string(),
        serde_json::Value::String("not-a-number".to_string()),
    );
    let structured_type_error = entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "bad-type-structured",
        Some("T".to_string()),
        "Note".to_string(),
        vec![],
        bad_type_fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await
    .expect_err("invalid structured type must fail");
    let structured_type_app_error = structured_type_error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("typed");
    assert_eq!(
        structured_type_app_error.code(),
        ugoite_core::error::ErrorCode::FormValidationFailed
    );
    assert!(
        entry::list_entries(&op, ws_path)
            .await?
            .iter()
            .all(|entry| entry["id"] != "bad-type-structured"),
        "failed structured validation must not persist"
    );
    Ok(())
}

#[tokio::test]
async fn temporal_values_and_revision_parentage_agree_across_both_paths() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "structured-temporal", "/tmp").await?;
    let ws_path = "spaces/structured-temporal";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Task",
            "fields": {
                "Summary": {"type": "string"},
                "Due": {"type": "date"},
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;

    // Same temporal meaning from raw Markdown and structured input.
    let markdown = "---\nform: Task\n---\n# Launch\n\n## Summary\nShip it.\n\n## Due\n2026-01-15\n";
    entry::create_entry(&op, ws_path, "legacy-task", markdown, "author", &integrity).await?;
    let mut fields = BTreeMap::new();
    fields.insert(
        "Summary".to_string(),
        serde_json::Value::String("Ship it.".to_string()),
    );
    fields.insert(
        "Due".to_string(),
        serde_json::Value::String("2026-01-15".to_string()),
    );
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "structured-task",
        Some("Launch".to_string()),
        "Task".to_string(),
        vec![],
        fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;
    let legacy = entry::get_entry(&op, ws_path, "legacy-task").await?;
    let structured = entry::get_entry(&op, ws_path, "structured-task").await?;
    assert_eq!(legacy["form"], structured["form"]);
    assert_eq!(
        legacy["sections"]["Due"], structured["sections"]["Due"],
        "temporal values must share one normalized meaning"
    );

    // Revision parentage survives a cross-path update with Change identity.
    let created_revision = structured["revision_id"]
        .as_str()
        .expect("revision_id")
        .to_string();
    let mut update_fields = BTreeMap::new();
    update_fields.insert(
        "Summary".to_string(),
        serde_json::Value::String("Ship it soon.".to_string()),
    );
    update_fields.insert(
        "Due".to_string(),
        serde_json::Value::String("2026-01-16".to_string()),
    );
    entry::update_structured_entry_authorized_with_change(
        &op,
        ws_path,
        "structured-task",
        Some("Launch".to_string()),
        Some("Task".to_string()),
        Some(vec![]),
        update_fields,
        BTreeMap::new(),
        Some(&created_revision),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;
    let updated = entry::get_entry(&op, ws_path, "structured-task").await?;
    let updated_revision = updated["revision_id"]
        .as_str()
        .expect("updated revision_id")
        .to_string();
    assert_ne!(updated_revision, created_revision);
    let history = entry::get_entry_history(&op, ws_path, "structured-task").await?;
    let revisions = history["revisions"].as_array().expect("history revisions");
    assert!(
        revisions.len() >= 2,
        "history must retain both committed revisions"
    );
    let updated_history =
        entry::get_entry_revision(&op, ws_path, "structured-task", &updated_revision).await?;
    assert_eq!(
        updated_history["parent_revision_id"],
        serde_json::Value::String(created_revision.clone()),
        "revision parentage must link the update to its parent"
    );
    assert!(
        revisions.iter().all(|revision| revision
            .get("change_id")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|change_id| !change_id.is_empty())),
        "every committed revision must surface its Change identity"
    );
    Ok(())
}

#[tokio::test]
async fn lossy_markdown_is_rejected_before_any_entry_mutation() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "markdown-loss", "/tmp").await?;
    let ws_path = "spaces/markdown-loss";
    ensure_note_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let lossy = "---\nform: Note\n---\n# T\n\nThis preamble has no field.\n\n## Body\nkept\n";

    let error = entry::create_entry(&op, ws_path, "lossy", lossy, "author", &integrity)
        .await
        .expect_err("lossy Markdown must not be persisted");
    let app_error = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("loss must remain a typed application error");
    assert_eq!(
        app_error.code(),
        ugoite_core::error::ErrorCode::MarkdownConversionLoss
    );
    assert!(app_error.detail().is_some_and(|detail| {
        detail["diagnostics"][0]["code"] == "markdown_unassigned_preamble"
    }));
    assert!(entry::list_entries(&op, ws_path).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn unknown_frontmatter_keys_follow_the_same_deny_path() -> anyhow::Result<()> {
    // Unknown frontmatter is preserved via extra_attributes where portable;
    // otherwise it rejects pre-persistence with the same UNKNOWN_FORM_FIELDS
    // code the structured path uses. Nothing is ever silently ignored.
    let op = setup_operator()?;
    space::create_space(&op, "frontmatter-extras", "/tmp").await?;
    let ws_path = "spaces/frontmatter-extras";
    let integrity = FakeIntegrityProvider;

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Strict",
            "fields": {
                "Body": {"type": "string"},
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;
    let denied = "---\nform: Strict\nScratch: keep\n---\n# T\n\n## Body\nx\n";
    let error = entry::create_entry(&op, ws_path, "denied", denied, "author", &integrity)
        .await
        .expect_err("unknown frontmatter key must fail under deny");
    assert_eq!(
        error
            .downcast_ref::<ugoite_core::error::AppError>()
            .expect("typed")
            .code(),
        ugoite_core::error::ErrorCode::UnknownFormFields
    );
    assert!(entry::list_entries(&op, ws_path).await?.is_empty());

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Lenient",
            "fields": {
                "Body": {"type": "string"},
            },
            "allow_extra_attributes": "allow_columns",
        }),
    )
    .await?;
    let allowed = "---\nform: Lenient\nScratch: keep\n---\n# T\n\n## Body\nx\n";
    entry::create_entry(&op, ws_path, "allowed", allowed, "author", &integrity).await?;
    let stored = entry::get_entry(&op, ws_path, "allowed").await?;
    assert_eq!(
        stored["extra_attributes"]["Scratch"],
        serde_json::Value::String("keep".to_string()),
        "portable unknown frontmatter must survive as an extra attribute"
    );
    Ok(())
}

#[tokio::test]
async fn duplicate_field_sources_are_diagnostics_not_precedence() -> anyhow::Result<()> {
    // The same key in `fields` and `extra_attributes` — or an explicit extra
    // shadowing a real field name — is an INVALID_INPUT diagnostic naming the
    // duplicate keys. Neither side silently wins and nothing is persisted.
    let op = setup_operator()?;
    space::create_space(&op, "structured-duplicates", "/tmp").await?;
    let ws_path = "spaces/structured-duplicates";
    ensure_note_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let mut fields = BTreeMap::new();
    fields.insert(
        "Body".to_string(),
        serde_json::Value::String("x".to_string()),
    );
    let mut extra = BTreeMap::new();
    extra.insert(
        "Body".to_string(),
        serde_json::Value::String("shadow".to_string()),
    );
    let error = entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "dupe",
        Some("T".to_string()),
        "Note".to_string(),
        vec![],
        fields,
        extra,
        "author",
        &integrity,
        None,
        None,
    )
    .await
    .expect_err("fields/extra_attributes overlap must fail");
    let app_error = error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("typed");
    assert_eq!(
        app_error.code(),
        ugoite_core::error::ErrorCode::InvalidInput
    );
    // Adapter boundary pins the exact stable shape: one code
    // (`INVALID_INPUT`) and one detail object carrying only the lexically
    // sorted duplicate key array. Message text is not contract.
    assert_eq!(
        app_error.code(),
        ugoite_core::error::ErrorCode::InvalidInput
    );
    assert_eq!(app_error.code().as_str(), "INVALID_INPUT");
    assert_eq!(
        app_error.detail().expect("detail").clone(),
        serde_json::json!({"duplicate_fields": ["Body"]})
    );

    // Multiple duplicates keep the same exact shape in lexical order.
    let mut multi_fields = BTreeMap::new();
    multi_fields.insert("Done".to_string(), serde_json::Value::Bool(true));
    let mut multi_extra = BTreeMap::new();
    multi_extra.insert("Done".to_string(), serde_json::Value::Bool(false));
    multi_extra.insert("Count".to_string(), serde_json::Value::Number(1.into()));
    let multi_error = entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "dupe-multi",
        Some("T".to_string()),
        "Note".to_string(),
        vec![],
        multi_fields,
        multi_extra,
        "author",
        &integrity,
        None,
        None,
    )
    .await
    .expect_err("multiple overlaps must fail");
    let multi_app_error = multi_error
        .downcast_ref::<ugoite_core::error::AppError>()
        .expect("typed");
    assert_eq!(multi_app_error.code().as_str(), "INVALID_INPUT");
    assert_eq!(
        multi_app_error.detail().expect("detail").clone(),
        serde_json::json!({"duplicate_fields": ["Count", "Done"]})
    );

    // An explicit extra naming a real field is the same caller bug even when
    // `fields` does not claim the key: it must not be silently dropped.
    let mut shadow_extra = BTreeMap::new();
    shadow_extra.insert("Count".to_string(), serde_json::Value::Number(1.into()));
    let shadow_error = entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "shadow",
        Some("T".to_string()),
        "Note".to_string(),
        vec![],
        BTreeMap::new(),
        shadow_extra,
        "author",
        &integrity,
        None,
        None,
    )
    .await
    .expect_err("extra shadowing a real field must fail");
    assert_eq!(
        shadow_error
            .downcast_ref::<ugoite_core::error::AppError>()
            .expect("typed")
            .code(),
        ugoite_core::error::ErrorCode::InvalidInput
    );
    assert!(entry::list_entries(&op, ws_path).await?.is_empty());
    Ok(())
}

#[tokio::test]
async fn change_and_run_grouping_agree_across_both_paths() -> anyhow::Result<()> {
    // Change ID propagation, revision parentage, and Run grouping flow
    // through the same shared boundary for raw Markdown and structured
    // payloads: no separate mutation implementation exists per input style.
    use ugoite_domain::change::{ChangeCommand, RunId};

    let op = setup_operator()?;
    space::create_space(&op, "structured-change", "/tmp").await?;
    let ws_path = "spaces/structured-change";
    ensure_note_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;
    let run_id = RunId::new("run-structured-parity").expect("run id");

    let create_change = ChangeCommand {
        change_id: "change-raw-create".to_string(),
        run_id: Some(run_id.clone()),
        actor_principal_id: "author".to_string(),
        message: Some("raw create".to_string()),
        reverts_change_id: None,
        created_at_micros: 1,
    };
    let markdown = "---\nform: Note\n---\n# T\n\n## Body\nhello\n\n## Done\ntrue\n\n## Count\n1\n\n## Labels\n- a\n";
    entry::create_entry_with_scopes_and_change(
        &op,
        ws_path,
        "note-1",
        markdown,
        "author",
        &integrity,
        None,
        Some(create_change),
    )
    .await?;
    let created = entry::get_entry(&op, ws_path, "note-1").await?;
    let created_revision = created["revision_id"]
        .as_str()
        .expect("revision_id")
        .to_string();

    let mut fields = BTreeMap::new();
    fields.insert(
        "Body".to_string(),
        serde_json::Value::String("edited".to_string()),
    );
    fields.insert("Done".to_string(), serde_json::Value::Bool(false));
    fields.insert("Count".to_string(), serde_json::Value::Number(2.into()));
    fields.insert("Labels".to_string(), serde_json::json!(["a", "b"]));
    let update_change = ChangeCommand {
        change_id: "change-structured-update".to_string(),
        run_id: Some(run_id),
        actor_principal_id: "author".to_string(),
        message: Some("structured update".to_string()),
        reverts_change_id: None,
        created_at_micros: 2,
    };
    entry::update_structured_entry_authorized_with_change(
        &op,
        ws_path,
        "note-1",
        Some("T".to_string()),
        Some("Note".to_string()),
        Some(vec![]),
        fields,
        BTreeMap::new(),
        Some(&created_revision),
        "author",
        &integrity,
        None,
        Some(update_change),
    )
    .await?;

    let history = entry::get_entry_history(&op, ws_path, "note-1").await?;
    let revisions = history["revisions"].as_array().expect("history revisions");
    assert_eq!(
        revisions.len(),
        2,
        "both paths must append one revision each"
    );
    let change_ids: Vec<&str> = revisions
        .iter()
        .map(|revision| {
            revision
                .get("change_id")
                .and_then(serde_json::Value::as_str)
                .expect("change identity")
        })
        .collect();
    assert!(change_ids.contains(&"change-raw-create"));
    assert!(change_ids.contains(&"change-structured-update"));
    let updated = entry::get_entry(&op, ws_path, "note-1").await?;
    let updated_revision = updated["revision_id"]
        .as_str()
        .expect("updated revision_id")
        .to_string();
    let updated_history =
        entry::get_entry_revision(&op, ws_path, "note-1", &updated_revision).await?;
    assert_eq!(
        updated_history["parent_revision_id"],
        serde_json::Value::String(created_revision),
        "structured update must parent the raw create revision"
    );
    Ok(())
}

#[tokio::test]
async fn reference_and_asset_ids_agree_across_both_paths() -> anyhow::Result<()> {
    // Reopening a raw Markdown entry and a structured entry must produce the
    // same reference/asset IDs: both inputs share one coercion boundary.
    let op = setup_operator()?;
    space::create_space(&op, "structured-refs", "/tmp").await?;
    let ws_path = "spaces/structured-refs";
    let integrity = FakeIntegrityProvider;

    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Project",
            "fields": {
                "Summary": {"type": "string"},
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;
    entry::create_entry(
        &op,
        ws_path,
        "project-alpha",
        "---\nform: Project\n---\n# Alpha\n\n## Summary\nPrimary\n",
        "author",
        &integrity,
    )
    .await?;
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Work",
            "fields": {
                "Owner": {"type": "row_reference", "target_form": "Project"},
                "File": {"type": "asset_reference"},
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;

    let asset_id = "01900000-0000-7000-8000-000000000001";
    op.write(
        &format!("{ws_path}/assets/{asset_id}"),
        "asset-bytes".as_bytes().to_vec(),
    )
    .await
    .expect("stage asset bytes");
    let reference = serde_json::json!({
        "asset_id": asset_id,
        "name": "spec.txt",
        "media_type": "text/plain",
        "size_bytes": 11,
        "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    });
    let markdown = format!(
        "---\nform: Work\n---\n# Job\n\n## Owner\nproject-alpha\n\n## File\n{}\n",
        serde_json::to_string(&reference).expect("reference json")
    );
    entry::create_entry(&op, ws_path, "legacy-job", &markdown, "author", &integrity).await?;

    let mut fields = BTreeMap::new();
    fields.insert(
        "Owner".to_string(),
        serde_json::Value::String("project-alpha".to_string()),
    );
    fields.insert("File".to_string(), reference.clone());
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "structured-job",
        Some("Job".to_string()),
        "Work".to_string(),
        vec![],
        fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let legacy = entry::get_entry(&op, ws_path, "legacy-job").await?;
    let structured = entry::get_entry(&op, ws_path, "structured-job").await?;
    assert_eq!(
        legacy["sections"]["Owner"], structured["sections"]["Owner"],
        "row_reference IDs must share one stored meaning"
    );
    assert_eq!(
        legacy["sections"]["File"], structured["sections"]["File"],
        "asset reference IDs must share one stored meaning"
    );
    assert_eq!(
        structured["sections"]["Owner"],
        serde_json::Value::String("project-alpha".to_string())
    );
    Ok(())
}
