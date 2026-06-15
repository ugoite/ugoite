use crate::config::{
    load_config, operator_for_path, print_json, resolve_space_reference, space_ws_path,
    validated_base_url,
};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};

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
            if let Some(base) = validated_base_url(&config)? {
                let result = http::http_post(
                    &format!("{base}/spaces/{space_id}/index"),
                    &serde_json::json!({}),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            ugoite_core::index::reindex_all(&op, &ws).await?;
            print_json(&serde_json::json!({"reindexed": true}));
        }
        IndexSubCmd::Stats { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "index stats")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/stats")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let stats = ugoite_core::index::get_space_stats(&op, &ws).await?;
            print_json(&stats);
        }
    }
    Ok(())
}

pub async fn query_cmd(space_path: &str, sql: &str) -> Result<()> {
    let config = load_config();
    let (root, space_id) = resolve_space_reference(&config, space_path, "query")?;
    if let Some(base) = validated_base_url(&config)? {
        let result = http::http_get(&format!("{base}/spaces/{space_id}/query?sql={sql}")).await?;
        print_json(&result);
        return Ok(());
    }
    let op = operator_for_path(&root)?;
    let ws = space_ws_path(&root, &space_id);
    let results = ugoite_core::index::execute_sql_query(&op, &ws, sql).await?;
    print_json(&results);
    Ok(())
}
