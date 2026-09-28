//! Frozen output from the pre-Form-binding v0.2.0 CLI.

mod common;

use anyhow::{Context, Result};
use common::setup_operator;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use ugoite_core::sql_query::{SavedSqlRevisionRef, SqlQueryCountRequest, SqlQueryRequest};
use ugoite_iceberg::{entry, service::UgoiteService, space};
use uuid::Uuid;

const EXPECTED: &str =
    include_str!("../../../fixtures/spaces/pre-binding-sql-history/expected.json");

#[derive(Debug, Deserialize)]
struct FixtureExpected {
    source_tag: String,
    source_commit: String,
    manifest_sha256: String,
    space_payload_sha256: String,
    space_id: String,
    space_version: String,
    slug: String,
    space_name: String,
    form: ExpectedForm,
    entries: Vec<String>,
    saved_sql: ExpectedSavedSql,
}

#[derive(Debug, Deserialize)]
struct ExpectedForm {
    id: String,
    name: String,
    relation: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedSavedSql {
    id: String,
    name: String,
    metadata: Value,
    revisions: Vec<ExpectedRevision>,
}

#[derive(Debug, Deserialize)]
struct ExpectedRevision {
    revision_id: String,
    parent_revision_id: Option<String>,
    sql: String,
    variables: Value,
    checksum: String,
    signature: String,
}

async fn write_directory(op: &Operator, source: &Path, target: &str) -> Result<()> {
    let mut pending = vec![source.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for item in std::fs::read_dir(&directory)? {
            let item = item?;
            let path = item.path();
            if item.file_type()?.is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path.strip_prefix(source)?;
            let key = relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            op.write(&format!("{target}/{key}"), std::fs::read(path)?)
                .await?;
        }
    }
    Ok(())
}

async fn snapshot(op: &Operator, prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let mut files = Vec::new();
    let mut pending = vec![prefix.to_string()];
    while let Some(path) = pending.pop() {
        let mut lister = op.lister(&path).await?;
        while let Some(item) = lister.try_next().await? {
            let item_path = item.path().to_string();
            if item_path == path {
                continue;
            }
            if item.metadata().mode() == EntryMode::DIR {
                pending.push(item_path);
            } else {
                files.push((item_path.clone(), op.read(&item_path).await?.to_vec()));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn verify_fixture(fixture_root: &Path, expected: &FixtureExpected) -> Result<()> {
    let manifest = std::fs::read(fixture_root.join("SHA256SUMS"))?;
    let manifest_digest = sha256(&manifest);
    assert_eq!(
        manifest_digest.as_bytes(),
        expected.manifest_sha256.as_bytes(),
        "manifest digest actual={} ({} bytes), expected={} ({} bytes)",
        manifest_digest,
        manifest_digest.len(),
        expected.manifest_sha256,
        expected.manifest_sha256.len()
    );
    let mut listed_paths = BTreeSet::new();
    let mut payload_digest = Sha256::new();
    for line in std::str::from_utf8(&manifest)?.lines() {
        let (expected_hash, relative_path) = line
            .split_once("  ")
            .context("fixture checksum line uses sha256sum format")?;
        assert!(
            listed_paths.insert(relative_path.to_owned()),
            "duplicate manifest path"
        );
        let bytes = std::fs::read(fixture_root.join(relative_path))?;
        assert_eq!(
            sha256(&bytes),
            expected_hash,
            "fixture file {relative_path}"
        );
        payload_digest.update(relative_path.as_bytes());
        payload_digest.update([0]);
        payload_digest.update(bytes);
    }
    let payload_digest = payload_digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(payload_digest, expected.space_payload_sha256);

    let mut actual_paths = BTreeSet::new();
    let mut pending = vec![fixture_root.join("spaces")];
    while let Some(directory) = pending.pop() {
        for item in std::fs::read_dir(directory)? {
            let item = item?;
            let path = item.path();
            if item.file_type()?.is_dir() {
                pending.push(path);
            } else {
                actual_paths.insert(
                    path.strip_prefix(fixture_root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    assert_eq!(
        listed_paths, actual_paths,
        "every fixture Space file is checksummed"
    );
    Ok(())
}

#[tokio::test]
async fn historical_saved_sql_revisions_read_run_and_continue_without_space_mutation() -> Result<()>
{
    let expected: FixtureExpected = serde_json::from_str(EXPECTED)?;
    assert_eq!(expected.source_tag, "v0.2.0");
    assert_eq!(
        expected.source_commit,
        "c40905a505f619aeb344ec96dc0f81b07a083bee"
    );
    assert_eq!(expected.space_version, "0.1");

    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/spaces/pre-binding-sql-history");
    verify_fixture(&fixture_root, &expected)?;

    let source_space = fixture_root.join("spaces").join(&expected.space_id);
    let prefix = format!("spaces/{}", expected.space_id);
    let op = setup_operator()?;
    // Bootstrap only current system records, then overlay the frozen Space
    // bytes. All Forms, Entries, SQL revisions, and their integrity stay old.
    space::create_space_with_identity_and_name(
        &op,
        Uuid::parse_str(&expected.space_id)?,
        &expected.slug,
        &expected.space_name,
        "memory://pre-binding-sql-history-fixture",
    )
    .await?;
    write_directory(&op, &source_space, &prefix).await?;
    let before_space = snapshot(&op, &prefix).await?;

    let service =
        UgoiteService::from_operator(op.clone(), "memory://pre-binding-sql-history-fixture");
    let form = service
        .get_form(&expected.space_id, &expected.form.name)
        .await?;
    assert_eq!(form["id"], expected.form.id);
    assert_eq!(form["sql_relation"], expected.form.relation);
    assert_eq!(form["fields"]["AmountJPY"]["type"], "long");

    let entries = service.list_entries(&expected.space_id).await?;
    let entry_ids = entries
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut expected_entry_ids = expected.entries.iter().cloned().collect::<BTreeSet<_>>();
    expected_entry_ids.insert(expected.saved_sql.id.clone());
    assert_eq!(entry_ids, expected_entry_ids);

    let current = service
        .get_saved_sql(&expected.space_id, &expected.saved_sql.id)
        .await?;
    let listed = service
        .list_saved_sql_operator_unscoped(&expected.space_id)
        .await?;
    let latest = expected
        .saved_sql
        .revisions
        .last()
        .context("latest revision")?;
    assert_eq!(current["id"], expected.saved_sql.id);
    assert_eq!(current["revision_id"], latest.revision_id);
    assert_eq!(current["name"], expected.saved_sql.name);
    assert_eq!(current["sql"], latest.sql);
    assert_eq!(current["variables"], latest.variables);
    assert_eq!(current["metadata"], expected.saved_sql.metadata);
    assert!(
        current["metadata"].is_null(),
        "historical metadata stays absent"
    );
    let listed_current = listed
        .iter()
        .find(|item| item["id"] == expected.saved_sql.id)
        .context("frozen Saved SQL appears in list")?;
    assert_eq!(listed_current["revision_id"], latest.revision_id);

    let workspace_path = service.workspace_path(&expected.space_id);
    let mut before_revisions = Vec::new();
    for revision in &expected.saved_sql.revisions {
        before_revisions.push(
            entry::get_entry_revision(
                service.operator(),
                &workspace_path,
                &expected.saved_sql.id,
                &revision.revision_id,
            )
            .await?,
        );
    }
    for (revision, raw) in expected.saved_sql.revisions.iter().zip(&before_revisions) {
        assert_eq!(raw["revision_id"], revision.revision_id);
        assert_eq!(
            raw["parent_revision_id"].as_str(),
            revision.parent_revision_id.as_deref()
        );
        assert_eq!(raw["fields"]["sql"], revision.sql);
        assert_eq!(raw["fields"]["variables"], revision.variables);
        assert_eq!(raw["extra_attributes"]["metadata"], Value::Null);
        assert_eq!(raw["integrity"]["checksum"], revision.checksum);
        assert_eq!(raw["integrity"]["signature"], revision.signature);
    }

    let historical = &expected.saved_sql.revisions[0];
    let historical_ref = SavedSqlRevisionRef {
        id: expected.saved_sql.id.clone(),
        revision_id: historical.revision_id.clone(),
    };
    let first = service
        .query_sql(
            &expected.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::from_iter([("minimum".into(), json!(1000))]),
                parameter_types: Default::default(),
                limit: 1,
                continuation: None,
                saved_sql: Some(historical_ref.clone()),
            },
        )
        .await?;
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0]["_ugoite_id"], "expense-01");
    assert_eq!(first.rows[0]["field_100"], 1200);
    assert!(first.has_more);
    let second = service
        .query_sql(
            &expected.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::from_iter([("minimum".into(), json!(1000))]),
                parameter_types: Default::default(),
                limit: 1,
                continuation: first.next.clone(),
                saved_sql: Some(historical_ref.clone()),
            },
        )
        .await?;
    assert_eq!(second.rows.len(), 1);
    assert_eq!(second.rows[0]["_ugoite_id"], "expense-02");
    assert_eq!(second.rows[0]["field_100"], 3200);
    assert!(!second.has_more);
    let count = service
        .count_sql(
            &expected.space_id,
            SqlQueryCountRequest {
                sql: String::new(),
                parameters: Map::from_iter([("minimum".into(), json!(1000))]),
                parameter_types: Default::default(),
                saved_sql: Some(historical_ref),
            },
        )
        .await?;
    assert_eq!(count, 2);

    // Selecting the latest ID@revision executes its distinct saved body too.
    let latest_page = service
        .query_sql(
            &expected.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::from_iter([("maximum".into(), json!(3000))]),
                parameter_types: Default::default(),
                limit: 10,
                continuation: None,
                saved_sql: Some(SavedSqlRevisionRef {
                    id: expected.saved_sql.id.clone(),
                    revision_id: latest.revision_id.clone(),
                }),
            },
        )
        .await?;
    assert_eq!(
        latest_page
            .rows
            .iter()
            .map(|row| row["_ugoite_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["expense-02"]
    );
    assert_eq!(latest_page.rows[0]["field_100"], 3200);

    let history = service
        .entry_history(&expected.space_id, &expected.saved_sql.id)
        .await?;
    let history_revisions = history["revisions"]
        .as_array()
        .context("saved SQL history")?;
    assert_eq!(history_revisions.len(), expected.saved_sql.revisions.len());
    for (actual, expected_revision) in history_revisions.iter().zip(&expected.saved_sql.revisions) {
        assert_eq!(actual["revision_id"], expected_revision.revision_id);
        assert_eq!(actual["checksum"], expected_revision.checksum);
        assert_eq!(actual["signature"], expected_revision.signature);
    }
    let mut after_revisions = Vec::new();
    for revision in &expected.saved_sql.revisions {
        after_revisions.push(
            entry::get_entry_revision(
                service.operator(),
                &workspace_path,
                &expected.saved_sql.id,
                &revision.revision_id,
            )
            .await?,
        );
    }
    assert_eq!(after_revisions, before_revisions);
    assert_eq!(snapshot(&op, &prefix).await?, before_space);
    Ok(())
}
