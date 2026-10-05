use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::process::{Command, Stdio};
use ugoite_iceberg::{authorization::Authorizer, service::UgoiteService};
use uuid::Uuid;

/// Optional end-to-end proof through the same application service used by the
/// server. Set UGOITE_S3_TEST_REQUIRED=1 to make missing configuration fail.
///
/// Isolated-prefix contract (issue #3266): the proof writes a uniquely
/// prefixed Space, Entry, Form, Asset, and Change history under
/// `ugoite/l12/recovery/<uuid>` and best-effort removes exactly that prefix
/// after success and failure alike, without masking the proof result.
/// Point the suite at a dedicated test bucket: cleanup deletes only this
/// run's prefix, so interrupted runs may still leave orphaned unique
/// prefixes behind, and nothing outside the prefix is ever touched.
#[tokio::test]
async fn s3_backed_space_survives_service_reopen_and_revert() -> Result<()> {
    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let prefix = format!("ugoite/l12/recovery/{}", Uuid::now_v7());
    let root_uri = format!("s3://{bucket}/{prefix}");
    let space_slug = format!("recovery-{}", Uuid::now_v7().simple());

    let outcome = run_recovery_proof(&root_uri, &endpoint, &space_slug).await;
    if let Err(error) = cleanup_recovery_prefix(&root_uri, &endpoint).await {
        eprintln!("warning: S3 recovery fixture cleanup failed for {prefix}: {error:#}");
    }
    outcome
}

/// Recovery proof body shared by the outer test: build the fixture Space,
/// mutate it, reopen through a fresh service, and verify the same Space and
/// append-only Change history recover from the remote store.
async fn run_recovery_proof(root_uri: &str, endpoint: &str, space_slug: &str) -> Result<()> {
    let owner = Uuid::now_v7();

    let service = open_verified_service(root_uri, endpoint, space_slug).await?;
    let space_id = service
        .create_space_for_principal(&space_slug, owner, "S3 recovery test")
        .await?
        .to_string();
    service
        .upsert_form(
            &space_id,
            &json!({
                "name": "RecoveryNote",
                "fields": {"Body": {"type": "markdown", "required": true}}
            }),
        )
        .await?;

    let mut fields = BTreeMap::new();
    fields.insert("Body".to_string(), json!("before restart"));
    service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "recovery-entry",
            "RecoveryNote".to_string(),
            Vec::new(),
            fields,
            BTreeMap::new(),
            "S3 recovery test",
            &[owner],
        )
        .await?;

    let asset = service
        .save_asset(&space_id, "recovery.txt", b"persisted asset")
        .await?;
    let mut updated_fields = BTreeMap::new();
    updated_fields.insert("Body".to_string(), json!("after update"));
    let updated = service
        .update_structured_entry_authorized_for_principals(
            &space_id,
            "recovery-entry",
            None,
            None,
            updated_fields,
            BTreeMap::new(),
            None,
            "S3 recovery test",
            &[owner],
        )
        .await?;
    let update_change_id = updated
        .get("change_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("updated Entry omitted its Change ID"))?
        .to_string();
    let reverted = service
        .revert_change(
            &space_id,
            &update_change_id,
            &owner.to_string(),
            None,
            Some("Recover the pre-update value"),
        )
        .await?;
    let revert_change_id = reverted
        .get("change_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("revert omitted its Change ID"))?
        .to_string();
    drop(service);

    // A newly constructed service/operator must recover the same Space and its
    // append-only Change history from the remote store.
    let reopened = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
    let recovery = reopened.open_space(&space_id).await?;
    assert_eq!(recovery["space_id"], space_id);
    // Discovery enumerates Space directory IDs (immutable UUIDs), not slugs.
    assert!(reopened.list_space_ids().await?.contains(&space_id));
    assert_eq!(
        reopened.get_form(&space_id, "RecoveryNote").await?["name"],
        "RecoveryNote"
    );
    assert_eq!(
        reopened.get_entry(&space_id, "recovery-entry").await?["fields"]["Body"],
        "before restart"
    );
    assert_eq!(
        reopened.read_asset(&space_id, &asset.asset_id).await?.bytes,
        b"persisted asset"
    );

    let changes = reopened.list_changes(&space_id).await?;
    let changes = changes
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Change history was not an array"))?;
    assert!(changes.iter().any(|change| {
        change.get("change_id").and_then(Value::as_str) == Some(update_change_id.as_str())
    }));
    assert!(changes.iter().any(|change| {
        change.get("change_id").and_then(Value::as_str) == Some(revert_change_id.as_str())
            && change
                .pointer("/change/reverts_change_id")
                .and_then(Value::as_str)
                == Some(update_change_id.as_str())
    }));
    Ok(())
}

/// Best-effort removal of one recovery fixture prefix (issue #3266). The
/// operator is rooted at the test's own `root_uri`, so only objects under
/// this run's unique prefix can be listed and deleted. Failures warn via
/// the caller and never mask the proof result.
async fn cleanup_recovery_prefix(root_uri: &str, endpoint: &str) -> Result<()> {
    let operator = ugoite_storage::operator_from_uri_with_endpoint(root_uri, Some(endpoint))?;
    remove_prefix_tree(&operator, "").await
}

/// Recursively delete every object under `dir` using only the confirmed
/// list/delete operator surface. S3 prefixes are implicit, so removing all
/// enclosed objects removes the fixture; empty prefixes need no extra step.
async fn remove_prefix_tree(operator: &opendal::Operator, dir: &str) -> Result<()> {
    for entry in operator.list(dir).await? {
        let path = entry.path().to_owned();
        // `list` returns the queried directory itself alongside its
        // children; descending into it would recurse forever.
        if path.trim_matches('/') == dir.trim_matches('/') {
            continue;
        }
        if entry.metadata().is_dir() {
            Box::pin(remove_prefix_tree(operator, &path)).await?;
        } else {
            operator.delete(&path).await?;
        }
    }
    Ok(())
}

#[tokio::test]
async fn recovery_prefix_cleanup_removes_nested_fixture_objects() -> Result<()> {
    let root = tempfile::tempdir()?;
    let operator = opendal::Operator::new(
        opendal::services::Fs::default().root(root.path().to_string_lossy().as_ref()),
    )?;
    operator
        .write("spaces/space-1/meta.json", b"{}".to_vec())
        .await?;
    operator
        .write("spaces/space-1/changes/0001.json", b"{}".to_vec())
        .await?;
    operator
        .write("_ugoite/assets/prepared/asset-1", b"bytes".to_vec())
        .await?;
    remove_prefix_tree(&operator, "").await?;
    // `list("")` always reports the root itself, so assert the fixture
    // objects themselves are gone instead of asserting an empty listing.
    for path in [
        "spaces/space-1/meta.json",
        "spaces/space-1/changes/0001.json",
        "_ugoite/assets/prepared/asset-1",
    ] {
        assert!(
            !operator.exists(path).await?,
            "fixture object {path} was not removed"
        );
    }
    Ok(())
}

/// Two independent application processes share one S3 prefix. The writer
/// process stops after preparing its immutable publication and immediately
/// before the Catalog Head CAS; the revoker then wins the exact Head CAS.
#[tokio::test]
async fn s3_authorization_revocation_wins_over_stale_process_content_publication() -> Result<()> {
    if let Ok(role) = env::var("UGOITE_S3_CROSS_PROCESS_ROLE") {
        let (endpoint, bucket) = s3_test_config()?.expect("S3 test configuration is required");
        let root_uri = env::var("UGOITE_S3_CROSS_ROOT_URI")?;
        let space_slug = env::var("UGOITE_S3_CROSS_SPACE_SLUG")?;
        let space_id = env::var("UGOITE_S3_CROSS_SPACE_ID")?;
        let owner = Uuid::parse_str(&env::var("UGOITE_S3_CROSS_OWNER")?)?;
        let editor = Uuid::parse_str(&env::var("UGOITE_S3_CROSS_EDITOR")?)?;
        let winner_editor = Uuid::parse_str(&env::var("UGOITE_S3_CROSS_WINNER_EDITOR")?)?;
        let entry_id = env::var("UGOITE_S3_CROSS_ENTRY_ID")?;
        let revision_id = env::var("UGOITE_S3_CROSS_REVISION_ID")?;
        let _bucket = bucket;
        let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
        match role.as_str() {
            "writer" => {
                let updated = service
                    .update_structured_entry_authorized_for_principals(
                        &space_id,
                        &entry_id,
                        None,
                        None,
                        [("Body".to_string(), json!("stale writer must not publish"))]
                            .into_iter()
                            .collect(),
                        BTreeMap::new(),
                        Some(&revision_id),
                        "S3 multiprocess writer",
                        &[editor],
                    )
                    .await;
                let error = updated.expect_err("stale writer must lose the final Catalog Head CAS");
                // Either the Head CAS itself rejects the stale base, or the
                // fenced recheck at the publication boundary observes the
                // revocation first. Both prove the stale write did not win.
                let message = format!("{error:#}");
                anyhow::ensure!(
                    message.contains("Catalog Head changed")
                        || message.contains("Space authorization changed"),
                    "writer failed outside the expected stale-write conflict: {message}"
                );
            }
            "revoker" => {
                Authorizer::new(service.operator().clone())
                    .revoke_principal(&space_id, owner, editor)
                    .await?;
            }
            "winner" => {
                let revision_id = env::var("UGOITE_S3_CROSS_WINNER_REVISION_ID")?;
                let update = service
                    .update_structured_entry_authorized_for_principals(
                        &space_id,
                        "winner-entry",
                        None,
                        None,
                        [("Body".to_string(), json!("content Head won first"))]
                            .into_iter()
                            .collect(),
                        BTreeMap::new(),
                        Some(&revision_id),
                        "S3 writer wins first",
                        &[winner_editor],
                    )
                    .await?;
                tokio::fs::write(
                    std::path::Path::new(&env::var("UGOITE_TEST_PUBLICATION_GATE_DIR")?)
                        .join("winner-result.json"),
                    serde_json::to_vec(&update)?,
                )
                .await?;
            }
            "winner-revoker" => {
                Authorizer::new(service.operator().clone())
                    .revoke_principal(&space_id, owner, winner_editor)
                    .await?;
            }
            unknown => bail!("unknown S3 multiprocess role {unknown}"),
        }
        return Ok(());
    }

    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let prefix = format!("ugoite/l12/authorization-race/{}", Uuid::now_v7());
    let root_uri = format!("s3://{bucket}/{prefix}");
    let owner = Uuid::now_v7();
    let editor = Uuid::now_v7();
    let winner_editor = Uuid::now_v7();
    let space_slug = format!("auth-race-{}", Uuid::now_v7().simple());
    let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
    let space_id = service
        .create_space_for_principal(&space_slug, owner, "S3 authorization race")
        .await?
        .to_string();
    // Scope the setup lease narrowly: the returned lease owns the
    // process-global authorization write lock, and every service mutation
    // below acquires its own lease. Holding the setup lease across those
    // calls re-enters the non-reentrant lock and deadlocks the test
    // process (hanging every test in the binary, since the lock and the
    // Space-creation serializer are process-global). The extracted fence
    // carries the same authorization revision for the fenced write, and
    // cross-process ordering stays with the Catalog Head CAS, so dropping
    // the lease changes no proven property.
    let form_fence = {
        let (_, form_lease) = Authorizer::new(service.operator().clone())
            .acquire_state_lease(&space_id)
            .await?;
        form_lease.prepare_mutation().await?;
        form_lease.write_fence()
    };
    ugoite_iceberg::authorization::with_authorization_write_fence(
        form_fence,
        service.upsert_form(
            &space_id,
            &json!({
                "name": "RaceNote",
                "fields": {"Body": {"type": "markdown", "required": true}}
            }),
        ),
    )
    .await?;
    Authorizer::new(service.operator().clone())
        .add_human_member(
            &space_id,
            owner,
            ugoite_domain::identity::SpacePrincipal {
                principal_id: editor,
                kind: ugoite_domain::identity::PrincipalKind::Human,
                display_name: "Race editor".to_string(),
                state: ugoite_domain::identity::PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            ugoite_domain::identity::SpaceRole::Editor,
        )
        .await?;
    Authorizer::new(service.operator().clone())
        .add_human_member(
            &space_id,
            owner,
            ugoite_domain::identity::SpacePrincipal {
                principal_id: winner_editor,
                kind: ugoite_domain::identity::PrincipalKind::Human,
                display_name: "Head winner".to_string(),
                state: ugoite_domain::identity::PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            ugoite_domain::identity::SpaceRole::Editor,
        )
        .await?;
    let winner_created = service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "winner-entry",
            "RaceNote".to_string(),
            Vec::new(),
            [("Body".to_string(), json!("before winner"))]
                .into_iter()
                .collect(),
            BTreeMap::new(),
            "S3 owner",
            &[owner],
        )
        .await?;
    let winner_revision_id = winner_created["revision_id"].as_str().unwrap().to_string();
    let created = service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "race-entry",
            "RaceNote".to_string(),
            Vec::new(),
            [("Body".to_string(), json!("published before revoke"))]
                .into_iter()
                .collect(),
            BTreeMap::new(),
            "S3 multiprocess owner",
            &[owner],
        )
        .await?;
    let revision_id = created
        .get("revision_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("created Entry omitted its revision ID"))?;

    let gate = env::temp_dir().join(format!("ugoite-s3-publication-gate-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&gate).await?;
    let test_binary = std::env::current_exe()?;
    let common = |role: &str| {
        let mut command = Command::new(&test_binary);
        command
            .arg("--exact")
            .arg("s3_authorization_revocation_wins_over_stale_process_content_publication")
            .arg("--nocapture")
            .env("UGOITE_S3_CROSS_PROCESS_ROLE", role)
            .env("UGOITE_S3_CROSS_ROOT_URI", &root_uri)
            .env("UGOITE_S3_CROSS_SPACE_SLUG", &space_slug)
            .env("UGOITE_S3_CROSS_SPACE_ID", &space_id)
            .env("UGOITE_S3_CROSS_OWNER", owner.to_string())
            .env("UGOITE_S3_CROSS_EDITOR", editor.to_string())
            .env("UGOITE_S3_CROSS_WINNER_EDITOR", winner_editor.to_string())
            .env("UGOITE_S3_CROSS_ENTRY_ID", "race-entry")
            .env("UGOITE_S3_CROSS_REVISION_ID", revision_id);
        command
    };
    let mut winner = common("winner")
        .env("UGOITE_S3_CROSS_WINNER_REVISION_ID", winner_revision_id)
        .env("UGOITE_TEST_PUBLICATION_GATE_DIR", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_for_gate(&gate.join("entered-1"), &mut winner).await?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    let winner = join_child(winner, "winner").await?;
    anyhow::ensure!(
        winner.status.success(),
        "{}",
        child_failure_detail("content-first writer", &winner)
    );
    let winner_receipt: Value =
        serde_json::from_slice(&tokio::fs::read(gate.join("winner-result.json")).await?)?;
    let winner_change_id = winner_receipt["change_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("winner omitted its Change receipt"))?;
    let winning_revoker = run_child(&mut common("winner-revoker"), "winner-revoker").await?;
    anyhow::ensure!(
        winning_revoker.status.success(),
        "{}",
        child_failure_detail("content-first revoker", &winning_revoker)
    );
    let winner_changes = service.list_changes(&space_id).await?;
    anyhow::ensure!(
        winner_changes
            .as_array()
            .unwrap()
            .iter()
            .any(|change| change["change_id"] == winner_change_id),
        "the winning content receipt is missing from Change history"
    );
    assert_eq!(
        service.get_entry(&space_id, "winner-entry").await?["fields"]["Body"],
        "content Head won first"
    );
    assert_principal_revoked(&service, &space_id, winner_editor).await?;
    tokio::fs::remove_dir_all(&gate).await?;
    tokio::fs::create_dir_all(&gate).await?;
    let mut writer = common("writer")
        .env("UGOITE_TEST_PUBLICATION_GATE_DIR", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let entered = gate.join("entered-1");
    wait_for_gate(&entered, &mut writer).await?;

    let revoker = run_child(&mut common("revoker"), "revoker").await?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    let writer = join_child(writer, "writer").await?;
    anyhow::ensure!(
        revoker.status.success(),
        "{}",
        child_failure_detail("revoker process", &revoker)
    );
    anyhow::ensure!(
        writer.status.success(),
        "{}",
        child_failure_detail("writer process", &writer)
    );

    assert_eq!(
        service.get_entry(&space_id, "race-entry").await?["fields"]["Body"],
        "published before revoke"
    );
    assert_principal_revoked(&service, &space_id, editor).await?;
    tokio::fs::remove_dir_all(gate).await?;
    Ok(())
}

/// Asset bytes are prepared outside the public namespace and become readable
/// only after their upload receipt wins the same Head CAS as ACL changes.
#[tokio::test]
async fn s3_asset_upload_is_not_exposed_after_revocation_wins() -> Result<()> {
    if let Ok(role) = env::var("UGOITE_S3_ASSET_ROLE") {
        let (endpoint, _) = s3_test_config()?.expect("S3 test configuration is required");
        let root_uri = env::var("UGOITE_S3_ASSET_ROOT_URI")?;
        let space_slug = env::var("UGOITE_S3_ASSET_SPACE_SLUG")?;
        let space_id = env::var("UGOITE_S3_ASSET_SPACE_ID")?;
        let owner = Uuid::parse_str(&env::var("UGOITE_S3_ASSET_OWNER")?)?;
        let editor = Uuid::parse_str(&env::var("UGOITE_S3_ASSET_EDITOR")?)?;
        let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
        match role.as_str() {
            "writer" => {
                let (state, lease) = Authorizer::new(service.operator().clone())
                    .acquire_state_lease(&space_id)
                    .await?;
                anyhow::ensure!(
                    ugoite_iceberg::authorization::effective_actions_for_state(
                        &state, editor, None,
                    )?
                    .contains(&ugoite_domain::identity::Action::Create),
                    "the editor must pass the Server Asset-create ACL before publication"
                );
                lease.prepare_mutation().await?;
                let result = ugoite_iceberg::authorization::with_authorization_write_fence(
                    lease.write_fence(),
                    service.save_asset(&space_id, "revoked.txt", b"must stay private"),
                )
                .await;
                let error = result.expect_err("stale Asset writer must lose the Catalog Head CAS");
                // Either the Head CAS itself rejects the stale base, or the
                // fenced recheck at the publication boundary observes the
                // revocation first. Both prove the stale upload did not win.
                let message = format!("{error:#}");
                anyhow::ensure!(
                    message.contains("Catalog Head changed")
                        || message.contains("Space authorization changed"),
                    "Asset writer failed outside the expected stale-write conflict: {message}"
                );
            }
            "revoker" => {
                Authorizer::new(service.operator().clone())
                    .revoke_principal(&space_id, owner, editor)
                    .await?;
            }
            unknown => bail!("unknown S3 Asset role {unknown}"),
        }
        return Ok(());
    }

    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let prefix = format!("ugoite/l12/asset-authorization-race/{}", Uuid::now_v7());
    let root_uri = format!("s3://{bucket}/{prefix}");
    let owner = Uuid::now_v7();
    let editor = Uuid::now_v7();
    let space_slug = format!("asset-race-{}", Uuid::now_v7().simple());
    let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
    let space_id = service
        .create_space_for_principal(&space_slug, owner, "S3 Asset authorization race")
        .await?
        .to_string();
    Authorizer::new(service.operator().clone())
        .add_human_member(
            &space_id,
            owner,
            ugoite_domain::identity::SpacePrincipal {
                principal_id: editor,
                kind: ugoite_domain::identity::PrincipalKind::Human,
                display_name: "Asset editor".to_string(),
                state: ugoite_domain::identity::PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            ugoite_domain::identity::SpaceRole::Editor,
        )
        .await?;

    let gate = env::temp_dir().join(format!("ugoite-s3-asset-gate-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&gate).await?;
    let test_binary = std::env::current_exe()?;
    let common = |role: &str| {
        let mut command = Command::new(&test_binary);
        command
            .arg("--exact")
            .arg("s3_asset_upload_is_not_exposed_after_revocation_wins")
            .arg("--nocapture")
            .env("UGOITE_S3_ASSET_ROLE", role)
            .env("UGOITE_S3_ASSET_ROOT_URI", &root_uri)
            .env("UGOITE_S3_ASSET_SPACE_SLUG", &space_slug)
            .env("UGOITE_S3_ASSET_SPACE_ID", &space_id)
            .env("UGOITE_S3_ASSET_OWNER", owner.to_string())
            .env("UGOITE_S3_ASSET_EDITOR", editor.to_string());
        command
    };
    let mut writer = common("writer")
        .env("UGOITE_TEST_PUBLICATION_GATE_DIR", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_for_gate(&gate.join("entered-1"), &mut writer).await?;
    let revoker = run_child(&mut common("revoker"), "revoker").await?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    let writer = join_child(writer, "writer").await?;
    anyhow::ensure!(
        revoker.status.success(),
        "{}",
        child_failure_detail("Asset revoker", &revoker)
    );
    anyhow::ensure!(
        writer.status.success(),
        "{}",
        child_failure_detail("Asset writer", &writer)
    );

    let staged = service
        .operator()
        .list(&format!(
            "{}/_ugoite/assets/prepared/",
            service.workspace_path(&space_id)
        ))
        .await?;
    anyhow::ensure!(
        !staged.is_empty(),
        "the losing upload should leave only an unreachable prepared object"
    );
    let asset_id = staged[0]
        .path()
        .rsplit('/')
        .next()
        .ok_or_else(|| anyhow::anyhow!("prepared Asset path omitted its ID"))?;
    anyhow::ensure!(
        service.read_asset(&space_id, asset_id).await.is_err(),
        "an Asset whose Head receipt lost must not be readable"
    );
    assert_principal_revoked(&service, &space_id, editor).await?;
    tokio::fs::remove_dir_all(gate).await?;
    Ok(())
}

/// Asserts a revocation took effect under the implemented authorization
/// contract: revocation marks the principal `Revoked` (the retained
/// membership entry preserves history and keeps human-principal validation
/// intact) and strips every effective action. Asserting membership removal
/// would contradict `validate_authorization_state` and the unit-tested
/// revoke contract, so these acceptance tests assert the same observable
/// Post-revoke state instead.
async fn assert_principal_revoked(
    service: &UgoiteService,
    space_id: &str,
    principal: Uuid,
) -> Result<()> {
    let authorization = Authorizer::new(service.operator().clone())
        .state(space_id)
        .await?;
    anyhow::ensure!(
        matches!(
            authorization
                .principals
                .get(&principal)
                .map(|member| &member.state),
            Some(ugoite_domain::identity::PrincipalState::Revoked)
        ),
        "principal {principal} was not revoked"
    );
    anyhow::ensure!(
        ugoite_iceberg::authorization::effective_actions_for_state(&authorization, principal, None)
            .is_err(),
        "revoked principal {principal} retains effective actions"
    );
    Ok(())
}

/// Finite join budget for every spawned test child. Gate files bound the
/// sequencing waits, but a child stuck outside its gated section must fail
/// the test loudly instead of hanging the lane until the job timeout. The
/// budget covers two gated publications plus setup; children carry their own
/// 60s gate caps and exit on their own after a timeout trips here.
const CHILD_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

async fn join_child(child: std::process::Child, label: &str) -> Result<std::process::Output> {
    match tokio::time::timeout(
        CHILD_JOIN_TIMEOUT,
        tokio::task::spawn_blocking(move || child.wait_with_output()),
    )
    .await
    {
        Ok(join) => Ok(join??),
        Err(_) => bail!("{label} test child did not exit within 300s"),
    }
}

async fn run_child(command: &mut Command, label: &str) -> Result<std::process::Output> {
    // A synchronous `output()` would block the test runtime with no bound;
    // route one-shot children through the same finite join budget instead.
    // Piping (rather than inheriting) keeps the child's output available for
    // the failure diagnostics below. libtest prints failures to stdout, so
    // both streams are needed: stderr alone is empty on failure.
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    join_child(child, label).await
}

/// Renders a failed child's status plus both output streams. The harness
/// prints failures to stdout, so stderr alone would leave lane failures
/// undiagnosable.
fn child_failure_detail(label: &str, output: &std::process::Output) -> String {
    format!(
        "{label} failed with status {}: stdout: {}; stderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

async fn wait_for_gate(path: &std::path::Path, child: &mut std::process::Child) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    while !path.exists() {
        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill();
            bail!("writer process did not reach the pre-Head-CAS gate");
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    Ok(())
}
#[tokio::test]
async fn s3_change_revert_rechecks_acl_at_publication_head() -> Result<()> {
    if let Ok(role) = env::var("UGOITE_S3_REVERT_ROLE") {
        let (endpoint, _) = s3_test_config()?.expect("S3 test configuration is required");
        let root_uri = env::var("UGOITE_S3_REVERT_ROOT_URI")?;
        let space_slug = env::var("UGOITE_S3_REVERT_SPACE_SLUG")?;
        let space_id = env::var("UGOITE_S3_REVERT_SPACE_ID")?;
        let owner = Uuid::parse_str(&env::var("UGOITE_S3_REVERT_OWNER")?)?;
        let editor = Uuid::parse_str(&env::var("UGOITE_S3_REVERT_EDITOR")?)?;
        let target_change_id = env::var("UGOITE_S3_REVERT_CHANGE_ID")?;
        let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
        match role.as_str() {
            "writer" => {
                let result = service
                    .revert_change_authorized_for_principals(
                        &space_id,
                        &target_change_id,
                        &editor.to_string(),
                        None,
                        Some("S3 revoke race"),
                        &[editor],
                    )
                    .await;
                let error = result.expect_err("stale revert must lose the Catalog Head CAS");
                // Either the Head CAS itself rejects the stale base, or the
                // fenced recheck at the publication boundary observes the
                // revocation first. Both prove the stale revert did not win.
                let message = format!("{error:#}");
                anyhow::ensure!(
                    message.contains("Catalog Head changed")
                        || message.contains("Space authorization changed"),
                    "revert failed outside the expected stale-write conflict: {message}"
                );
            }
            "revoker" => {
                Authorizer::new(service.operator().clone())
                    .revoke_principal(&space_id, owner, editor)
                    .await?;
            }
            unknown => bail!("unknown S3 Change revert role {unknown}"),
        }
        return Ok(());
    }

    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let root_uri = format!(
        "s3://{bucket}/ugoite/l12/change-revert-race/{}",
        Uuid::now_v7()
    );
    let owner = Uuid::now_v7();
    let editor = Uuid::now_v7();
    let space_slug = format!("revert-race-{}", Uuid::now_v7().simple());
    let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
    let space_id = service
        .create_space_for_principal(&space_slug, owner, "S3 revert race")
        .await?
        .to_string();
    // Narrow lease scope (see the authorization-race test): the lease owns
    // the process-global authorization write lock, which later setup calls
    // re-acquire. Dropping it after extracting the fence keeps the fenced
    // revision without re-entering the lock.
    let revert_fence = {
        let (_, lease) = Authorizer::new(service.operator().clone())
            .acquire_state_lease(&space_id)
            .await?;
        lease.prepare_mutation().await?;
        lease.write_fence()
    };
    ugoite_iceberg::authorization::with_authorization_write_fence(
        revert_fence,
        service.upsert_form(
            &space_id,
            &json!({"name":"RevertRaceNote","fields":{"Body":{"type":"markdown","required":true}}}),
        ),
    )
    .await?;
    let mut initial = BTreeMap::new();
    initial.insert("Body".to_string(), json!("before"));
    service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "revert-race-entry",
            "RevertRaceNote".to_string(),
            Vec::new(),
            initial,
            BTreeMap::new(),
            "S3 owner",
            &[owner],
        )
        .await?;
    Authorizer::new(service.operator().clone())
        .add_human_member(
            &space_id,
            owner,
            ugoite_domain::identity::SpacePrincipal {
                principal_id: editor,
                kind: ugoite_domain::identity::PrincipalKind::Human,
                display_name: "Revert editor".to_string(),
                state: ugoite_domain::identity::PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            ugoite_domain::identity::SpaceRole::Editor,
        )
        .await?;
    let updated = service
        .update_structured_entry_authorized_for_principals(
            &space_id,
            "revert-race-entry",
            None,
            None,
            [("Body".to_string(), json!("after"))].into_iter().collect(),
            BTreeMap::new(),
            None,
            "S3 editor",
            &[editor],
        )
        .await?;
    let target_change_id = updated["change_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("update omitted Change ID"))?
        .to_string();
    let gate = env::temp_dir().join(format!("ugoite-s3-revert-gate-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&gate).await?;
    let test_binary = std::env::current_exe()?;
    let common = |role: &str| {
        let mut command = Command::new(&test_binary);
        command
            .arg("--exact")
            .arg("s3_change_revert_rechecks_acl_at_publication_head")
            .arg("--nocapture")
            .env("UGOITE_S3_REVERT_ROLE", role)
            .env("UGOITE_S3_REVERT_ROOT_URI", &root_uri)
            .env("UGOITE_S3_REVERT_SPACE_SLUG", &space_slug)
            .env("UGOITE_S3_REVERT_SPACE_ID", &space_id)
            .env("UGOITE_S3_REVERT_OWNER", owner.to_string())
            .env("UGOITE_S3_REVERT_EDITOR", editor.to_string())
            .env("UGOITE_S3_REVERT_CHANGE_ID", &target_change_id);
        command
    };
    let mut writer = common("writer")
        .env("UGOITE_TEST_PUBLICATION_GATE_DIR", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_for_gate(&gate.join("entered-1"), &mut writer).await?;
    let revoker = run_child(&mut common("revoker"), "revoker").await?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    let writer = join_child(writer, "writer").await?;
    anyhow::ensure!(
        revoker.status.success(),
        "{}",
        child_failure_detail("revert revoker", &revoker)
    );
    anyhow::ensure!(
        writer.status.success(),
        "{}",
        child_failure_detail("revert writer", &writer)
    );
    assert_eq!(
        service.get_entry(&space_id, "revert-race-entry").await?["fields"]["Body"],
        "after"
    );
    let changes = service.list_changes(&space_id).await?;
    assert!(!changes.as_array().unwrap().iter().any(|change| change
        .pointer("/change/reverts_change_id")
        .and_then(Value::as_str)
        == Some(target_change_id.as_str())));
    assert_principal_revoked(&service, &space_id, editor).await?;
    tokio::fs::remove_dir_all(gate).await?;
    Ok(())
}

#[tokio::test]
async fn s3_run_undo_stops_after_revoke_between_inverse_changes() -> Result<()> {
    if let Ok(role) = env::var("UGOITE_S3_UNDO_ROLE") {
        let (endpoint, _) = s3_test_config()?.expect("S3 test configuration is required");
        let root_uri = env::var("UGOITE_S3_UNDO_ROOT_URI")?;
        let space_slug = env::var("UGOITE_S3_UNDO_SPACE_SLUG")?;
        let space_id = env::var("UGOITE_S3_UNDO_SPACE_ID")?;
        let owner = Uuid::parse_str(&env::var("UGOITE_S3_UNDO_OWNER")?)?;
        let editor = Uuid::parse_str(&env::var("UGOITE_S3_UNDO_EDITOR")?)?;
        let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
        match role.as_str() {
            "writer" => {
                let result = service
                    .undo_run_authorized_for_principals(
                        &space_id,
                        "shared-run",
                        &editor.to_string(),
                        &[editor],
                    )
                    .await?;
                tokio::fs::write(
                    std::path::Path::new(&env::var("UGOITE_TEST_PUBLICATION_GATE_DIR")?)
                        .join("undo-result.json"),
                    serde_json::to_vec(&result)?,
                )
                .await?;
                anyhow::ensure!(
                    result["state"] == "partially_saved",
                    "Run undo did not report its partial result: {result}"
                );
            }
            "revoker" => {
                Authorizer::new(service.operator().clone())
                    .revoke_principal(&space_id, owner, editor)
                    .await?;
            }
            unknown => bail!("unknown S3 Run undo role {unknown}"),
        }
        return Ok(());
    }

    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let root_uri = format!("s3://{bucket}/ugoite/l12/run-undo-race/{}", Uuid::now_v7());
    let owner = Uuid::now_v7();
    let editor = Uuid::now_v7();
    let space_slug = format!("undo-race-{}", Uuid::now_v7().simple());
    let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
    let space_id = service
        .create_space_for_principal(&space_slug, owner, "S3 Run undo race")
        .await?
        .to_string();
    // Narrow lease scope (see the authorization-race test): the lease owns
    // the process-global authorization write lock, which later setup calls
    // re-acquire. Dropping it after extracting the fence keeps the fenced
    // revision without re-entering the lock.
    let undo_fence = {
        let (_, lease) = Authorizer::new(service.operator().clone())
            .acquire_state_lease(&space_id)
            .await?;
        lease.prepare_mutation().await?;
        lease.write_fence()
    };
    ugoite_iceberg::authorization::with_authorization_write_fence(
        undo_fence,
        service.upsert_form(
            &space_id,
            &json!({"name":"UndoRaceNote","fields":{"Body":{"type":"markdown","required":true}}}),
        ),
    )
    .await?;
    let mut initial = BTreeMap::new();
    initial.insert("Body".to_string(), json!("baseline"));
    service
        .create_structured_entry_authorized_for_principals(
            &space_id,
            "undo-race-entry",
            "UndoRaceNote".to_string(),
            Vec::new(),
            initial,
            BTreeMap::new(),
            "S3 owner",
            &[owner],
        )
        .await?;
    Authorizer::new(service.operator().clone())
        .add_human_member(
            &space_id,
            owner,
            ugoite_domain::identity::SpacePrincipal {
                principal_id: editor,
                kind: ugoite_domain::identity::PrincipalKind::Human,
                display_name: "Undo editor".to_string(),
                state: ugoite_domain::identity::PrincipalState::Active,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            ugoite_domain::identity::SpaceRole::Editor,
        )
        .await?;
    let run_id = ugoite_domain::change::RunId::new("shared-run")?;
    let mut original_change_ids = Vec::new();
    for (change_id, value) in [("undo-change-a", "first"), ("undo-change-b", "second")] {
        let change = ugoite_domain::change::ChangeCommand {
            change_id: change_id.to_string(),
            run_id: Some(run_id.clone()),
            actor_principal_id: editor.to_string(),
            message: Some("S3 grouped edit".to_string()),
            reverts_change_id: None,
            created_at_micros: chrono::Utc::now().timestamp_micros(),
        };
        let updated = service
            .update_structured_entry_authorized_for_principals_with_change(
                &space_id,
                "undo-race-entry",
                None,
                None,
                [("Body".to_string(), json!(value))].into_iter().collect(),
                BTreeMap::new(),
                None,
                &editor.to_string(),
                &[editor],
                Some(change),
            )
            .await?;
        original_change_ids.push(updated["change_id"].as_str().unwrap().to_string());
    }

    let gate = env::temp_dir().join(format!("ugoite-s3-undo-gate-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&gate).await?;
    let test_binary = std::env::current_exe()?;
    let common = |role: &str| {
        let mut command = Command::new(&test_binary);
        command
            .arg("--exact")
            .arg("s3_run_undo_stops_after_revoke_between_inverse_changes")
            .arg("--nocapture")
            .env("UGOITE_S3_UNDO_ROLE", role)
            .env("UGOITE_S3_UNDO_ROOT_URI", &root_uri)
            .env("UGOITE_S3_UNDO_SPACE_SLUG", &space_slug)
            .env("UGOITE_S3_UNDO_SPACE_ID", &space_id)
            .env("UGOITE_S3_UNDO_OWNER", owner.to_string())
            .env("UGOITE_S3_UNDO_EDITOR", editor.to_string());
        command
    };
    let mut writer = common("writer")
        .env("UGOITE_TEST_PUBLICATION_GATE_DIR", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    wait_for_gate(&gate.join("entered-1"), &mut writer).await?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    wait_for_gate(&gate.join("entered-2"), &mut writer).await?;
    let revoker = run_child(&mut common("revoker"), "revoker").await?;
    tokio::fs::write(gate.join("release-2"), b"continue").await?;
    let writer = join_child(writer, "writer").await?;
    anyhow::ensure!(
        revoker.status.success(),
        "{}",
        child_failure_detail("Run revoker", &revoker)
    );
    anyhow::ensure!(
        writer.status.success(),
        "{}",
        child_failure_detail("Run undo writer", &writer)
    );
    let result: Value =
        serde_json::from_slice(&tokio::fs::read(gate.join("undo-result.json")).await?)?;
    assert_eq!(result["state"], "partially_saved");
    assert_eq!(result["reverted_change_count"], 1);
    assert_eq!(result["remaining_change_count"], 1);
    assert_eq!(
        result["remaining_change_ids"],
        json!([original_change_ids[0]])
    );
    assert_eq!(
        service.get_entry(&space_id, "undo-race-entry").await?["fields"]["Body"],
        "first"
    );
    let changes = service.list_changes(&space_id).await?;
    let inverse_targets = changes
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|change| {
            change
                .pointer("/change/reverts_change_id")
                .and_then(Value::as_str)
        })
        .collect::<Vec<_>>();
    assert!(inverse_targets.contains(&original_change_ids[1].as_str()));
    assert!(!inverse_targets.contains(&original_change_ids[0].as_str()));
    assert_principal_revoked(&service, &space_id, editor).await?;
    tokio::fs::remove_dir_all(gate).await?;
    Ok(())
}

async fn open_verified_service(
    root_uri: &str,
    endpoint: &str,
    space_slug: &str,
) -> Result<UgoiteService> {
    let service = UgoiteService::new_with_endpoint(root_uri, Some(endpoint))?;
    let authorizer = Authorizer::new(service.operator().clone());
    authorizer.ensure_authoritative_mutation_contract()?;
    authorizer.verify_authoritative_storage(space_slug).await?;
    Ok(service)
}

fn s3_test_config() -> Result<Option<(String, String)>> {
    let required = s3_test_required()?;
    let endpoint = env::var("UGOITE_S3_TEST_ENDPOINT").ok();
    let bucket = env::var("UGOITE_S3_TEST_BUCKET").ok();
    match (endpoint, bucket) {
        (None, None) if required => bail!("S3 endpoint and bucket are required for this test"),
        (None, None) => Ok(None),
        (Some(endpoint), Some(bucket))
            if !endpoint.trim().is_empty() && !bucket.trim().is_empty() =>
        {
            Ok(Some((
                endpoint.trim().to_string(),
                bucket.trim().to_string(),
            )))
        }
        (Some(_), Some(_)) => bail!("S3 test endpoint and bucket must not be empty"),
        _ => bail!("S3 test endpoint and bucket must be configured together"),
    }
}

/// Explicit opt-in parser for `UGOITE_S3_TEST_REQUIRED` (issue #3262).
/// Unset or empty means the S3 backend stays optional; `1`/`true`/`yes`/`on`
/// require it, `0`/`false`/`no`/`off` skip it, and anything else fails with
/// a diagnostic instead of silently requiring (or skipping) the backend.
fn parse_s3_test_required(raw: Option<&str>) -> Result<bool> {
    match raw
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("") => Ok(false),
        Some("1" | "true" | "yes" | "on") => Ok(true),
        Some("0" | "false" | "no" | "off") => Ok(false),
        Some(other) => bail!(
            "UGOITE_S3_TEST_REQUIRED has an unsupported value {other:?}; expected one of \
             1/true/yes/on to require S3 or 0/false/no/off (or unset) to skip it"
        ),
    }
}

fn s3_test_required() -> Result<bool> {
    parse_s3_test_required(env::var("UGOITE_S3_TEST_REQUIRED").ok().as_deref())
}

#[test]
fn s3_test_required_flag_parses_explicitly() {
    assert!(!parse_s3_test_required(None).unwrap());
    assert!(!parse_s3_test_required(Some("")).unwrap());
    for value in ["0", "false", "FALSE", "no", "off", " 0 "] {
        assert!(!parse_s3_test_required(Some(value)).unwrap(), "{value}");
    }
    for value in ["1", "true", "TRUE", "yes", "on", " 1 "] {
        assert!(parse_s3_test_required(Some(value)).unwrap(), "{value}");
    }
    for value in ["2", "required", "yes please"] {
        let error = parse_s3_test_required(Some(value)).unwrap_err();
        assert!(
            error.to_string().contains("UGOITE_S3_TEST_REQUIRED"),
            "{error:?}"
        );
    }
}
