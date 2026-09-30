use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::env;
use std::process::{Command, Stdio};
use ugoite_iceberg::{authorization::Authorizer, service::UgoiteService};
use uuid::Uuid;

/// Optional end-to-end proof through the same application service used by the
/// server. Set UGOITE_S3_TEST_REQUIRED=1 to make missing configuration fail.
#[tokio::test]
async fn s3_backed_space_survives_service_reopen_and_revert() -> Result<()> {
    let Some((endpoint, bucket)) = s3_test_config()? else {
        return Ok(());
    };
    let prefix = format!("ugoite/l12/recovery/{}", Uuid::now_v7());
    let root_uri = format!("s3://{bucket}/{prefix}");
    let owner = Uuid::now_v7();
    let space_slug = format!("recovery-{}", Uuid::now_v7().simple());

    let service = open_verified_service(&root_uri, &endpoint, &space_slug).await?;
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
    assert!(reopened.list_space_ids().await?.contains(&space_slug));
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
                anyhow::ensure!(
                    format!("{error:#}").contains("Catalog Head changed"),
                    "writer failed outside the stale Catalog Head CAS: {error:#}"
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
    let (_, form_lease) = Authorizer::new(service.operator().clone())
        .acquire_state_lease(&space_id)
        .await?;
    form_lease.prepare_mutation().await?;
    ugoite_iceberg::authorization::with_authorization_write_fence(
        form_lease.write_fence(),
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
    let winner = tokio::task::spawn_blocking(move || winner.wait_with_output()).await??;
    anyhow::ensure!(
        winner.status.success(),
        "content-first writer failed: {}",
        String::from_utf8_lossy(&winner.stderr)
    );
    let winner_receipt: Value =
        serde_json::from_slice(&tokio::fs::read(gate.join("winner-result.json")).await?)?;
    let winner_change_id = winner_receipt["change_id"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("winner omitted its Change receipt"))?;
    let winning_revoker = common("winner-revoker").output()?;
    anyhow::ensure!(
        winning_revoker.status.success(),
        "content-first revoker failed: {}",
        String::from_utf8_lossy(&winning_revoker.stderr)
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
    assert!(!Authorizer::new(service.operator().clone())
        .state(&space_id)
        .await?
        .memberships
        .contains_key(&winner_editor));
    tokio::fs::remove_dir_all(&gate).await?;
    tokio::fs::create_dir_all(&gate).await?;
    let mut writer = common("writer")
        .env("UGOITE_TEST_PUBLICATION_GATE_DIR", &gate)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let entered = gate.join("entered-1");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
    while !entered.exists() {
        if tokio::time::Instant::now() >= deadline {
            let _ = writer.kill();
            bail!("writer process did not reach the pre-Head-CAS gate");
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }

    let revoker = common("revoker").output()?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    let writer = tokio::task::spawn_blocking(move || writer.wait_with_output()).await??;
    anyhow::ensure!(
        revoker.status.success(),
        "revoker process failed: {}",
        String::from_utf8_lossy(&revoker.stderr)
    );
    anyhow::ensure!(
        writer.status.success(),
        "writer process failed: {}",
        String::from_utf8_lossy(&writer.stderr)
    );

    assert_eq!(
        service.get_entry(&space_id, "race-entry").await?["fields"]["Body"],
        "published before revoke"
    );
    let authorization = Authorizer::new(service.operator().clone())
        .state(&space_id)
        .await?;
    assert!(!authorization.memberships.contains_key(&editor));
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
                anyhow::ensure!(
                    format!("{error:#}").contains("Catalog Head changed"),
                    "Asset writer failed outside the stale Catalog Head CAS: {error:#}"
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
    let revoker = common("revoker").output()?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    let writer = tokio::task::spawn_blocking(move || writer.wait_with_output()).await??;
    anyhow::ensure!(
        revoker.status.success(),
        "Asset revoker failed: {}",
        String::from_utf8_lossy(&revoker.stderr)
    );
    anyhow::ensure!(
        writer.status.success(),
        "Asset writer failed: {}",
        String::from_utf8_lossy(&writer.stderr)
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
    let authorization = Authorizer::new(service.operator().clone())
        .state(&space_id)
        .await?;
    assert!(!authorization.memberships.contains_key(&editor));
    tokio::fs::remove_dir_all(gate).await?;
    Ok(())
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
                anyhow::ensure!(
                    format!("{error:#}").contains("Catalog Head changed"),
                    "revert failed outside the stale Head CAS: {error:#}"
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
    let (_, lease) = Authorizer::new(service.operator().clone())
        .acquire_state_lease(&space_id)
        .await?;
    lease.prepare_mutation().await?;
    ugoite_iceberg::authorization::with_authorization_write_fence(
        lease.write_fence(),
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
    let revoker = common("revoker").output()?;
    tokio::fs::write(gate.join("release-1"), b"continue").await?;
    let writer = tokio::task::spawn_blocking(move || writer.wait_with_output()).await??;
    anyhow::ensure!(
        revoker.status.success(),
        "revoke failed: {}",
        String::from_utf8_lossy(&revoker.stderr)
    );
    anyhow::ensure!(
        writer.status.success(),
        "revert failed: {}",
        String::from_utf8_lossy(&writer.stderr)
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
    assert!(!Authorizer::new(service.operator().clone())
        .state(&space_id)
        .await?
        .memberships
        .contains_key(&editor));
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
    let (_, lease) = Authorizer::new(service.operator().clone())
        .acquire_state_lease(&space_id)
        .await?;
    lease.prepare_mutation().await?;
    ugoite_iceberg::authorization::with_authorization_write_fence(
        lease.write_fence(),
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
    let revoker = common("revoker").output()?;
    tokio::fs::write(gate.join("release-2"), b"continue").await?;
    let writer = tokio::task::spawn_blocking(move || writer.wait_with_output()).await??;
    anyhow::ensure!(
        revoker.status.success(),
        "Run revoker failed: {}",
        String::from_utf8_lossy(&revoker.stderr)
    );
    anyhow::ensure!(
        writer.status.success(),
        "Run undo failed: {}",
        String::from_utf8_lossy(&writer.stderr)
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
    assert!(!Authorizer::new(service.operator().clone())
        .state(&space_id)
        .await?
        .memberships
        .contains_key(&editor));
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
    let required = env::var_os("UGOITE_S3_TEST_REQUIRED").is_some();
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
