use crate::config::{load_config, print_json, resolve_space_reference, validated_base_url};
use crate::http;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use ugoite_core::service::UgoiteService;

#[derive(Args)]
pub struct IndexCmd {
    #[command(subcommand)]
    pub sub: IndexSubCmd,
}

#[derive(Subcommand)]
pub enum IndexSubCmd {
    /// Reindex a space
    #[command(
        long_about = "Reindex a space.\n\nExamples:\n  # Core mode\n  ugoite index run /root/spaces/my-space\n\n  # Backend mode\n  ugoite index run my-space"
    )]
    Run {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
    },
    /// Show aggregated stats for a space
    #[command(
        long_about = "Show aggregated stats for a space.\n\nExamples:\n  # Core mode\n  ugoite index stats /root/spaces/my-space\n\n  # Backend mode\n  ugoite index stats my-space"
    )]
    Stats {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
    },
}

pub async fn run(cmd: IndexCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        IndexSubCmd::Run { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "index run")?;
            if validated_base_url(&config)?.is_some() {
                bail!(
                    "index run is not available in backend/api mode in this release; use core mode for local reindexing"
                );
            }
            let service = UgoiteService::new(&root)?;
            service.reindex(&space_id).await?;
            print_json(&serde_json::json!({"reindexed": true}));
        }
        IndexSubCmd::Stats { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "index stats")?;
            if validated_base_url(&config)?.is_some() {
                bail!(
                    "index stats is not available in backend/api mode in this release; use core mode for local index stats"
                );
            }
            let service = UgoiteService::new(&root)?;
            let stats = service.space_stats(&space_id).await?;
            print_json(&stats);
        }
    }
    Ok(())
}

pub async fn query_cmd(space_path: &str, sql: &str) -> Result<()> {
    let config = load_config();
    let (root, space_id) = resolve_space_reference(&config, space_path, "query")?;
    if let Some(base) = validated_base_url(&config)? {
        let session = http::http_post(
            &format!("{base}/spaces/{space_id}/sql-sessions"),
            &serde_json::json!({ "sql": sql }),
        )
        .await?;
        let session_id = session
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("SQL session response did not include an id"))?;
        let rows = http::http_get(&format!(
            "{base}/spaces/{space_id}/sql-sessions/{session_id}/rows?offset=0&limit=1000"
        ))
        .await?;
        print_json(&rows);
        return Ok(());
    }
    let service = UgoiteService::new(&root)?;
    let results = service.execute_sql_query(&space_id, sql).await?;
    print_json(&results);
    Ok(())
}
