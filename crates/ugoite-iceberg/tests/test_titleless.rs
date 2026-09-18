mod common;

use anyhow::{Context, Result};
use common::setup_operator;
use std::collections::BTreeMap;
use ugoite_domain::form::{FormDefinition, FormVersion};
use ugoite_domain::id::{FormId, SpaceId};
use ugoite_iceberg::integrity::FakeIntegrityProvider;
use ugoite_iceberg::{entry, form, publication_context, space, IcebergWorkspace};
use uuid::Uuid;

const FIXTURE: &str = include_str!("fixtures/v0.1-knowledge.json");

/// REQ-ENTRY-011 legacy-read: freeze the legacy title-bearing v0.1 fixture.
/// New title-less work must not rewrite this fixture to hide compatibility.
#[tokio::test]
async fn legacy_title_bearing_fixture_stays_frozen() -> Result<()> {
    let fixture: serde_json::Value =
        serde_json::from_str(FIXTURE).context("parse v0.1 Knowledge fixture")?;
    assert_eq!(fixture["fixture_version"], 1);
    assert_eq!(fixture["release"], "v0.1");
    let entries = fixture["space"]["entries"]
        .as_array()
        .context("fixture entries")?;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["id"], "portable-knowledge");
    assert_eq!(entries[0]["title"], "Portable Knowledge");
    assert_eq!(entries[1]["id"], "append-only-history");
    assert_eq!(entries[1]["title"], "Append-only History");
    assert_eq!(
        fixture["space"]["update"]["expected_history_operations"],
        serde_json::json!(["upsert", "upsert"])
    );
    Ok(())
}

/// REQ-ENTRY-011 no-implicit-title-field: a newly created Form must not
/// acquire a `ugoite_entry_title` column. Field ID 13 may stay vacant.
#[tokio::test]
async fn new_form_has_no_implicit_title_column() -> Result<()> {
    let space_id = SpaceId::from(Uuid::now_v7());
    let workspace =
        IcebergWorkspace::memory_for_tests(space_id, "memory://titleless-probe").await?;
    let form = FormDefinition {
        id: FormId::from(Uuid::from_u128(77)),
        version: FormVersion::new(1).unwrap(),
        name: "Reading".into(),
        description: None,
        fields: Vec::new(),
        allow_extra_attributes: false,
        extension_metadata: BTreeMap::new(),
    };
    workspace
        .commit(publication_context(
            Uuid::new_v4().to_string(),
            "test.form.create",
            &form,
        )?)?
        .create_form(&form)
        .await?;
    let table = workspace
        .catalog_for_testing()
        .load_table(&iceberg::TableIdent::new(
            workspace.namespace_for_testing().clone(),
            ugoite_iceberg::physical_form_name(form.id),
        ))
        .await?;
    assert!(
        table
            .metadata()
            .current_schema()
            .field_by_name("ugoite_entry_title")
            .is_none(),
        "new Form storage must not contain ugoite_entry_title"
    );
    Ok(())
}

async fn ensure_reading_form(op: &opendal::Operator, ws_path: &str) -> Result<()> {
    form::upsert_form(
        op,
        ws_path,
        &serde_json::json!({
            "name": "Reading",
            "fields": {
                "temperature": {"type": "float"},
                "note": {"type": "string"}
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;
    Ok(())
}

/// REQ-ENTRY-011 entry-id-identity: structured create without a title stores
/// no synthetic title; the entry_id is never copied into the title.
#[tokio::test]
async fn structured_create_without_title_stores_no_synthetic_title() -> Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "titleless-structured", "/tmp").await?;
    let ws_path = "spaces/titleless-structured";
    ensure_reading_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let mut fields = BTreeMap::new();
    fields.insert("temperature".to_string(), serde_json::json!(21.4));
    fields.insert("note".to_string(), serde_json::json!("lab"));
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "reading-01",
        None,
        "Reading".to_string(),
        Vec::new(),
        fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let stored = entry::get_entry(&op, ws_path, "reading-01").await?;
    assert_eq!(stored["id"], "reading-01");
    assert_eq!(
        stored["title"], "",
        "structured create without title must not synthesize entry_id"
    );
    assert!(stored["title"] != "reading-01");
    assert!(!stored["content"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .any(|line| line.starts_with("# ")));
    Ok(())
}

/// REQ-ENTRY-011 entry-id-identity: raw Markdown without an H1 creates a
/// title-less Entry and renders no synthetic H1.
#[tokio::test]
async fn markdown_create_without_h1_is_title_less() -> Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "titleless-markdown", "/tmp").await?;
    let ws_path = "spaces/titleless-markdown";
    ensure_reading_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let markdown = "---\nform: Reading\n---\n## temperature\n21.4\n";
    entry::create_entry(&op, ws_path, "reading-02", markdown, "author", &integrity).await?;

    let stored = entry::get_entry(&op, ws_path, "reading-02").await?;
    assert_eq!(stored["title"], "");
    assert!(stored["title"] != "reading-02");
    assert!(!stored["content"]
        .as_str()
        .unwrap_or_default()
        .lines()
        .any(|line| line.starts_with("# ")));
    Ok(())
}

/// REQ-ENTRY-011 legacy-read: a non-empty compatibility title sent to a
/// title-less Form is preserved (extension_metadata) and survives reopen.
#[tokio::test]
async fn compat_title_on_titleless_form_is_preserved() -> Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "titleless-compat", "/tmp").await?;
    let ws_path = "spaces/titleless-compat";
    ensure_reading_form(&op, ws_path).await?;
    let integrity = FakeIntegrityProvider;

    let mut fields = BTreeMap::new();
    fields.insert("temperature".to_string(), serde_json::json!(19.0));
    fields.insert("note".to_string(), serde_json::json!("lab"));
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "reading-03",
        Some("Legacy Label".to_string()),
        "Reading".to_string(),
        Vec::new(),
        fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let stored = entry::get_entry(&op, ws_path, "reading-03").await?;
    assert_eq!(stored["title"], "Legacy Label");

    // History survives a close/reopen-equivalent read of the same workspace.
    let history = entry::get_entry_history(&op, ws_path, "reading-03").await?;
    assert_eq!(history["revisions"].as_array().map(|v| v.len()), Some(1));
    Ok(())
}

/// REQ-ENTRY-011 user-defined-title-field: a Form-declared `title` string
/// field behaves like any normal field.
#[tokio::test]
async fn form_defined_title_field_is_a_normal_field() -> Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "titleless-book", "/tmp").await?;
    let ws_path = "spaces/titleless-book";
    form::upsert_form(
        &op,
        ws_path,
        &serde_json::json!({
            "name": "Book",
            "fields": {
                "title": {"type": "string"},
                "writer": {"type": "string"}
            },
            "allow_extra_attributes": "deny",
        }),
    )
    .await?;
    let integrity = FakeIntegrityProvider;

    let mut fields = BTreeMap::new();
    fields.insert("title".to_string(), serde_json::json!("Dune"));
    fields.insert("writer".to_string(), serde_json::json!("Herbert"));
    entry::create_structured_entry_with_scopes_and_change(
        &op,
        ws_path,
        "book-01",
        None,
        "Book".to_string(),
        Vec::new(),
        fields,
        BTreeMap::new(),
        "author",
        &integrity,
        None,
        None,
    )
    .await?;

    let stored = entry::get_entry(&op, ws_path, "book-01").await?;
    assert_eq!(stored["title"], "", "Entry-level title stays empty");
    assert_eq!(stored["sections"]["title"], "Dune");
    assert_eq!(stored["sections"]["writer"], "Herbert");
    Ok(())
}
