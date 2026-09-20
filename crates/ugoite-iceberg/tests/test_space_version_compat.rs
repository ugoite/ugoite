mod common;

use common::setup_operator;
use common::LegacyServiceEntryExt;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeSet;
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::space::classify_space_version;
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::space;
use uuid::Uuid;

const FIXTURE_META: &str = include_str!(
    "../../../fixtures/spaces/0.1/spaces/019c1234-5678-7abc-8def-0123456789ab/meta.json"
);
const FIXTURE_SETTINGS: &str = include_str!(
    "../../../fixtures/spaces/0.1/spaces/019c1234-5678-7abc-8def-0123456789ab/settings.json"
);
const FIXTURE_EXPECTED: &str = include_str!("../../../fixtures/spaces/0.1/expected.json");

#[derive(Debug, Deserialize)]
struct ExpectedFixture {
    space_version: String,
    space_id: String,
    space_uid: String,
    slug: String,
    name: String,
    settings: ExpectedSettings,
    form: ExpectedForm,
    entries: Vec<ExpectedEntry>,
    history_operations: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExpectedSettings {
    default_form: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedForm {
    name: String,
    field: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedEntry {
    id: String,
    title: String,
    initial_body: String,
    body: String,
}

fn markdown(form_name: &str, title: &str, body: &str) -> String {
    format!("---\nform: {form_name}\n---\n# {title}\n\n## Body\n{body}")
}

async fn snapshot(op: &Operator, prefix: &str) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let mut snapshot = Vec::new();
    let mut pending = vec![prefix.to_string()];
    let mut visited = BTreeSet::new();
    while let Some(path) = pending.pop() {
        if !visited.insert(path.clone()) {
            continue;
        }
        let mut lister = op.lister(&path).await?;
        while let Some(entry) = lister.try_next().await? {
            let entry_path = entry.path().to_string();
            if entry_path == path {
                continue;
            }
            if entry.metadata().mode() == EntryMode::DIR {
                pending.push(entry_path);
            } else {
                snapshot.push((entry_path.clone(), op.read(&entry_path).await?.to_vec()));
            }
        }
    }
    snapshot.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(snapshot)
}

async fn stage_frozen_fixture(op: &Operator) -> anyhow::Result<ExpectedFixture> {
    let expected: ExpectedFixture = serde_json::from_str(FIXTURE_EXPECTED)?;
    let metadata: Value = serde_json::from_str(FIXTURE_META)?;
    let settings: Value = serde_json::from_str(FIXTURE_SETTINGS)?;
    assert_eq!(classify_space_version(&metadata)?.to_string(), "0.1");
    assert_eq!(metadata["space_version"], expected.space_version);
    assert_eq!(metadata["space_id"], expected.space_id);
    assert_eq!(metadata["space_uid"], expected.space_uid);
    assert_eq!(metadata["slug"], expected.slug);
    assert_eq!(metadata["name"], expected.name);
    assert!(metadata.get("schema_version").is_none());
    assert_eq!(settings["default_form"], expected.settings.default_form);

    let space_uid = Uuid::parse_str(&expected.space_uid)?;
    // The frozen fixture owns the bootstrap identity and settings. The
    // current test creates only the non-portable Iceberg scaffold needed to
    // exercise the authoritative Form/Entry reader against that frozen input.
    space::create_space_with_identity_and_name(
        op,
        space_uid,
        &expected.slug,
        &expected.name,
        "memory:///fixture",
    )
    .await?;
    let meta_path = format!("spaces/{}/meta.json", expected.space_id);
    let settings_path = format!("spaces/{}/settings.json", expected.space_id);
    op.write(&meta_path, FIXTURE_META.as_bytes().to_vec())
        .await?;
    op.write(&settings_path, FIXTURE_SETTINGS.as_bytes().to_vec())
        .await?;
    Ok(expected)
}

#[tokio::test]
async fn frozen_space_01_fixture_opens_mutates_reopens_and_preserves_history() -> anyhow::Result<()>
{
    let op = setup_operator()?;
    let expected = stage_frozen_fixture(&op).await?;
    let space_id = expected.space_id.as_str();
    let service = UgoiteService::from_operator(op.clone(), "memory://fixture");

    let opened = service.get_space(space_id).await?;
    assert_eq!(opened["space_version"], expected.space_version);
    assert_eq!(opened["space_uid"], expected.space_uid);
    assert_eq!(opened["slug"], expected.slug);
    assert_eq!(opened["name"], expected.name);
    assert_eq!(
        opened["settings"],
        serde_json::json!({"default_form": "Entry"})
    );

    let form = service.get_form(space_id, &expected.form.name).await?;
    assert_eq!(form["name"], expected.form.name);
    assert_eq!(form["fields"][&expected.form.field]["type"], "markdown");

    let mut parent_revision = None;
    for expected_entry in &expected.entries {
        let created = service
            .create_entry(
                space_id,
                &expected_entry.id,
                &markdown(
                    &expected.form.name,
                    &expected_entry.title,
                    &expected_entry.initial_body,
                ),
                "fixture-owner",
            )
            .await?;
        if expected_entry.id == "append-only-history" {
            parent_revision = created["revision_id"].as_str().map(str::to_owned);
        }
    }
    let parent_revision = parent_revision.expect("fixture update entry was created");
    let updated = service
        .update_entry(
            space_id,
            "append-only-history",
            &markdown(
                &expected.form.name,
                "Append-only History",
                "The second durable value.",
            ),
            Some(&parent_revision),
            "fixture-owner",
        )
        .await?;
    let updated_revision = updated["revision_id"]
        .as_str()
        .expect("updated entry has a revision")
        .to_owned();

    drop(service);
    let reopened_service = UgoiteService::from_operator(op, "memory://fixture");
    let reopened = reopened_service.get_space(space_id).await?;
    assert_eq!(reopened["space_version"], "0.1");
    assert_eq!(reopened["space_uid"], expected.space_uid);

    let entries = reopened_service.list_entries(space_id).await?;
    assert_eq!(entries.len(), expected.entries.len());
    for expected_entry in &expected.entries {
        let entry = entries
            .iter()
            .find(|entry| entry["id"] == expected_entry.id)
            .expect("fixture entry remains readable after reopen");
        assert_eq!(entry["form"], expected.form.name);
        assert_eq!(
            entry["properties"][&expected.form.field],
            expected_entry.body
        );
    }

    let history = reopened_service
        .entry_history(space_id, "append-only-history")
        .await?;
    let revisions = history["revisions"].as_array().expect("history revisions");
    let operations = revisions
        .iter()
        .map(|revision| {
            revision["operation"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(operations, expected.history_operations);
    assert_eq!(revisions[0]["revision_id"], parent_revision);
    assert_eq!(revisions[1]["revision_id"], updated_revision);
    Ok(())
}

#[tokio::test]
async fn unsupported_space_version_is_typed_and_does_not_write() -> anyhow::Result<()> {
    let op = setup_operator()?;
    space::create_space(&op, "unsupported-space", "/tmp").await?;
    let metadata_path = "spaces/unsupported-space/meta.json";
    let mut metadata: Value = serde_json::from_slice(&op.read(metadata_path).await?.to_vec())?;
    metadata["space_version"] = Value::String("0.2".to_string());
    op.write(metadata_path, serde_json::to_vec(&metadata)?)
        .await?;
    let before = snapshot(&op, "spaces/unsupported-space/").await?;

    let error = space::get_space_raw(&op, "unsupported-space")
        .await
        .expect_err("future Space version must fail closed");
    let app_error = error
        .downcast_ref::<AppError>()
        .expect("compatibility failure must retain its typed error");
    assert_eq!(app_error.code(), ErrorCode::UnsupportedSpaceVersion);
    assert_eq!(app_error.code_str(), "UNSUPPORTED_SPACE_VERSION");
    assert_eq!(
        app_error
            .detail()
            .and_then(|detail| detail.get("detected_space_version"))
            .and_then(Value::as_str),
        Some("0.2")
    );
    let after = snapshot(&op, "spaces/unsupported-space/").await?;
    assert_eq!(before, after, "unsupported open must not repair or migrate");
    Ok(())
}
