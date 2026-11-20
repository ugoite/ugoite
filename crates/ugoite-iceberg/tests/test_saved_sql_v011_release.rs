//! A published v0.1.1 CLI-created named Saved SQL Space stays readable.

mod common;

use anyhow::{Context, Result};
use common::setup_operator;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use serde::Deserialize;
use serde_json::{json, Map};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use ugoite_core::sql_query::{SavedSqlRevisionRef, SqlQueryRequest};
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::space;
use uuid::Uuid;

const EXPECTED: &str =
    include_str!("../../../fixtures/historical-spaces/v0.1.1-named-sql/expected.json");

#[derive(Debug, Deserialize)]
struct Expected {
    space_id: String,
    slug: String,
    space_name: String,
    saved_sql_id: String,
    name: String,
    current_revision_id: String,
    previous_revision_id: String,
    sql: String,
    variables: serde_json::Value,
}

async fn write_directory(op: &Operator, source: &Path, target: &str) -> Result<()> {
    let mut pending = vec![source.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for item in std::fs::read_dir(directory)? {
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
async fn published_v011_named_saved_sql_reads_history_and_preserves_bytes() -> Result<()> {
    let expected: Expected = serde_json::from_str(EXPECTED)?;
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/historical-spaces/v0.1.1-named-sql");
    let mut listed_paths = BTreeSet::new();
    for line in std::str::from_utf8(&std::fs::read(fixture_root.join("SHA256SUMS"))?)?.lines() {
        let (expected_hash, relative_path) = line
            .split_once("  ")
            .context("fixture checksum line uses sha256sum format")?;
        assert!(
            listed_paths.insert(relative_path.to_owned()),
            "duplicate manifest path"
        );
        assert_eq!(
            sha256(&std::fs::read(fixture_root.join(relative_path))?),
            expected_hash
        );
    }
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
                    path.strip_prefix(&fixture_root)?
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
    let source_space = fixture_root.join("spaces").join(&expected.space_id);
    let prefix = format!("spaces/{}", expected.space_id);
    let op = setup_operator()?;
    // Bootstrap current-reader system directories, then overlay the released
    // Space bytes exactly as stored by the v0.1.1 binary.
    space::create_space_with_identity_and_name(
        &op,
        Uuid::parse_str(&expected.space_id)?,
        &expected.slug,
        &expected.space_name,
        "memory://v011-named-sql-fixture",
    )
    .await?;
    write_directory(&op, &source_space, &prefix).await?;
    let before = snapshot(&op, &prefix).await?;

    let service = UgoiteService::from_operator(op.clone(), "memory://v011-named-sql-fixture");
    let saved = service
        .get_saved_sql(&expected.space_id, &expected.saved_sql_id)
        .await?;
    assert_eq!(saved["id"], expected.saved_sql_id);
    assert_eq!(saved["name"], expected.name);
    assert_eq!(saved["revision_id"], expected.current_revision_id);
    let saved_list = service
        .list_saved_sql_operator_unscoped(&expected.space_id)
        .await?;
    let listed = saved_list
        .iter()
        .find(|entry| entry["id"] == expected.saved_sql_id)
        .context("named Saved SQL appears in list")?;
    assert_eq!(listed["name"], expected.name);
    assert_eq!(saved["sql"], expected.sql);
    assert_eq!(saved["variables"], expected.variables);
    let history = service
        .entry_history(&expected.space_id, &expected.saved_sql_id)
        .await?;
    let revisions = history["revisions"]
        .as_array()
        .context("Saved SQL history revisions")?;
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0]["revision_id"], expected.previous_revision_id);
    assert_eq!(revisions[1]["revision_id"], expected.current_revision_id);

    for (revision_id, expected_answer) in [
        (&expected.previous_revision_id, "released SQL"),
        (&expected.current_revision_id, "released SQL"),
    ] {
        let page = service
            .query_sql(
                &expected.space_id,
                SqlQueryRequest {
                    binding_base: None,
                    form_binding_guards: Vec::new(),
                    sql: String::new(),
                    parameters: Map::from_iter([("label".into(), json!(expected_answer))]),
                    parameter_types: Default::default(),
                    include_exact_integer_values: false,
                    limit: 10,
                    continuation: None,
                    execution_id: None,
                    saved_sql: Some(SavedSqlRevisionRef {
                        id: expected.saved_sql_id.clone(),
                        revision_id: revision_id.clone(),
                    }),
                },
            )
            .await?;
        assert_eq!(page.rows[0]["answer"], expected_answer);
    }

    let after = snapshot(&op, &prefix).await?;
    assert_eq!(
        after, before,
        "reading released Saved SQL and history preserves persisted bytes"
    );
    Ok(())
}
