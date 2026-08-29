use anyhow::{Context, Result};
use serde::Deserialize;
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::{iceberg_store, RevisionView};

const FIXTURE: &str = include_str!("fixtures/v0.1-knowledge.json");

#[derive(Debug, Deserialize)]
struct KnowledgeFixture {
    fixture_version: u32,
    release: String,
    space: SpaceFixture,
}

#[derive(Debug, Deserialize)]
struct SpaceFixture {
    slug: String,
    form_name: String,
    form_field: String,
    entries: Vec<FixtureEntry>,
    update: FixtureUpdate,
    expected_current_entry_count: usize,
}

#[derive(Debug, Deserialize)]
struct FixtureEntry {
    id: String,
    title: String,
    body: String,
    author: String,
}

#[derive(Debug, Deserialize)]
struct FixtureUpdate {
    entry_id: String,
    title: String,
    body: String,
    expected_history_operations: Vec<String>,
    expected_history_length: usize,
}

fn markdown(form_name: &str, title: &str, body: &str) -> String {
    format!("---\nform: {form_name}\n---\n# {title}\n\n## Body\n{body}")
}

fn fixture() -> Result<KnowledgeFixture> {
    serde_json::from_str(FIXTURE).context("parse v0.1 Knowledge fixture")
}

/// REQ-STO-014: v0.1 Knowledge remains semantically readable through the
/// authoritative Space, publication, and append-only history paths.
#[tokio::test]
async fn v01_knowledge_fixture_is_semantically_recoverable() -> Result<()> {
    let fixture = fixture()?;
    assert_eq!(fixture.fixture_version, 1);
    assert_eq!(fixture.release, "v0.1");
    assert_eq!(
        fixture.space.entries.len(),
        fixture.space.expected_current_entry_count
    );

    let service = UgoiteService::new("memory://v01-knowledge-compatibility")?;
    service.create_space(&fixture.space.slug).await?;

    let form = service
        .get_form(&fixture.space.slug, &fixture.space.form_name)
        .await?;
    assert_eq!(form["name"], fixture.space.form_name);
    assert_eq!(
        form["fields"][&fixture.space.form_field]["type"],
        "markdown"
    );

    let mut update_parent_revision = None;
    for entry in &fixture.space.entries {
        let created = service
            .create_entry(
                &fixture.space.slug,
                &entry.id,
                &markdown(&fixture.space.form_name, &entry.title, &entry.body),
                &entry.author,
            )
            .await?;
        if entry.id == fixture.space.update.entry_id {
            update_parent_revision = created["revision_id"].as_str().map(str::to_owned);
        }
    }

    let workspace = iceberg_store::native_workspace(
        service.operator(),
        &service.workspace_path(&fixture.space.slug),
    )
    .await?;
    let before_update = workspace.current_publication().await?;
    let parent_revision = update_parent_revision.context("fixture update entry was not created")?;
    service
        .update_entry(
            &fixture.space.slug,
            &fixture.space.update.entry_id,
            &markdown(
                &fixture.space.form_name,
                &fixture.space.update.title,
                &fixture.space.update.body,
            ),
            Some(&parent_revision),
            "fixture-owner",
        )
        .await?;
    let after_update = workspace.current_publication().await?;
    assert!(before_update.generation < after_update.generation);

    let mut entries = service.list_entries(&fixture.space.slug).await?;
    entries.sort_by(|left, right| left["id"].as_str().cmp(&right["id"].as_str()));
    assert_eq!(entries.len(), fixture.space.expected_current_entry_count);
    for expected in &fixture.space.entries {
        let entry = entries
            .iter()
            .find(|entry| entry["id"] == expected.id)
            .with_context(|| format!("fixture entry {} was not readable", expected.id))?;
        let is_updated = expected.id == fixture.space.update.entry_id;
        assert_eq!(entry["id"], expected.id);
        assert_eq!(entry["form"], fixture.space.form_name);
        assert_eq!(
            entry["title"],
            if is_updated {
                fixture.space.update.title.as_str()
            } else {
                expected.title.as_str()
            }
        );
        assert_eq!(
            entry["properties"][&fixture.space.form_field],
            if is_updated {
                fixture.space.update.body.as_str()
            } else {
                expected.body.as_str()
            }
        );
    }

    let history = service
        .entry_history(&fixture.space.slug, &fixture.space.update.entry_id)
        .await?;
    let revisions = history["revisions"]
        .as_array()
        .context("fixture history did not return revisions")?;
    assert_eq!(
        revisions.len(),
        fixture.space.update.expected_history_length
    );
    let operations = revisions
        .iter()
        .map(|revision| {
            revision["operation"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(operations, fixture.space.update.expected_history_operations);

    let form = workspace
        .list_forms()
        .await?
        .into_iter()
        .find(|candidate| candidate.name == fixture.space.form_name)
        .context("fixture form was not present in the publication catalog")?;
    let before_entries = workspace
        .read_revision_view_at_publication(&before_update, form.id, RevisionView::Current)
        .await?;
    let after_entries = workspace
        .read_revision_view_at_publication(&after_update, form.id, RevisionView::Current)
        .await?;
    assert_eq!(
        before_entries.len(),
        fixture.space.expected_current_entry_count
    );
    assert_eq!(
        after_entries.len(),
        fixture.space.expected_current_entry_count
    );
    let checkpoint = workspace.resolve_publication(&after_update).await?;
    assert_eq!(checkpoint.catalog_generation, after_update.generation);
    assert!(workspace
        .forms_at_publication(&after_update)
        .await?
        .iter()
        .any(|candidate| candidate.id == form.id && candidate.name == fixture.space.form_name));

    Ok(())
}
