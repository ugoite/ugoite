use crate::config::{
    base_url, load_config, operator_for_path, parse_space_path, print_json, space_ws_path,
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
    Run { space_path: String },
    /// Show aggregated stats for a space
    Stats { space_path: String },
}

pub async fn run(cmd: IndexCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        IndexSubCmd::Run { space_path } => {
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
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
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
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
    let (root, space_id) = parse_space_path(space_path);
    if let Some(base) = base_url(&config) {
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
