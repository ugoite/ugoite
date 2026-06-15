use crate::config::{
    load_config, operator_for_path, print_json, resolve_space_reference, space_ws_path,
    validated_base_url,
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
    #[command(
        long_about = "List assets in a space.\n\nExamples:\n  # Core mode\n  ugoite asset list /root/spaces/my-space\n\n  # Backend mode\n  ugoite asset list my-space"
    )]
    List {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
    },
    /// Upload an asset
    #[command(
        long_about = "Upload an asset.\n\nExamples:\n  # Core mode\n  ugoite asset upload /root/spaces/my-space ./logo.png\n\n  # Backend mode\n  ugoite asset upload my-space ./logo.png"
    )]
    Upload {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        file_path: String,
        #[arg(long)]
        filename: Option<String>,
    },
    /// Delete an asset
    #[command(
        long_about = "Delete an asset.\n\nExamples:\n  # Core mode\n  ugoite asset delete /root/spaces/my-space asset-123\n\n  # Backend mode\n  ugoite asset delete my-space asset-123"
    )]
    Delete {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        asset_id: String,
    },
}

pub async fn run(cmd: AssetCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        AssetSubCmd::List { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "asset list")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/assets")).await?;
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
            let (root, space_id) = resolve_space_reference(&config, &space_path, "asset upload")?;
            if validated_base_url(&config)?.is_some() {
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
            let (root, space_id) = resolve_space_reference(&config, &space_path, "asset delete")?;
            if let Some(base) = validated_base_url(&config)? {
                let result =
                    http::http_delete(&format!("{base}/spaces/{space_id}/assets/{asset_id}"))
                        .await?;
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
