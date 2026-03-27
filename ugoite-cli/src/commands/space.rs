use crate::config::{
    base_url, effective_format, load_config, operator_for_path, parse_space_path, print_json,
    print_json_table, print_list_table, Format,
};
use crate::http;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use ugoite_core::sample_data::SampleDataOptions;

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
    /// List spaces
    List {
        #[arg(long = "root", value_name = "LOCAL_ROOT")]
        root_path: Option<String>,
    },
    /// Get space metadata
    Get {
        #[arg(long = "root", value_name = "LOCAL_ROOT")]
        root_path: Option<String>,
        #[arg(value_name = "SPACE_ID")]
        space_id: String,
    },
    /// Patch space metadata
    Patch {
        #[arg(long = "root", value_name = "LOCAL_ROOT")]
        root_path: Option<String>,
        #[arg(value_name = "SPACE_ID")]
        space_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        storage_config: Option<String>,
        #[arg(long)]
        settings: Option<String>,
    },
    /// Create sample data
    SampleData {
        #[arg(value_name = "LOCAL_ROOT")]
        root_path: String,
        #[arg(value_name = "SPACE_ID")]
        space_id: String,
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long, default_value_t = 50)]
        entry_count: usize,
        #[arg(long)]
        seed: Option<u64>,
    },
    /// List sample scenarios
    SampleScenarios,
    /// Create a sample data job
    SampleJob {
        #[arg(value_name = "LOCAL_ROOT")]
        root_path: String,
        #[arg(value_name = "SPACE_ID")]
        space_id: String,
        #[arg(long)]
        scenario: Option<String>,
        #[arg(long, default_value_t = 50)]
        entry_count: usize,
        #[arg(long)]
        seed: Option<u64>,
    },
    /// Get sample data job status
    SampleJobStatus {
        #[arg(value_name = "LOCAL_ROOT")]
        root_path: String,
        job_id: String,
    },
    /// Test storage connection
    TestConnection { storage_config_json: String },
    /// List service accounts (backend/api mode only)
    ServiceAccountList {
        #[arg(value_name = "SPACE_ID")]
        space_id: String,
    },
    /// Create a service account (backend/api mode only)
    ServiceAccountCreate {
        #[arg(value_name = "SPACE_ID")]
        space_id: String,
        #[arg(long)]
        display_name: String,
        #[arg(long, value_delimiter = ',')]
        scopes: Vec<String>,
    },
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

pub async fn create_space_cmd(root_path: Option<&str>, space_id: &str) -> Result<()> {
    let config = load_config();
    if let Some(base) = base_url(&config) {
        let result = http::http_post(
            &format!("{base}/spaces"),
            &serde_json::json!({"name": space_id}),
        )
        .await?;
        print_json(&result);
        return Ok(());
    }
    let root_path = require_local_root(root_path, "create-space")?;
    let op = operator_for_path(root_path)?;
    ugoite_core::space::create_space(&op, space_id, root_path).await?;
    print_json(&serde_json::json!({"created": true, "id": space_id}));
    Ok(())
}

pub async fn run(cmd: SpaceCmd) -> Result<()> {
    let config = load_config();
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        SpaceSubCmd::List { root_path } => {
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces")).await?;
                if fmt != Format::Json {
                    if let Some(arr) = result.as_array() {
                        print_json_table(arr, &[("ID", "id"), ("NAME", "name")]);
                        return Ok(());
                    }
                }
                print_json(&result);
                return Ok(());
            }
            let root_path = require_local_root(root_path.as_deref(), "space list")?;
            let op = operator_for_path(root_path)?;
            let spaces = ugoite_core::space::list_spaces(&op).await?;
            if fmt != Format::Json {
                print_list_table("SPACE_ID", &spaces);
            } else {
                print_json(&spaces);
            }
        }
        SpaceSubCmd::Get {
            root_path,
            space_id,
        } => {
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces/{space_id}")).await?;
                print_json(&result);
                return Ok(());
            }
            let root_path = require_local_root(root_path.as_deref(), "space get")?;
            let op = operator_for_path(root_path)?;
            let space = ugoite_core::space::get_space_raw(&op, &space_id).await?;
            print_json(&space);
        }
        SpaceSubCmd::Patch {
            root_path,
            space_id,
            name,
            storage_config,
            settings,
        } => {
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
                patch.insert("settings".to_string(), v);
            }
            if let Some(base) = base_url(&config) {
                let result = http::http_patch(
                    &format!("{base}/spaces/{space_id}"),
                    &serde_json::Value::Object(patch),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let root_path = require_local_root(root_path.as_deref(), "space patch")?;
            let op = operator_for_path(root_path)?;
            let result =
                ugoite_core::space::patch_space(&op, &space_id, &serde_json::Value::Object(patch))
                    .await?;
            print_json(&result);
        }
        SpaceSubCmd::SampleData {
            root_path,
            space_id,
            scenario,
            entry_count,
            seed,
        } => {
            let op = operator_for_path(&root_path)?;
            let root_uri = format!("file://{}/", root_path.trim_end_matches('/'));
            let opts = SampleDataOptions {
                space_id: space_id.clone(),
                scenario: scenario.unwrap_or_default(),
                entry_count,
                seed,
            };
            ugoite_core::sample_data::create_sample_space_with_terminal_progress(
                &op, &root_uri, &opts,
            )
            .await?;
            print_json(&serde_json::json!({"created": true}));
        }
        SpaceSubCmd::SampleScenarios => {
            let scenarios = ugoite_core::sample_data::list_sample_scenarios();
            print_json(&scenarios);
        }
        SpaceSubCmd::SampleJob {
            root_path,
            space_id,
            scenario,
            entry_count,
            seed,
        } => {
            let op = operator_for_path(&root_path)?;
            let root_uri = format!("file://{}/", root_path.trim_end_matches('/'));
            let opts = SampleDataOptions {
                space_id: space_id.clone(),
                scenario: scenario.unwrap_or_default(),
                entry_count,
                seed,
            };
            let job =
                ugoite_core::sample_data::create_sample_space_job(&op, &root_uri, &opts).await?;
            print_json(&job);
        }
        SpaceSubCmd::SampleJobStatus { root_path, job_id } => {
            let op = operator_for_path(&root_path)?;
            let job = ugoite_core::sample_data::get_sample_space_job(&op, &job_id).await?;
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
        SpaceSubCmd::ServiceAccountList { space_id } => {
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_get(&format!("{base}/spaces/{space_id}/service-accounts")).await?;
                print_json(&result);
                return Ok(());
            }
            bail!("service-account-list requires backend or api mode");
        }
        SpaceSubCmd::ServiceAccountCreate {
            space_id,
            display_name,
            scopes,
        } => {
            if let Some(base) = base_url(&config) {
                let result = http::http_post(
                    &format!("{base}/spaces/{space_id}/service-accounts"),
                    &serde_json::json!({
                        "display_name": display_name,
                        "scopes": scopes,
                    }),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            bail!("service-account-create requires backend or api mode");
        }
        SpaceSubCmd::Members { space_path } => {
            let (_, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/members")).await?;
                print_json(&result);
                return Ok(());
            }
            bail!("members requires backend or api mode");
        }
        SpaceSubCmd::AuditEvents {
            space_path,
            offset,
            limit,
        } => {
            let (_, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!(
                    "{base}/spaces/{space_id}/audit-events?offset={offset}&limit={limit}"
                ))
                .await?;
                print_json(&result);
                return Ok(());
            }
            bail!("audit-events requires backend or api mode");
        }
    }
    Ok(())
}
