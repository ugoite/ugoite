use crate::config::{load_config, print_json, resolve_space_reference, validated_base_url};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};
use std::io::Read;
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct AssetCmd {
    #[command(subcommand)]
    pub sub: AssetSubCmd,
}

#[derive(Subcommand)]
pub enum AssetSubCmd {
    /// Upload an asset
    #[command(
        long_about = "Upload an asset.\n\nExamples:\n  # Core mode\n  ugoite asset upload /root/spaces/my-space ./logo.png\n\nRemote CLI upload is not available in this release; use the API client or REST surface for remote uploads."
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
        #[arg(long)]
        human_approval: Option<String>,
    },
}

pub async fn run(cmd: AssetCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        AssetSubCmd::Upload {
            space_path,
            file_path,
            filename,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "asset upload")?;
            if validated_base_url(&config)?.is_some() {
                anyhow::bail!(
                    "asset upload is not available in backend/api mode in this release; upload through the API client or REST surface"
                );
            }
            let file_size = std::fs::metadata(&file_path)?.len();
            if file_size > ugoite_iceberg::asset::MAX_ASSET_BYTES as u64 {
                anyhow::bail!(
                    "asset exceeds the {}-byte size limit",
                    ugoite_iceberg::asset::MAX_ASSET_BYTES
                );
            }
            let mut file = std::fs::File::open(&file_path)?;
            let mut data = Vec::with_capacity(file_size as usize);
            file.by_ref()
                .take(ugoite_iceberg::asset::MAX_ASSET_BYTES as u64 + 1)
                .read_to_end(&mut data)?;
            if data.len() > ugoite_iceberg::asset::MAX_ASSET_BYTES {
                anyhow::bail!(
                    "asset exceeds the {}-byte size limit",
                    ugoite_iceberg::asset::MAX_ASSET_BYTES
                );
            }
            let name = filename.unwrap_or_else(|| {
                std::path::Path::new(&file_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("asset")
                    .to_string()
            });
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let asset = service.save_asset(&space_id, &name, &data).await?;
            print_json(&asset);
        }
        AssetSubCmd::Delete {
            space_path,
            asset_id,
            human_approval,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "asset delete")?;
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "asset.delete",
                    serde_json::json!({"space_id": space_id, "asset_id": asset_id, "human_approval": human_approval}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            if human_approval.is_some() {
                anyhow::bail!("--human-approval is only supported in backend/api mode");
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            service.delete_asset(&space_id, &asset_id).await?;
            print_json(&serde_json::json!({"deleted": true}));
        }
    }
    Ok(())
}
