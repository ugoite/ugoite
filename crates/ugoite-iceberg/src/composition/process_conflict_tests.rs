//! Cross-process stale-base coverage for the typed Composition publication path.
//!
//! Each child reads the same current revision and reaches the Catalog Head
//! publication boundary before either is released. This intentionally calls
//! the crate-private typed storage helper instead of the authorized service
//! wrapper: the local filesystem authorization lease serializes public writes
//! before they can contend at Catalog Head, while the existing service-level
//! tests cover that authorization boundary.

use crate::composition::{self, CompositionSaveRequest};
use crate::service::UgoiteService;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::composition::{canonicalize_composition, parse_composition_yaml};
use ugoite_domain::id::{EntryId, RevisionId};
use uuid::Uuid;

const MONTHLY_EXPENSE: &str =
    include_str!("../../../ugoite-domain/tests/fixtures/composition/monthly-expense.ugcomp.yaml");
const CHILD_MODE_ENV: &str = "UGOITE_COMPOSITION_CONFLICT_CHILD";
const TEST_NAME: &str =
    "composition::authorized_raw_read_tests::process_conflict_tests::composition_stale_base_conflicts_across_processes";
const IDEMPOTENT_TEST_NAME: &str = "composition::authorized_raw_read_tests::process_conflict_tests::composition_identical_save_retries_share_one_publication_across_processes";
const CHILD_WAIT: Duration = Duration::from_secs(45);

#[derive(Debug, Serialize, Deserialize)]
struct WriterOutcome {
    role: String,
    result: String,
    revision_id: Option<String>,
    receipt: Option<crate::CommitReceipt>,
    receipt_command_id: Option<String>,
    committed_revision_ids: Vec<String>,
    error_code: Option<String>,
    error_message: Option<String>,
}

/// Isolates the Catalog Head compare-and-swap from the service's process-local
/// authorization lease. Both children still read the persisted base and use
/// the typed Composition save path; the sibling service tests exercise ACLs.
#[tokio::test]
async fn composition_stale_base_conflicts_across_processes() -> anyhow::Result<()> {
    if std::env::var_os(CHILD_MODE_ENV).is_some() {
        return run_writer_child().await;
    }

    let directory = tempfile::tempdir()?;
    let root_uri = format!("file://{}", directory.path().display());
    let service = UgoiteService::new_without_background_refresh(&root_uri)?;
    let owner = Uuid::from_u128(3_482_501);
    let space_id = service
        .create_space_for_principal("composition-process-conflict", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.tags = vec!["composition-process-conflict".to_string()];
    let initial = service
        .save_composition_authorized_for_principals(
            &space_id,
            CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let entry_id = initial.entry_id.to_string();
    let base_revision_id = initial.revision_id.to_string();
    let winner_name = "Winner from process one";
    let loser_name = "Loser from process two";
    let winner_gate = directory.path().join("winner-gate");
    let loser_gate = directory.path().join("loser-gate");
    fs::create_dir_all(&winner_gate)?;
    fs::create_dir_all(&loser_gate)?;
    let winner_result = directory.path().join("winner-result.json");
    let loser_result = directory.path().join("loser-result.json");

    let mut winner = spawn_writer(
        WriterSpec {
            role: "winner",
            root_uri: &root_uri,
            space_id: &space_id,
            entry_id: &entry_id,
            base_revision_id: &base_revision_id,
            name: winner_name,
            gate: &winner_gate,
            result: &winner_result,
        },
        TEST_NAME,
    )?;
    winner.wait_for_gate(&winner_gate)?;

    let mut loser = spawn_writer(
        WriterSpec {
            role: "loser",
            root_uri: &root_uri,
            space_id: &space_id,
            entry_id: &entry_id,
            base_revision_id: &base_revision_id,
            name: loser_name,
            gate: &loser_gate,
            result: &loser_result,
        },
        TEST_NAME,
    )?;
    loser.wait_for_gate(&loser_gate)?;

    fs::write(winner_gate.join("release-1"), b"release")?;
    let winner_output = winner.wait()?;
    assert_child_success("winner", &winner_output);
    let winner_outcome: WriterOutcome = serde_json::from_slice(&fs::read(&winner_result)?)?;
    assert_eq!(winner_outcome.role, "winner");
    assert_eq!(winner_outcome.result, "saved");
    let winner_revision_id = winner_outcome
        .revision_id
        .as_deref()
        .expect("winner must report its revision ID");
    let winner_command_id = winner_outcome
        .receipt_command_id
        .as_deref()
        .expect("winner must report its receipt command ID");
    assert_eq!(
        winner_outcome.committed_revision_ids,
        [winner_revision_id.to_string()]
    );

    fs::write(loser_gate.join("release-1"), b"release")?;
    let loser_output = loser.wait()?;
    assert_child_success("loser", &loser_output);
    let loser_outcome: WriterOutcome = serde_json::from_slice(&fs::read(&loser_result)?)?;
    assert_eq!(loser_outcome.role, "loser");
    assert_eq!(loser_outcome.result, "conflict");
    assert_eq!(
        loser_outcome.error_code.as_deref(),
        Some(ErrorCode::RevisionConflict.as_str())
    );
    assert!(loser_outcome
        .error_message
        .as_deref()
        .is_some_and(|message| message.contains("Revision conflict")));
    assert!(loser_outcome.committed_revision_ids.is_empty());

    let latest = service
        .get_composition_raw_authorized_for_principals(&space_id, &entry_id, &[owner])
        .await?;
    assert_eq!(latest.revision.revision_id.to_string(), winner_revision_id);
    assert_eq!(latest.revision.change_id, winner_command_id);
    assert_eq!(
        latest.revision.parent_revision_id,
        Some(initial.revision_id)
    );
    assert_eq!(latest.fields["name"], winner_name);

    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &entry_id,
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 2, "only the base and winner are reachable");
    assert_eq!(history.revisions.len(), 2);
    let reachable = history
        .revisions
        .iter()
        .map(|revision| revision.revision.revision_id.to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        reachable,
        BTreeSet::from([base_revision_id, winner_revision_id.to_string()])
    );
    Ok(())
}

/// Proves that a retry can adopt an identical publication after it becomes
/// durable but before its original writer updates Catalog Head. This is the
/// storage idempotency boundary; neither request passes through the service's
/// per-Space authorization lease.
#[tokio::test]
async fn composition_identical_save_retries_share_one_publication_across_processes(
) -> anyhow::Result<()> {
    if std::env::var_os(CHILD_MODE_ENV).is_some() {
        return run_writer_child().await;
    }

    let directory = tempfile::tempdir()?;
    let root_uri = format!("file://{}", directory.path().display());
    let service = UgoiteService::new_without_background_refresh(&root_uri)?;
    let owner = Uuid::from_u128(3_482_502);
    let space_id = service
        .create_space_for_principal("composition-identical-process-save", owner, "Owner")
        .await?
        .to_string();
    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.tags = vec!["composition-identical-process-save".to_string()];
    let initial = service
        .save_composition_authorized_for_principals(
            &space_id,
            CompositionSaveRequest {
                entry_id: None,
                base_revision_id: None,
                document,
            },
            &owner.to_string(),
            &[owner],
        )
        .await?;

    let entry_id = initial.entry_id.to_string();
    let base_revision_id = initial.revision_id.to_string();
    let same_operation_name = "Same idempotent publication";
    let first_gate = directory.path().join("first-gate");
    let retry_gate = directory.path().join("retry-gate");
    fs::create_dir_all(&first_gate)?;
    fs::create_dir_all(&retry_gate)?;
    let first_result = directory.path().join("first-result.json");
    let retry_result = directory.path().join("retry-result.json");

    let mut first = spawn_writer(
        WriterSpec {
            role: "first",
            root_uri: &root_uri,
            space_id: &space_id,
            entry_id: &entry_id,
            base_revision_id: &base_revision_id,
            name: same_operation_name,
            gate: &first_gate,
            result: &first_result,
        },
        IDEMPOTENT_TEST_NAME,
    )?;
    first.wait_for_gate(&first_gate)?;

    let retry = spawn_writer(
        WriterSpec {
            role: "retry",
            root_uri: &root_uri,
            space_id: &space_id,
            entry_id: &entry_id,
            base_revision_id: &base_revision_id,
            name: same_operation_name,
            gate: &retry_gate,
            result: &retry_result,
        },
        IDEMPOTENT_TEST_NAME,
    )?;
    let retry_output = retry.wait()?;
    assert_child_success("retry", &retry_output);
    assert!(
        !retry_gate.join("entered-1").exists(),
        "the retry should adopt the durable publication before attempting a second publication"
    );

    fs::write(first_gate.join("release-1"), b"release")?;
    let first_output = first.wait()?;
    assert_child_success("first", &first_output);

    let first_outcome: WriterOutcome = serde_json::from_slice(&fs::read(&first_result)?)?;
    let retry_outcome: WriterOutcome = serde_json::from_slice(&fs::read(&retry_result)?)?;
    assert_eq!(first_outcome.result, "saved");
    assert_eq!(retry_outcome.result, "saved");
    assert_eq!(first_outcome.receipt, retry_outcome.receipt);
    assert_eq!(
        first_outcome.revision_id, retry_outcome.revision_id,
        "same operation identity should resolve to the same revision"
    );
    assert_eq!(
        first_outcome.committed_revision_ids,
        retry_outcome.committed_revision_ids
    );
    let revision_id = first_outcome
        .revision_id
        .as_deref()
        .expect("successful save must report a revision ID");
    let command_id = first_outcome
        .receipt_command_id
        .as_deref()
        .expect("successful save must report a receipt command ID");
    let receipt = first_outcome
        .receipt
        .as_ref()
        .expect("successful save must report the complete receipt");
    assert_eq!(receipt.command_id, command_id);
    assert_eq!(
        receipt.committed_revision_ids,
        [RevisionId::from(Uuid::parse_str(revision_id)?)]
    );
    assert_eq!(
        first_outcome.committed_revision_ids,
        [revision_id.to_string()]
    );

    let latest = service
        .get_composition_raw_authorized_for_principals(&space_id, &entry_id, &[owner])
        .await?;
    assert_eq!(latest.revision.revision_id.to_string(), revision_id);
    assert_eq!(latest.revision.change_id, command_id);
    assert_eq!(
        latest.revision.parent_revision_id,
        Some(initial.revision_id)
    );
    assert_eq!(latest.fields["name"], same_operation_name);

    let history = service
        .composition_history_authorized_for_principals_page(
            &space_id,
            &entry_id,
            &[owner],
            composition::COMPOSITION_HISTORY_MAX_PAGE_SIZE,
            0,
        )
        .await?;
    assert_eq!(history.total, 2, "only one retry publication is reachable");
    assert_eq!(history.revisions.len(), 2);
    assert_eq!(
        history
            .revisions
            .last()
            .unwrap()
            .revision
            .revision_id
            .to_string(),
        revision_id
    );
    Ok(())
}

async fn run_writer_child() -> anyhow::Result<()> {
    let role = required_env("UGOITE_COMPOSITION_CONFLICT_ROLE")?;
    let root_uri = required_env("UGOITE_COMPOSITION_CONFLICT_ROOT_URI")?;
    let space_id = required_env("UGOITE_COMPOSITION_CONFLICT_SPACE_ID")?;
    let entry_id = EntryId::from(Uuid::parse_str(&required_env(
        "UGOITE_COMPOSITION_CONFLICT_ENTRY_ID",
    )?)?);
    let base_revision_id = RevisionId::from(Uuid::parse_str(&required_env(
        "UGOITE_COMPOSITION_CONFLICT_BASE_REVISION_ID",
    )?)?);
    let result_path = PathBuf::from(required_env("UGOITE_COMPOSITION_CONFLICT_RESULT")?);
    let name = required_env("UGOITE_COMPOSITION_CONFLICT_NAME")?;

    let service = UgoiteService::new_without_background_refresh(&root_uri)?;
    let mut document = parse_composition_yaml(MONTHLY_EXPENSE)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    document.name = name.clone();
    let canonical = canonicalize_composition(&document)
        .map_err(|diagnostic| anyhow::anyhow!(diagnostic.as_str()))?;
    let request = CompositionSaveRequest {
        entry_id: Some(entry_id),
        base_revision_id: Some(base_revision_id),
        document,
    };

    let outcome = match composition::save_composition(
        service.operator(),
        &space_id,
        &service.workspace_path(&space_id),
        request,
        entry_id,
        canonical,
        "cross-process-conflict-test",
        &name,
    )
    .await
    {
        Ok(saved) => WriterOutcome {
            role,
            result: "saved".to_string(),
            revision_id: Some(saved.revision_id.to_string()),
            receipt: Some(saved.receipt.clone()),
            receipt_command_id: Some(saved.receipt.command_id),
            committed_revision_ids: saved
                .receipt
                .committed_revision_ids
                .iter()
                .map(ToString::to_string)
                .collect(),
            error_code: None,
            error_message: None,
        },
        Err(error) => {
            let app_error = error
                .chain()
                .find_map(|cause| cause.downcast_ref::<AppError>());
            let result =
                if app_error.is_some_and(|error| error.code() == ErrorCode::RevisionConflict) {
                    "conflict"
                } else {
                    "error"
                };
            WriterOutcome {
                role,
                result: result.to_string(),
                revision_id: None,
                receipt: None,
                receipt_command_id: None,
                committed_revision_ids: Vec::new(),
                error_code: app_error.map(|error| error.code().as_str().to_string()),
                error_message: Some(format!("{error:#}")),
            }
        }
    };
    fs::write(result_path, serde_json::to_vec(&outcome)?)?;
    Ok(())
}

struct WriterSpec<'a> {
    role: &'a str,
    root_uri: &'a str,
    space_id: &'a str,
    entry_id: &'a str,
    base_revision_id: &'a str,
    name: &'a str,
    gate: &'a Path,
    result: &'a Path,
}

fn spawn_writer(spec: WriterSpec<'_>, test_name: &str) -> anyhow::Result<ChildProcess> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(CHILD_MODE_ENV, "1")
        .env("UGOITE_TEST_PUBLICATION_GATE_DIR", spec.gate)
        .env("UGOITE_COMPOSITION_CONFLICT_ROLE", spec.role)
        .env("UGOITE_COMPOSITION_CONFLICT_ROOT_URI", spec.root_uri)
        .env("UGOITE_COMPOSITION_CONFLICT_SPACE_ID", spec.space_id)
        .env("UGOITE_COMPOSITION_CONFLICT_ENTRY_ID", spec.entry_id)
        .env(
            "UGOITE_COMPOSITION_CONFLICT_BASE_REVISION_ID",
            spec.base_revision_id,
        )
        .env("UGOITE_COMPOSITION_CONFLICT_NAME", spec.name)
        .env("UGOITE_COMPOSITION_CONFLICT_RESULT", spec.result)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    Ok(ChildProcess {
        child: Some(command.spawn()?),
        role: spec.role.to_string(),
    })
}

struct ChildProcess {
    child: Option<Child>,
    role: String,
}

impl ChildProcess {
    fn wait_for_gate(&mut self, gate: &Path) -> anyhow::Result<()> {
        let entered = gate.join("entered-1");
        let deadline = Instant::now() + CHILD_WAIT;
        loop {
            if entered.exists() {
                return Ok(());
            }
            if let Some(status) = self.child_mut().try_wait()? {
                let output = self
                    .child
                    .take()
                    .expect("child is present until it exits")
                    .wait_with_output()?;
                anyhow::bail!(
                    "{} process exited before publication gate (status {status}): {}{}",
                    self.role,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            if Instant::now() >= deadline {
                anyhow::bail!("timed out waiting for {} publication gate", self.role);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait(mut self) -> anyhow::Result<Output> {
        let deadline = Instant::now() + CHILD_WAIT;
        loop {
            if self.child_mut().try_wait()?.is_some() {
                return Ok(self
                    .child
                    .take()
                    .expect("child is present until it exits")
                    .wait_with_output()?);
            }
            if Instant::now() >= deadline {
                anyhow::bail!("timed out waiting for {} process", self.role);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn child_mut(&mut self) -> &mut Child {
        self.child.as_mut().expect("child has not been collected")
    }
}

impl Drop for ChildProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn assert_child_success(role: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{role} child failed (status {}): {}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn required_env(name: &str) -> anyhow::Result<String> {
    std::env::var(name).map_err(|error| anyhow::anyhow!("missing {name}: {error}"))
}
