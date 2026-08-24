use crate::config::{
    effective_format, load_config, normalize_space_root, operator_for_path, parse_space_path,
    print_json, print_json_table, print_list_table, resolve_space_reference, validated_base_url,
    EndpointConfig, Format,
};
use crate::http;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
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
        long_about = "Create a new space.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<id>` path or a bare `SPACE_ID`.\n\nExamples:\n  # Core mode (full local space path)\n  ugoite space create /root/spaces/my-space\n\n  # Backend mode (requires: ugoite config set --mode backend ...)\n  ugoite space create my-space"
    )]
    Create {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
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
        long_about = "Get space metadata.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<id>` path or a bare `SPACE_ID`.\n\nExamples:\n  # Core mode\n  ugoite space get /root/spaces/my-space\n\n  # Backend mode\n  ugoite space get my-space"
    )]
    Get {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
    },
    /// Patch space metadata
    #[command(
        long_about = "Patch space metadata.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<id>` path or a bare `SPACE_ID`.\n\nExamples:\n  # Core mode\n  ugoite space patch /root/spaces/my-space --name \"Renamed Space\"\n\n  # Backend mode\n  ugoite space patch my-space --settings '{\"theme\":\"dark\"}'"
    )]
    Patch {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
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
            help = "Local workspace root (for example . or /root) where spaces/<SPACE_ID> will be created"
        )]
        root_path: String,
        #[arg(
            value_name = "SPACE_ID",
            help = "Space ID for the generated sample-data space"
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
            help = "Local workspace root (for example . or /root) where spaces/<SPACE_ID> will be created"
        )]
        root_path: String,
        #[arg(
            value_name = "SPACE_ID",
            help = "Space ID for the generated sample-data space"
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
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
    },
    /// Audit events (backend/api mode only)
    AuditEvents {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
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

pub async fn create_space_cmd(
    root_path: Option<&str>,
    space_id: &str,
    command_name: &str,
) -> Result<()> {
    let config = load_config();
    if let Some(base) = validated_base_url(&config)? {
        let result = http::execute(
            &base,
            "space.create",
            serde_json::json!({}),
            Some(serde_json::json!({"name": space_id})),
        )
        .await?;
        print_json(&result);
        return Ok(());
    }
    let root_path = require_local_root(root_path, command_name)?;
    let service = UgoiteService::new_without_background_refresh(root_path)?;
    let immutable_id = service.create_operator_space(space_id).await?;
    print_json(&serde_json::json!({"created": true, "id": immutable_id, "slug": space_id}));
    Ok(())
}

pub async fn run(cmd: SpaceCmd) -> Result<()> {
    let config = load_config();
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        SpaceSubCmd::Create { space_path } => {
            let requested_slug = parse_space_path(&space_path).1;
            let (root, _) = resolve_space_reference(&config, &space_path, "space create")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "space.create",
                    serde_json::json!({}),
                    Some(serde_json::json!({"name": requested_slug})),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let immutable_id = service.create_operator_space(&requested_slug).await?;
            print_json(
                &serde_json::json!({"created": true, "id": immutable_id, "slug": requested_slug}),
            );
        }
        SpaceSubCmd::List { root_path } => {
            if let Some(base) = validated_base_url(&config)? {
                let result =
                    http::execute(&base, "space.list", serde_json::json!({}), None).await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("ID", "id"), ("NAME", "name")]);
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
                print_list_table("SPACE_ID", &spaces);
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
                let result = http::execute(
                    &base,
                    "space.patch",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::Value::Object(patch)),
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
            let config_val: serde_json::Value = serde_json::from_str(&storage_config_json)?;
            let uri = config_val.get("uri").and_then(|v| v.as_str()).unwrap_or("");
            let mode = if uri.starts_with("file://") || uri.starts_with('/') {
                "local"
            } else if uri.starts_with("memory://") {
                "memory"
            } else if uri.starts_with("s3://") {
                "s3"
            } else {
                "unknown"
            };
            print_json(&serde_json::json!({"status": "ok", "mode": mode}));
        }
        SpaceSubCmd::Members { space_path } => {
            let (_, space_id) = parse_space_path(&space_path);
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
            let (_, space_id) = parse_space_path(&space_path);
            if validated_base_url(&config)?.is_some() {
                bail!(
                    "space audit-events for {space_id} (offset {offset}, limit {limit}) is not available in backend/api mode in this release"
                );
            }
            bail!("{}", backend_api_mode_error(&config, "audit-events"));
        }
    }
    Ok(())
}
