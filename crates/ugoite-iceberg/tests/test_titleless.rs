use anyhow::{Context, Result};
use std::collections::BTreeMap;
use ugoite_domain::form::{FormDefinition, FormVersion};
use ugoite_domain::id::{FormId, SpaceId};
use ugoite_iceberg::{publication_context, IcebergWorkspace};
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

/// REQ-ENTRY-011 no-implicit-title-field (target state, implemented in PR2):
/// a newly created Form must not acquire a `ugoite_entry_title` column.
/// Kept ignored until the dual-schema writer lands so PR1 stays green.
#[tokio::test]
#[ignore]
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
