//! A frozen Space written by the pre-Form-binding CLI must remain readable.

mod common;

use anyhow::{Context, Result};
use common::setup_operator;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use ugoite_core::sql_query::{SavedSqlRevisionRef, SqlQueryCountRequest, SqlQueryRequest};
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::space;
use uuid::Uuid;

const EXPECTED: &str = include_str!("../../../fixtures/historical-spaces/pre-binding-sql/expected.json");

#[derive(Debug, Deserialize)]
struct FixtureExpected {
    source_sha: String,
    fixture_digest: String,
    space_id: String,
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
}

#[derive(Debug, Deserialize)]
struct ExpectedSavedSql {
    id: String,
    revision_id: String,
    name: String,
    sql: String,
    metadata: Value,
    variables: Value,
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
        while let Some(entry) = lister.try_next().await? {
            let entry_path = entry.path().to_string();
            if entry_path == path {
                continue;
            }
            if entry.metadata().mode() == EntryMode::DIR {
                pending.push(entry_path);
            } else {
                files.push((entry_path.clone(), op.read(&entry_path).await?.to_vec()));
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

#[tokio::test]
async fn prebinding_space_saved_sql_reads_executes_and_reopens_without_mutation() -> Result<()> {
    let expected: FixtureExpected = serde_json::from_str(EXPECTED)?;
    assert_eq!(
        expected.source_sha,
        "eaa2b7d7f08e3c1598d1a82d07ae5861c4a86527"
    );

    let fixture_root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/historical-spaces/pre-binding-sql");
    let checksums = std::fs::read(fixture_root.join("SHA256SUMS"))?;
    assert_eq!(sha256(&checksums), expected.fixture_digest);
    for line in std::str::from_utf8(&checksums)?.lines() {
        let (expected_hash, relative_path) = line
            .split_once("  ")
            .context("fixture checksum line uses sha256sum format")?;
        let bytes = std::fs::read(fixture_root.join(relative_path))?;
        assert_eq!(
            sha256(&bytes),
            expected_hash,
            "fixture file {relative_path}"
        );
    }

    let source_space = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/historical-spaces/pre-binding-sql/spaces")
        .join(&expected.space_id);
    let prefix = format!("spaces/{}", expected.space_id);
    let op = setup_operator()?;
    // Current Space readers require the current system bootstrap directories.
    // Preserve those empty system records, then overlay the frozen historical
    // Space bytes verbatim for the Form, Entry, Saved SQL, and history data.
    space::create_space_with_identity_and_name(
        &op,
        Uuid::parse_str(&expected.space_id)?,
        &expected.slug,
        &expected.space_name,
        "memory://pre-binding-sql-fixture",
    )
    .await?;
    write_directory(&op, &source_space, &prefix).await?;
    let before = snapshot(&op, &prefix).await?;

    let service = UgoiteService::from_operator(op.clone(), "memory://pre-binding-sql-fixture");
    let form = service
        .get_form(&expected.space_id, &expected.form.name)
        .await?;
    assert_eq!(form["id"], expected.form.id);
    assert_eq!(form["fields"]["AmountJPY"]["type"], "long");
    assert_eq!(form["fields"]["Purpose"]["type"], "string");
    assert_eq!(form["fields"]["PaidOn"]["type"], "date");

    let entries = service.list_entries(&expected.space_id).await?;
    let entry_ids = entries
        .iter()
        .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
        .collect::<BTreeSet<_>>();
    let mut all_entry_ids = expected.entries.clone();
    all_entry_ids.push(expected.saved_sql.id.clone());
    assert_eq!(entry_ids, all_entry_ids.into_iter().collect());

    let saved_sql = service
        .get_saved_sql(&expected.space_id, &expected.saved_sql.id)
        .await?;
    assert_eq!(saved_sql["id"], expected.saved_sql.id);
    assert_eq!(saved_sql["revision_id"], expected.saved_sql.revision_id);
    assert_eq!(saved_sql["name"], expected.saved_sql.name);
    assert_eq!(saved_sql["sql"], expected.saved_sql.sql);
    assert_eq!(saved_sql["metadata"], expected.saved_sql.metadata);
    assert_eq!(saved_sql["variables"], expected.saved_sql.variables);

    let source = SavedSqlRevisionRef {
        id: expected.saved_sql.id.clone(),
        revision_id: expected.saved_sql.revision_id.clone(),
    };
    let page = service
        .query_sql(
            &expected.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: Default::default(),
                limit: 100,
                continuation: None,
                saved_sql: Some(source.clone()),
            },
        )
        .await?;
    let row_ids = page
        .rows
        .iter()
        .filter_map(|row| row["_ugoite_id"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(row_ids, expected.entries);
    assert!(!page.has_more);
    assert!(page.next.is_none());

    let count = service
        .count_sql(
            &expected.space_id,
            SqlQueryCountRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: Default::default(),
                saved_sql: Some(source),
            },
        )
        .await?;
    assert_eq!(count, 3);

    let history = service
        .entry_history(&expected.space_id, &expected.saved_sql.id)
        .await?;
    let revisions = history["revisions"]
        .as_array()
        .context("Saved SQL history revisions")?;
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0]["revision_id"], expected.saved_sql.revision_id);

    let after = snapshot(&op, &prefix).await?;
    assert_eq!(after, before, "read and query must not rewrite Space bytes");
    drop(service);

    let reopened = UgoiteService::from_operator(op.clone(), "memory://pre-binding-sql-fixture");
    let saved_after_reopen = reopened
        .get_saved_sql(&expected.space_id, &expected.saved_sql.id)
        .await?;
    assert_eq!(
        saved_after_reopen["revision_id"],
        expected.saved_sql.revision_id
    );
    let reopened_page = reopened
        .query_sql(
            &expected.space_id,
            SqlQueryRequest {
                sql: String::new(),
                parameters: Map::new(),
                parameter_types: Default::default(),
                limit: 100,
                continuation: None,
                saved_sql: Some(SavedSqlRevisionRef {
                    id: expected.saved_sql.id,
                    revision_id: expected.saved_sql.revision_id,
                }),
            },
        )
        .await?;
    assert_eq!(reopened_page.rows, page.rows);
    assert_eq!(snapshot(&op, &prefix).await?, before);
    Ok(())
}
