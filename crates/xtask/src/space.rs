use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::{fs, path::Path};
use ugoite_core::error::{AppError, ErrorKind};
use ugoite_domain::identity::{PrincipalKind, PrincipalState, SpaceRole};
use ugoite_iceberg::authorization::Authorizer;
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::verify::{verify_space, VerifyStatus};
use uuid::Uuid;

/// Development-only sample-data seeder. This is the single supported entry
/// for generating sample Spaces; the production `ugoite` CLI intentionally
/// exposes no local-root seeding bypass.
pub(super) fn seed(args: Vec<String>) -> Result<()> {
    let mut root = String::from("./data");
    let mut space_id: Option<String> = None;
    let mut scenario = String::new();
    let mut entry_count = 50_usize;
    let mut seed_value: Option<u64> = None;
    let mut owner: Option<String> = None;
    let mut profile_output: Option<String> = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--root" => {
                root = iter
                    .next()
                    .context("seed: --root requires a value")?
                    .clone();
            }
            "--space-id" => {
                space_id = Some(
                    iter.next()
                        .context("seed: --space-id requires a value")?
                        .clone(),
                );
            }
            "--scenario" => {
                scenario = iter
                    .next()
                    .context("seed: --scenario requires a value")?
                    .clone();
            }
            "--entry-count" => {
                entry_count = iter
                    .next()
                    .context("seed: --entry-count requires a value")?
                    .parse()
                    .context("seed: --entry-count must be an integer")?;
            }
            "--seed" => {
                seed_value = Some(
                    iter.next()
                        .context("seed: --seed requires a value")?
                        .parse()
                        .context("seed: --seed must be an integer")?,
                );
            }
            "--owner" => {
                owner = Some(
                    iter.next()
                        .context("seed: --owner requires a value")?
                        .clone(),
                );
            }
            "--profile-output" => {
                profile_output = Some(
                    iter.next()
                        .context("seed: --profile-output requires a value")?
                        .clone(),
                );
            }
            other => bail!("seed: unknown argument: {other}"),
        }
    }
    let Some(space_id) = space_id else {
        bail!("seed: --space-id is required");
    };
    let trimmed_root = root.trim_end_matches('/').to_string();
    let op = opendal::Operator::new(opendal::services::Fs::default().root(
        if trimmed_root.is_empty() {
            "/"
        } else {
            &trimmed_root
        },
    ))
    .context("seed: open local workspace root")?;
    let root_uri = format!(
        "file://{}/",
        if trimmed_root.is_empty() {
            "/"
        } else {
            trimmed_root.trim_end_matches('/')
        }
    );
    let options = ugoite_iceberg::sample_data::SampleDataOptions {
        space_id: space_id.clone(),
        scenario: scenario.clone(),
        entry_count,
        seed: seed_value,
        owner_display_name: owner
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty()),
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("seed: start async runtime")?;
    let (summary, profile) = runtime.block_on(async {
        if profile_output.is_some() {
            let (summary, profile) =
                ugoite_iceberg::sample_data::create_sample_space_with_terminal_progress_profiled(
                    &op, &root_uri, &options,
                )
                .await?;
            Ok::<_, anyhow::Error>((summary, Some(serde_json::to_value(profile)?)))
        } else {
            let summary = ugoite_iceberg::sample_data::create_sample_space_with_terminal_progress(
                &op, &root_uri, &options,
            )
            .await?;
            Ok((summary, None))
        }
    })?;
    if let (Some(path), Some(profile)) = (profile_output, profile) {
        if let Some(parent) = Path::new(&path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).with_context(|| format!("create {parent:?}"))?;
        }
        fs::write(&path, serde_json::to_vec_pretty(&profile)?)
            .with_context(|| format!("write seed profile {path}"))?;
    }
    println!(
        "{}",
        serde_json::json!({
            "created": true,
            "id": summary.space_id,
            "slug": space_id,
            "scenario": summary.scenario,
            "entry_count": summary.entry_count,
        })
    );
    Ok(())
}

/// Read back a generated Space through the canonical integrity, service, and
/// authorization paths. This is used by CI fixture packaging; it never repairs
/// or mutates a Space.
pub(super) fn verify_seed(args: Vec<String>) -> Result<()> {
    let mut root: Option<String> = None;
    let mut space_id: Option<String> = None;
    let mut scenario: Option<String> = None;
    let mut expected_entry_count: Option<usize> = None;
    let mut expected_owner: Option<String> = None;
    let mut expected_form_names = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--root" => {
                root = Some(
                    iter.next()
                        .context("verify-seed: --root requires a value")?
                        .clone(),
                );
            }
            "--space-id" => {
                space_id = Some(
                    iter.next()
                        .context("verify-seed: --space-id requires a value")?
                        .clone(),
                );
            }
            "--scenario" => {
                scenario = Some(
                    iter.next()
                        .context("verify-seed: --scenario requires a value")?
                        .clone(),
                );
            }
            "--entry-count" => {
                expected_entry_count = Some(
                    iter.next()
                        .context("verify-seed: --entry-count requires a value")?
                        .parse()
                        .context("verify-seed: --entry-count must be an integer")?,
                );
            }
            "--owner" => {
                expected_owner = Some(
                    iter.next()
                        .context("verify-seed: --owner requires a value")?
                        .clone(),
                );
            }
            "--form-name" => expected_form_names.push(
                iter.next()
                    .context("verify-seed: --form-name requires a value")?
                    .clone(),
            ),
            other => bail!("verify-seed: unknown argument: {other}"),
        }
    }
    let root = root.context("verify-seed: --root is required")?;
    let space_slug = space_id.context("verify-seed: --space-id is required")?;
    let scenario = scenario.context("verify-seed: --scenario is required")?;
    let expected_entry_count =
        expected_entry_count.context("verify-seed: --entry-count is required")?;
    if expected_form_names.is_empty() {
        bail!("verify-seed: at least one --form-name is required");
    }
    if expected_owner.as_deref().is_some_and(str::is_empty) {
        bail!("verify-seed: --owner must not be empty");
    }

    let root_path =
        fs::canonicalize(&root).with_context(|| format!("verify-seed: resolve root {root}"))?;
    let root_uri = format!(
        "file://{}/",
        root_path.to_string_lossy().trim_end_matches('/')
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("verify-seed: start async runtime")?;
    let report = runtime.block_on(async {
        let service = UgoiteService::new_without_background_refresh(root_uri)?;
        let space_id = service
            .space_id_by_slug(&space_slug)
            .await?
            .ok_or_else(|| anyhow!("Space not found for slug: {space_slug}"))?;
        let space = service.get_space(&space_id).await?;
        let actual_slug = space
            .get("slug")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Space metadata has no slug"))?;
        if actual_slug != space_slug {
            bail!("Space metadata slug mismatch: expected {space_slug}, got {actual_slug}");
        }
        let space_uid = service.space_uid(&space_id).await?;
        let integrity = verify_space(service.operator(), &space_id, true).await?;
        if !integrity.valid
            || !matches!(
                integrity.status,
                VerifyStatus::Valid | VerifyStatus::ValidWithRebuildableDerivedState
            )
        {
            bail!(
                "canonical Space integrity verification failed: {}",
                serde_json::to_string(&integrity)?
            );
        }
        for (name, section) in [
            ("metadata", &integrity.sections.metadata),
            ("catalog", &integrity.sections.catalog),
            ("forms", &integrity.sections.forms),
            ("entries", &integrity.sections.entries),
            ("changes_and_audit", &integrity.sections.changes_and_audit),
            ("assets", &integrity.sections.assets),
        ] {
            if section.status != VerifyStatus::Valid {
                bail!(
                    "canonical Space {name} verification is {:?}: {}",
                    section.status,
                    section.detail.as_deref().unwrap_or("no detail")
                );
            }
        }

        let forms = service.list_forms(&space_id).await?;
        let mut form_names = forms
            .iter()
            .map(|form| {
                form.get("name")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| anyhow!("Core API returned a Form without a name"))
            })
            .collect::<Result<Vec<_>>>()?;
        form_names.sort();
        let mut expected_names = expected_form_names.clone();
        expected_names.sort();
        expected_names.dedup();
        if form_names != expected_names {
            bail!(
                "Core API Form set mismatch: expected {}, got {}",
                expected_names.join(","),
                form_names.join(",")
            );
        }

        let entries = service.list_entries(&space_id).await?;
        if entries.len() != expected_entry_count {
            bail!(
                "Core API Entry count mismatch: expected {expected_entry_count}, got {}",
                entries.len()
            );
        }
        let mut form_entry_counts = form_names
            .iter()
            .map(|name| (name.clone(), 0))
            .collect::<BTreeMap<_, _>>();
        for entry in &entries {
            let form_name = entry
                .get("form")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("Core API returned an Entry without a Form name"))?;
            if !form_names.iter().any(|name| name == form_name) {
                bail!("Entry refers to an unlisted Form: {form_name}");
            }
            *form_entry_counts.entry(form_name.to_string()).or_default() += 1;
        }
        if form_names
            .iter()
            .any(|name| name != "Entry" && form_entry_counts[name] == 0)
        {
            bail!("Core API Entry distribution leaves a scenario Form empty");
        }
        let sample_entry_id = entries
            .first()
            .and_then(|entry| entry.get("id"))
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("Core API returned no readable Entry identity"))?
            .to_string();

        let authorization = Authorizer::new(service.operator().clone())
            .state_if_present(&space_id, space_uid)
            .await?;
        let owner = if let Some(expected_owner) = expected_owner.as_deref() {
            let state = authorization
                .as_ref()
                .ok_or_else(|| anyhow!("expected an initialized owner authorization state"))?;
            let owners = state
                .memberships
                .values()
                .filter(|membership| membership.role == SpaceRole::Owner)
                .collect::<Vec<_>>();
            if owners.len() != 1 || state.memberships.len() != 1 || state.principals.len() != 1 {
                bail!("owner fixture must have exactly one principal and one owner membership");
            }
            let owner_membership = owners[0];
            let principal = state
                .principals
                .get(&owner_membership.principal_id)
                .ok_or_else(|| anyhow!("owner membership has no persisted principal"))?;
            if principal.display_name != expected_owner
                || principal.state != PrincipalState::Active
                || principal.kind != PrincipalKind::Human
            {
                bail!("persisted owner does not match expected active human {expected_owner}");
            }
            let authorized = service
                .get_entry_authorized_for_principals(
                    &space_id,
                    &sample_entry_id,
                    &[principal.principal_id],
                )
                .await?;
            if authorized.get("id").and_then(Value::as_str) != Some(sample_entry_id.as_str()) {
                bail!("owner-authorized Core API Entry read returned a different Entry");
            }
            let unauthorized = service
                .get_entry_authorized_for_principals(&space_id, &sample_entry_id, &[Uuid::nil()])
                .await
                .expect_err("an unrelated principal must not read an owner-protected Entry");
            let is_forbidden = unauthorized
                .downcast_ref::<AppError>()
                .is_some_and(|error| error.kind() == ErrorKind::Forbidden);
            if !is_forbidden {
                bail!("non-owner authorization probe failed for an unexpected reason: {unauthorized:#}");
            }
            json!({
                "mode": "owner",
                "display_name": principal.display_name,
                "principal_id": principal.principal_id,
                "role": "owner",
                "active": true,
                "authorized_read_verified": true,
                "non_owner_denial_verified": true,
            })
        } else {
            if authorization.is_some() {
                bail!("ownerless fixture unexpectedly contains persisted authorization state");
            }
            if integrity.sections.authorization.status != VerifyStatus::Incomplete {
                bail!("ownerless fixture authorization verification did not report the expected uninitialized state");
            }
            json!({ "mode": "none" })
        };

        Ok::<_, anyhow::Error>(json!({
            "schema_version": 1,
            "space_slug": space_slug,
            "space_uid": space_uid,
            "scenario": scenario,
            "entry_count": entries.len(),
            "form_names": form_names,
            "form_entry_counts": form_entry_counts,
            "owner": owner,
            "integrity": {
                "deep": integrity.deep,
                "status": integrity.status,
                "changes_and_audit": integrity.sections.changes_and_audit.status,
                "authorization": integrity.sections.authorization.status,
            }
        }))
    })?;
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}
