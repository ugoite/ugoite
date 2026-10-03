use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::tempdir;
use ugoite_domain::composition::parse_composition_yaml;
use ugoite_iceberg::{composition, service::UgoiteService};
use uuid::Uuid;

const MONTHLY_EXPENSE: &str =
    include_str!("../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml");
const OLD_READER_BINARY_ENV: &str = "UGOITE_V021_CLI_PATH";
const UNRELATED_ENTRY_ID: &str = "unrelated-note";
const UNRELATED_FORM_NAME: &str = "CompatibilityNote";

fn run_v021(cli: &Path, config: &Path, args: &[&str]) -> Result<Output> {
    Command::new(cli)
        .arg("--config")
        .arg(config)
        .args(args)
        .output()
        .with_context(|| format!("run v0.2.1 CLI with {args:?}"))
}

fn require_success(output: &Output, action: &str) -> Result<()> {
    ensure!(
        output.status.success(),
        "{action} failed ({}): {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn cli_config(root: &Path, space_id: &str) -> Result<(tempfile::TempDir, PathBuf)> {
    let directory = tempdir()?;
    let root = root
        .to_str()
        .context("temporary Space root is not valid UTF-8")?;
    let config_path = directory.path().join("ugoite.toml");
    let config = format!(
        "version = 1\ncurrent_context = \"downgrade-check\"\n\n[connections.local]\ntype = \"core\"\nroot = {}\n\n[contexts.downgrade-check]\nconnection = \"local\"\nspace_uid = {}\n",
        serde_json::to_string(root)?,
        serde_json::to_string(space_id)?
    );
    std::fs::write(&config_path, config)?;
    Ok((directory, config_path))
}

/// Uses the released v0.2.1 CLI as the old reader/writer in the Composition
/// downgrade compatibility path. Run with the pinned binary through
/// `mise run test:composition:downgrade`.
#[tokio::test]
#[ignore = "requires the pinned v0.2.1 CLI supplied by the downgrade task"]
async fn v021_reader_writer_preserves_composition_and_unrelated_knowledge() -> Result<()> {
    let cli = PathBuf::from(
        std::env::var_os(OLD_READER_BINARY_ENV)
            .with_context(|| format!("{OLD_READER_BINARY_ENV} must be set"))?,
    );
    let version = Command::new(&cli).arg("--version").output()?;
    require_success(&version, "check v0.2.1 CLI version")?;
    ensure!(
        String::from_utf8_lossy(&version.stdout).trim() == "ugoite 0.2.1",
        "unexpected compatibility reader version: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );

    let root = tempdir()?;
    let root_uri = format!("file://{}", root.path().display());
    let owner = Uuid::now_v7();
    let service = UgoiteService::new_without_background_refresh(&root_uri)?;
    let space_id = service
        .create_space_for_principal("composition-downgrade", owner, "Composition downgrade")
        .await?
        .to_string();

    service
        .upsert_form(
            &space_id,
            &json!({
                "name": UNRELATED_FORM_NAME,
                "fields": {
                    "title": {"type": "string", "required": true},
                    "body": {"type": "string", "required": true}
                }
            }),
        )
        .await?;
    let (entry_before, _) = service
        .create_structured_entry_with_receipt(
            &space_id,
            UNRELATED_ENTRY_ID,
            UNRELATED_FORM_NAME.to_owned(),
            Vec::new(),
            BTreeMap::from([
                ("title".to_owned(), json!("Before downgrade")),
                (
                    "body".to_owned(),
                    json!("Unrelated Knowledge remains readable."),
                ),
            ]),
            BTreeMap::new(),
            "current-writer",
        )
        .await?;

    let document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let saved = service
        .save_composition_authorized_for_principals(
            &space_id,
            composition::CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
                tags: Some(vec!["compatibility".to_owned()]),
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;
    let composition_id = saved.entry_id.to_string();
    let composition_revision_id = saved.revision_id.to_string();
    let (config_dir, config_path) = cli_config(root.path(), &space_id)?;
    let fields_path = config_dir.path().join("updated-fields.json");
    std::fs::write(
        &fields_path,
        serde_json::to_vec(&json!({
            "title": "After downgrade",
            "body": "The v0.2.1 writer appended this unrelated update."
        }))?,
    )?;

    let old_read = run_v021(
        &cli,
        &config_path,
        &["entry", "--format", "json", "get", UNRELATED_ENTRY_ID],
    )?;
    require_success(&old_read, "v0.2.1 read of unrelated Knowledge")?;
    let old_entry: Value =
        serde_json::from_slice(&old_read.stdout).context("decode v0.2.1 unrelated Entry read")?;
    ensure!(
        old_entry["fields"]["title"] == "Before downgrade",
        "v0.2.1 did not read the expected unrelated Entry: {old_entry}"
    );

    let old_write = Command::new(&cli)
        .arg("--config")
        .arg(&config_path)
        .args([
            "entry",
            "--format",
            "json",
            "update",
            UNRELATED_ENTRY_ID,
            "--form",
            UNRELATED_FORM_NAME,
            "--fields-file",
        ])
        .arg(&fields_path)
        .args(["--author", "v0.2.1-downgrade-fixture"])
        .output()
        .context("run v0.2.1 update of unrelated Knowledge")?;
    require_success(&old_write, "v0.2.1 update of unrelated Knowledge")?;
    drop(service);

    let reopened = UgoiteService::new_without_background_refresh(&root_uri)?;
    let reopened_space = reopened.get_space(&space_id).await?;
    ensure!(
        reopened_space["space_version"] == "0.1",
        "the downgrade path changed the Space compatibility version"
    );
    let latest_composition = reopened
        .get_composition_raw_authorized_for_principals(&space_id, &composition_id, &[owner])
        .await?;
    ensure!(
        latest_composition.revision.revision_id.to_string() == composition_revision_id,
        "the v0.2.1 write changed the Composition revision"
    );
    ensure!(
        latest_composition.fields.get("spec") == Some(&json!(saved.canonical_yaml)),
        "the v0.2.1 write changed the raw Composition carrier"
    );

    let composition_history = reopened
        .composition_history_authorized_for_principals_page(
            &space_id,
            &composition_id,
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    ensure!(composition_history.total == 1);
    ensure!(composition_history.revisions.len() == 1);
    ensure!(
        composition_history.revisions[0]
            .revision
            .revision_id
            .to_string()
            == composition_revision_id
    );

    let entry_after = reopened.get_entry(&space_id, UNRELATED_ENTRY_ID).await?;
    ensure!(
        entry_after["fields"]["title"] == "After downgrade",
        "v0.2.2 did not read the v0.2.1 update: {entry_after}"
    );
    let unrelated_history = reopened
        .entry_history(&space_id, UNRELATED_ENTRY_ID)
        .await?;
    let revisions = unrelated_history["revisions"]
        .as_array()
        .context("unrelated Entry history is missing its revisions")?;
    ensure!(
        revisions.len() == 2,
        "expected the current and v0.2.1 revisions, got {}",
        revisions.len()
    );
    ensure!(
        revisions[0]["revision_id"] == entry_before["revision_id"],
        "the v0.2.1 update did not append after the original unrelated revision"
    );
    ensure!(
        revisions[1]["revision_id"].is_string() && revisions[1]["operation"] == "upsert",
        "the v0.2.1 write did not create an append-only update revision"
    );

    Ok(())
}
