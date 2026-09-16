use crate::config::{
    effective_format, load_config, normalize_space_root, operator_for_path, parse_space_path,
    print_json, print_json_table, print_list_table, resolve_backend_space_uid,
    resolve_space_reference, validated_base_url, EndpointConfig, Format,
};
use crate::http;
use crate::step_up;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::path::Path;
use ugoite_iceberg::sample_data::SampleDataOptions;
use ugoite_iceberg::service::{validate_public_space_patch, UgoiteService};

fn backend_api_mode_error(config: &EndpointConfig, command_name: &str) -> String {
    format!(
        "{command_name} requires backend or api mode.\nRun `ugoite config current` to inspect the active mode, then switch with `ugoite config set --mode backend --backend-url {}` or `ugoite config set --mode api --api-url {}`.",
        config.backend_url, config.api_url
    )
}

#[derive(Args)]
pub struct SpaceCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: SpaceSubCmd,
}

#[derive(Subcommand)]
pub enum SpaceSubCmd {
    /// Create a new space
    #[command(
        long_about = "Create a new space.\n\nRun `ugoite config current` to check whether you are in core, backend, or api mode. The positional value is a local Space path in core mode or the new human-readable Space slug in backend/api mode. A server-generated Space UID is returned after creation and is the authority for all later operations; the requested slug is never a UID.\n\nExamples:\n  # Core mode (full local Space path, optional display name)\n  ugoite space create /root/spaces/my-space --name \"My Space\"\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite space create team-notes --name \"Team Notes\""
    )]
    Create {
        #[arg(
            value_name = "SPACE_SLUG_OR_PATH",
            help = "New Space slug in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
        #[arg(
            long,
            value_name = "DISPLAY_NAME",
            help = "Display name for the new Space; defaults to the requested slug."
        )]
        name: Option<String>,
    },
    /// List spaces
    #[command(
        long_about = "List all spaces.\n\nRun `ugoite config current` to check whether you should pass a local `ROOT_PATH` or omit it entirely.\nUse `ROOT_PATH` in core mode and omit it in backend/api mode.\n\nExamples:\n  # Core mode (workspace root)\n  ugoite space list /root\n\n  # Core mode (spaces directory also accepted)\n  ugoite space list /root/spaces\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite space list"
    )]
    List {
        #[arg(
            value_name = "ROOT_PATH",
            help = "Workspace root in core mode (for example /root or /root/spaces). Omit in backend/api mode."
        )]
        root_path: Option<String>,
    },
    /// Get space metadata
    #[command(
        long_about = "Get space metadata.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or an immutable `SPACE_UID`.\n\nExamples:\n  # Core mode\n  ugoite space get /root/spaces/my-space\n\n  # Backend mode (immutable Space UID)\n  ugoite space get 019f1234-5678-7abc-8def-0123456789ab"
    )]
    Get {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Immutable Space UID in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
    },
    /// Patch space metadata
    #[command(
        long_about = "Patch space metadata.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or an immutable `SPACE_UID`.\n\nExamples:\n  # Core mode\n  ugoite space patch /root/spaces/my-space --name \"Renamed Space\"\n\n  # Backend mode (immutable Space UID)\n  ugoite space patch 019f1234-5678-7abc-8def-0123456789ab --settings '{\"theme\":\"dark\"}'"
    )]
    Patch {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Immutable Space UID in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        storage_config: Option<String>,
        #[arg(long)]
        settings: Option<String>,
    },
    /// Create sample data
    SampleData {
        #[arg(
            value_name = "LOCAL_ROOT",
            help = "Local workspace root (for example . or /root) where spaces/<SPACE_SLUG> will be created"
        )]
        root_path: String,
        #[arg(
            value_name = "SPACE_SLUG",
            help = "Space slug for the generated sample-data space"
        )]
        space_id: String,
        #[arg(
            long,
            help = "Sample-data scenario ID (run `ugoite space sample-scenarios` to list options)"
        )]
        scenario: Option<String>,
        #[arg(
            long,
            default_value_t = 50,
            help = "Approximate number of generated entries for the seeded space"
        )]
        entry_count: usize,
        #[arg(long, help = "Deterministic random seed for reproducible sample data")]
        seed: Option<u64>,
        /// Create a portable owner principal with this display name.
        /// A node binding is still required before remote access.
        #[arg(long)]
        owner: Option<String>,
    },
    /// List sample scenarios
    SampleScenarios,
    /// Create a sample data job
    SampleJob {
        #[arg(
            value_name = "LOCAL_ROOT",
            help = "Local workspace root (for example . or /root) where spaces/<SPACE_SLUG> will be created"
        )]
        root_path: String,
        #[arg(
            value_name = "SPACE_SLUG",
            help = "Space slug for the generated sample-data space"
        )]
        space_id: String,
        #[arg(
            long,
            help = "Sample-data scenario ID (run `ugoite space sample-scenarios` to list options)"
        )]
        scenario: Option<String>,
        #[arg(
            long,
            default_value_t = 50,
            help = "Approximate number of generated entries for the seeded space"
        )]
        entry_count: usize,
        #[arg(long, help = "Deterministic random seed for reproducible sample data")]
        seed: Option<u64>,
        /// Create a portable owner principal with this display name.
        /// A node binding is still required before remote access.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Get sample data job status
    SampleJobStatus {
        #[arg(
            value_name = "LOCAL_ROOT",
            help = "Local workspace root that stores sample-data job state"
        )]
        root_path: String,
        #[arg(help = "Job ID returned by `ugoite space sample-job`")]
        job_id: String,
    },
    /// Test storage connection
    TestConnection { storage_config_json: String },
    /// List space members (backend/api mode only)
    Members {
        #[arg(
            value_name = "SPACE_UID",
            help = "Immutable Space UID in backend/api mode."
        )]
        space_path: String,
    },
    /// List Space audit events (append-only evidence: event/change/revision/actor only, never paths or secrets)
    AuditEvents {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Immutable Space UID in backend/api mode, or a local Space path in core mode."
        )]
        space_path: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        #[arg(long, default_value_t = 50)]
        limit: u64,
    },
}

fn require_local_root<'a>(root_path: Option<&'a str>, command_name: &str) -> Result<&'a str> {
    root_path
        .ok_or_else(|| anyhow::anyhow!("{command_name} requires --root <LOCAL_ROOT> in core mode"))
}

fn require_space_list_root(root_path: Option<&str>) -> Result<String> {
    root_path
        .map(normalize_space_root)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "space list requires ROOT_PATH as /path/to/root or /path/to/root/spaces in core mode"
            )
        })
}

fn resolve_sample_owner_display_name(owner: Option<String>) -> Option<String> {
    match owner {
        Some(owner_display_name) => {
            let owner_display_name = owner_display_name.trim().to_string();
            (!owner_display_name.is_empty()).then_some(owner_display_name)
        }
        None => None,
    }
}

fn validate_patch_settings(settings: &serde_json::Value) -> Result<()> {
    let patch = serde_json::json!({ "settings": settings });
    validate_public_space_patch(&patch).map_err(|error| anyhow::anyhow!(error.to_string()))
}

/// Concise TTY projection for Space audit event rows. Piped output keeps
/// full JSON. Only identity fields are projected; paths and secrets never
/// enter audit events by construction.
fn audit_rows_table(rows: &[serde_json::Value]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let metadata = row.get("metadata");
            let actor = row
                .get("actor_principal_id")
                .or_else(|| row.get("subject_principal_id"))
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let target_type = row
                .get("target_type")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let target_id = row
                .get("target_id")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            serde_json::json!({
                "event_id": row.get("event_id").and_then(|value| value.as_str()).unwrap_or_default(),
                "action": row.get("action").and_then(|value| value.as_str()).unwrap_or_default(),
                "actor": actor,
                "target": if target_type.is_empty() { target_id.to_owned() } else { format!("{target_type}:{target_id}") },
                "revision_id": metadata.and_then(|meta| meta.get("revision_id")).and_then(|value| value.as_str()).unwrap_or_default(),
            })
        })
        .collect()
}

pub async fn create_space_cmd(
    root_path: Option<&str>,
    space_id: &str,
    command_name: &str,
) -> Result<()> {
    create_space_cmd_with_name(root_path, space_id, None, command_name).await
}

pub async fn create_space_cmd_with_name(
    root_path: Option<&str>,
    space_id: &str,
    display_name: Option<&str>,
    command_name: &str,
) -> Result<()> {
    let config = load_config()?;
    let requested_slug = parse_space_path(space_id).1;
    let resolved_name = display_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(&requested_slug)
        .to_string();
    if let Some(base) = validated_base_url(&config)? {
        // Remote Space creation may require fresh human presence; the
        // step-up handoff (browser approval, one automatic retry) keeps the
        // ceremony policy intact instead of weakening it.
        let result = step_up::execute_with_step_up(
            &base,
            "space.create",
            serde_json::json!({}),
            Some(serde_json::json!({"slug": requested_slug, "name": resolved_name})),
            None,
        )
        .await?;
        print_json(&result);
        return Ok(());
    }
    let root_path = require_local_root(root_path, command_name)?;
    let service = UgoiteService::new_without_background_refresh(root_path)?;
    let outcome = service
        .ensure_operator_space_with_name(&requested_slug, &resolved_name)
        .await?;
    print_json(
        &serde_json::json!({"created": outcome.created(), "id": outcome.space_id(), "slug": requested_slug, "name": resolved_name}),
    );
    Ok(())
}

pub async fn run(cmd: SpaceCmd) -> Result<()> {
    let config = load_config()?;
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        SpaceSubCmd::Create { space_path, name } => {
            if let Some(base) = validated_base_url(&config)? {
                // Backend/api creation takes a new human-readable slug; the
                // server-generated Space UID in the response is the authority
                // for all later operations. Never treat the requested slug as
                // a UID and never fall back to another Space.
                let requested_slug = parse_space_path(&space_path).1;
                let resolved_name = name
                    .as_deref()
                    .map(str::trim)
                    .filter(|display| !display.is_empty())
                    .unwrap_or(&requested_slug)
                    .to_string();
                let result = step_up::execute_with_step_up(
                    &base,
                    "space.create",
                    serde_json::json!({}),
                    Some(serde_json::json!({"slug": requested_slug, "name": resolved_name})),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let requested_slug = parse_space_path(&space_path).1;
            let resolved_name = name
                .as_deref()
                .map(str::trim)
                .filter(|display| !display.is_empty())
                .unwrap_or(&requested_slug)
                .to_string();
            let (root, _) = resolve_space_reference(&config, &space_path, "space create")?;
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let outcome = service
                .ensure_operator_space_with_name(&requested_slug, &resolved_name)
                .await?;
            print_json(
                &serde_json::json!({"created": outcome.created(), "id": outcome.space_id(), "slug": requested_slug, "name": resolved_name}),
            );
        }
        SpaceSubCmd::List { root_path } => {
            if let Some(base) = validated_base_url(&config)? {
                let result =
                    http::execute(&base, "space.list", serde_json::json!({}), None).await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("SPACE_UID", "space_uid"), ("NAME", "name")]);
                        return Ok(());
                    }
                }
                print_json(&result);
                return Ok(());
            }
            let root_path = require_space_list_root(root_path.as_deref())?;
            let service = UgoiteService::new_without_background_refresh(&root_path)?;
            let spaces = service.list_space_ids().await?;
            if fmt != Format::Json {
                let paths = spaces
                    .iter()
                    .map(|space_id| {
                        Path::new(&root_path)
                            .join(service.workspace_path(space_id))
                            .display()
                            .to_string()
                    })
                    .collect::<Vec<_>>();
                print_list_table("LOCAL_SPACE_PATH", &paths);
            } else {
                print_json(&spaces);
            }
        }
        SpaceSubCmd::Get { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "space get")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "space.get",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let space = service.get_space(&space_id).await?;
            print_json(&space);
        }
        SpaceSubCmd::Patch {
            space_path,
            name,
            storage_config,
            settings,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "space patch")?;
            let mut patch = serde_json::Map::new();
            if let Some(n) = name {
                patch.insert("name".to_string(), serde_json::json!(n));
            }
            if let Some(s) = &storage_config {
                let v: serde_json::Value = serde_json::from_str(s)?;
                patch.insert("storage_config".to_string(), v);
            }
            if let Some(s) = &settings {
                let v: serde_json::Value = serde_json::from_str(s)?;
                validate_patch_settings(&v)?;
                patch.insert("settings".to_string(), v);
            }
            if let Some(base) = validated_base_url(&config)? {
                let result = step_up::execute_with_step_up(
                    &base,
                    "space.patch",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::Value::Object(patch)),
                    Some(space_id.as_str()),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .patch_space(&space_id, &serde_json::Value::Object(patch))
                .await?;
            print_json(&result);
        }
        SpaceSubCmd::SampleData {
            root_path,
            space_id,
            scenario,
            entry_count,
            seed,
            owner,
        } => {
            let op = operator_for_path(&root_path)?;
            let root_uri = format!("file://{}/", root_path.trim_end_matches('/'));
            let opts = SampleDataOptions {
                space_id: space_id.clone(),
                scenario: scenario.unwrap_or_default(),
                entry_count,
                seed,
                owner_display_name: resolve_sample_owner_display_name(owner),
            };
            let summary = ugoite_iceberg::sample_data::create_sample_space_with_terminal_progress(
                &op, &root_uri, &opts,
            )
            .await?;
            print_json(&serde_json::json!({
                "created": true,
                "id": summary.space_id,
                "slug": space_id,
                "scenario": summary.scenario,
                "entry_count": summary.entry_count,
                "form_count": summary.form_count,
                "forms": summary.forms,
            }));
        }
        SpaceSubCmd::SampleScenarios => {
            let scenarios = ugoite_iceberg::sample_data::list_sample_scenarios();
            print_json(&scenarios);
        }
        SpaceSubCmd::SampleJob {
            root_path,
            space_id,
            scenario,
            entry_count,
            seed,
            owner,
        } => {
            let op = operator_for_path(&root_path)?;
            let root_uri = format!("file://{}/", root_path.trim_end_matches('/'));
            let opts = SampleDataOptions {
                space_id: space_id.clone(),
                scenario: scenario.unwrap_or_default(),
                entry_count,
                seed,
                owner_display_name: resolve_sample_owner_display_name(owner),
            };
            let job = ugoite_iceberg::sample_data::create_sample_space_job_and_wait(
                &op, &root_uri, &opts,
            )
            .await?;
            print_json(&job);
        }
        SpaceSubCmd::SampleJobStatus { root_path, job_id } => {
            let op = operator_for_path(&root_path)?;
            let job = ugoite_iceberg::sample_data::get_sample_space_job(&op, &job_id).await?;
            let v = serde_json::to_value(job)?;
            print_json(&v);
        }
        SpaceSubCmd::TestConnection {
            storage_config_json,
        } => {
            let payload: serde_json::Value = serde_json::from_str(&storage_config_json)?;
            let result = ugoite_iceberg::service::probe_storage_connection(
                &ugoite_iceberg::space::StorageConnectionTestConfig::from_payload(&payload)?,
            )
            .await?;
            print_json(&result);
        }
        SpaceSubCmd::Members { space_path } => {
            let space_id = resolve_backend_space_uid(&space_path, "space members")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "space.members.list",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            bail!("{}", backend_api_mode_error(&config, "members"));
        }
        SpaceSubCmd::AuditEvents {
            space_path,
            offset,
            limit,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "space audit-events")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "space.audit",
                    serde_json::json!({"space_id": space_id, "offset": offset, "limit": limit}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(rows) = result.get("items").and_then(|value| value.as_array()) {
                        let table = audit_rows_table(rows);
                        print_json_table(
                            &table,
                            &[
                                ("EVENT_ID", "event_id"),
                                ("ACTION", "action"),
                                ("ACTOR", "actor"),
                                ("TARGET", "target"),
                                ("REVISION", "revision_id"),
                            ],
                        );
                        return Ok(());
                    }
                }
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            // Open hook heals crash-missing evidence; the list itself is a
            // light read of committed evidence.
            service.open_space(&space_id).await?;
            let result = service
                .list_space_audit(&space_id, offset as usize, limit as usize)
                .await?;
            if fmt != Format::Json {
                if let Some(rows) = result.get("items").and_then(|value| value.as_array()) {
                    let table = audit_rows_table(rows);
                    print_json_table(
                        &table,
                        &[
                            ("EVENT_ID", "event_id"),
                            ("ACTION", "action"),
                            ("ACTOR", "actor"),
                            ("TARGET", "target"),
                            ("REVISION", "revision_id"),
                        ],
                    );
                    return Ok(());
                }
            }
            print_json(&result);
        }
    }
    Ok(())
}
