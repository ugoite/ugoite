use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::config::print_json;
use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::time::Duration;
use ugoite_iceberg::service::UgoiteService;

const INDEX_RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Args)]
pub struct IndexCmd {
    #[command(subcommand)]
    pub sub: IndexSubCmd,
}

#[derive(Subcommand)]
pub enum IndexSubCmd {
    /// Reindex a space
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Run {
        #[arg(
            long,
            value_name = "COMPONENT",
            help = "Derived component to rebuild (currently: asset-text)."
        )]
        component: Option<String>,
    },
    /// Show aggregated stats for a space
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Stats,
}

pub async fn run(
    cmd: IndexCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        IndexSubCmd::Run { component } => {
            let target = resolve_command_target(explicit_config, context_override, "index run")?;
            let SpaceTarget::Core { root, space_id } = &target else {
                bail!(
                    "index run is not available in backend/api mode in this release; use core mode for local reindexing"
                );
            };
            if let Some(component) = component.as_deref() {
                if component != "asset-text" {
                    bail!("unsupported index component: {component}; expected asset-text");
                }
            }
            let service = UgoiteService::new_without_background_refresh(root)?;
            tokio::time::timeout(INDEX_RUN_TIMEOUT, async {
                service.reindex(space_id).await?;
                service.garbage_collect_asset_text_builds(space_id).await?;
                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|_| anyhow::anyhow!("index run timed out after 10 minutes"))??;
            print_json(&serde_json::json!({"reindexed": true}));
        }
        IndexSubCmd::Stats => {
            let target = resolve_command_target(explicit_config, context_override, "index stats")?;
            let SpaceTarget::Core { root, space_id } = &target else {
                bail!(
                    "index stats is not available in backend/api mode in this release; use core mode for local index stats"
                );
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let stats = service.space_stats(space_id).await?;
            print_json(&stats);
        }
    }
    Ok(())
}
