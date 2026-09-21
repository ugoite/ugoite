use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::config::print_json;
use crate::http;
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

pub async fn query_cmd(
    sql: &str,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let target = resolve_command_target(explicit_config, context_override, "query")?;
    if let SpaceTarget::Remote { space_uid, .. } = &target {
        let session = http::execute_for_target(
            &target,
            "sql_session.create",
            serde_json::json!({"space_id": space_uid}),
            Some(serde_json::json!({"sql": sql})),
        )
        .await?;
        let session_id = session
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("SQL session response did not include an id"))?;
        let payload = http::execute_for_target(
            &target,
            "sql_session.rows",
            serde_json::json!({
                "space_id": space_uid,
                "session_id": session_id,
                "offset": 0,
                "limit": 1000,
            }),
            None,
        )
        .await?;
        // Fail loudly on protocol drift without changing valid stdout:
        // the server envelope prints verbatim only after strict validation.
        super::sql::decode_remote_rows(&payload, "sql_session.rows")?;
        print_json(&payload);
        return Ok(());
    }
    let SpaceTarget::Core { root, space_id } = &target else {
        anyhow::bail!("operation sql_session.create does not use the remote transport")
    };
    let service = UgoiteService::new_without_background_refresh(root)?;
    let results = service.execute_sql_query(space_id, sql).await?;
    print_json(&results);
    Ok(())
}
