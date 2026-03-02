use crate::config::{
    base_url, load_config, operator_for_path, parse_space_path, print_json, space_ws_path,
};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Args)]
pub struct AssetCmd {
    #[command(subcommand)]
    pub sub: AssetSubCmd,
}

#[derive(Subcommand)]
pub enum AssetSubCmd {
    /// List assets in a space
    List { space_path: String },
    /// Upload an asset
    Upload {
        space_path: String,
        file_path: String,
        #[arg(long)]
        filename: Option<String>,
    },
    /// Delete an asset
    Delete {
        space_path: String,
        asset_id: String,
    },
}

pub async fn run(cmd: AssetCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        AssetSubCmd::List { space_path } => {
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/assets"))?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let assets = ugoite_core::asset::list_assets(&op, &ws).await?;
            print_json(&assets);
        }
        AssetSubCmd::Upload {
            space_path,
            file_path,
            filename,
        } => {
            let (root, space_id) = parse_space_path(&space_path);
            if base_url(&config).is_some() {
                anyhow::bail!("asset upload in remote mode not yet supported via CLI");
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let data = std::fs::read(&file_path)?;
            let name = filename.unwrap_or_else(|| {
                std::path::Path::new(&file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("asset")
                    .to_string()
            });
            let asset = ugoite_core::asset::save_asset(&op, &ws, &name, &data).await?;
            print_json(&asset);
        }
        AssetSubCmd::Delete {
            space_path,
            asset_id,
        } => {
            let (root, space_id) = parse_space_path(&space_path);
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_delete(&format!("{base}/spaces/{space_id}/assets/{asset_id}"))?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            ugoite_core::asset::delete_asset(&op, &ws, &asset_id).await?;
            print_json(&serde_json::json!({"deleted": true}));
        }
    }
    Ok(())
}
