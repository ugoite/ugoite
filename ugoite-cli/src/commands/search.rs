use crate::config::{
    base_url, load_config, operator_for_path, print_json, resolve_space_reference, space_ws_path,
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
    #[command(
        long_about = "Run keyword search.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<id>` path or a bare `SPACE_ID`.\n\nExamples:\n  # Core mode\n  ugoite search keyword /root/spaces/my-space invoice\n\n  # Backend mode\n  ugoite search keyword my-space invoice"
    )]
    Keyword {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            value_name = "QUERY",
            help = "Plain-text query string to match against indexed entry content."
        )]
        query: String,
    },
}

pub async fn run(cmd: SearchCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        SearchSubCmd::Keyword { space_path, query } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "search keyword")?;
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
