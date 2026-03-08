use crate::config::{
    base_url, load_config, operator_for_path, parse_space_path, print_json, space_ws_path,
};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct SearchCmd {
    #[command(subcommand)]
    pub sub: SearchSubCmd,
}

#[derive(Subcommand)]
pub enum SearchSubCmd {
    /// Keyword search
    Keyword { space_path: String, query: String },
}

pub async fn run(cmd: SearchCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        SearchSubCmd::Keyword { space_path, query } => {
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_get(&format!("{base}/spaces/{space_id}/search?q={query}")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let results = ugoite_core::search::search_entries(&op, &ws, &query).await?;
            print_json(&results);
        }
    }
    Ok(())
}
