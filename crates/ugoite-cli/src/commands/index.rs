use crate::cli_config::resolve_command_triple;
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
    #[command(
        long_about = "Reindex a space.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite index run\n\n  # Legacy explicit Space (v0.1.x compatibility)\n  ugoite index run /root/spaces/my-space\n  ugoite index run 019f1234-5678-7abc-8def-0123456789ab"
    )]
    Run {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Legacy explicit Space (immutable UID or local path). Omit to use the selected context."
        )]
        space_path: Option<String>,
        #[arg(
            long,
            value_name = "COMPONENT",
            help = "Derived component to rebuild (currently: asset-text)."
        )]
        component: Option<String>,
    },
    /// Show aggregated stats for a space
    #[command(
        long_about = "Show aggregated stats for a space.\n\nExamples:\n  # Core mode\n  ugoite index stats /root/spaces/my-space\n\n  # Backend mode (immutable Space UID)\n  ugoite index stats 019f1234-5678-7abc-8def-0123456789ab"
    )]
    Stats {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Legacy explicit Space (immutable UID or local path). Omit to use the selected context."
        )]
        space_path: Option<String>,
    },
}

pub async fn run(
    cmd: IndexCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        IndexSubCmd::Run {
            space_path,
            component,
        } => {
            let (root, space_id, base) = resolve_command_triple(
                space_path.as_deref(),
                explicit_config,
                context_override,
                "index run",
            )?;
            if base.is_some() {
                bail!(
                    "index run is not available in backend/api mode in this release; use core mode for local reindexing"
                );
            }
            if let Some(component) = component.as_deref() {
                if component != "asset-text" {
                    bail!("unsupported index component: {component}; expected asset-text");
                }
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            tokio::time::timeout(INDEX_RUN_TIMEOUT, async {
                service.reindex(&space_id).await?;
                service.garbage_collect_asset_text_builds(&space_id).await?;
                Ok::<(), anyhow::Error>(())
            })
            .await
            .map_err(|_| anyhow::anyhow!("index run timed out after 10 minutes"))??;
            print_json(&serde_json::json!({"reindexed": true}));
        }
        IndexSubCmd::Stats { space_path } => {
            let (root, space_id, base) = resolve_command_triple(
                space_path.as_deref(),
                explicit_config,
                context_override,
                "index stats",
            )?;
            if base.is_some() {
                bail!(
                    "index stats is not available in backend/api mode in this release; use core mode for local index stats"
                );
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let stats = service.space_stats(&space_id).await?;
            print_json(&stats);
        }
    }
    Ok(())
}

pub async fn query_cmd(
    space_path: Option<&str>,
    sql: &str,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let (root, space_id, base) =
        resolve_command_triple(space_path, explicit_config, context_override, "query")?;
    if let Some(base) = base {
        let session = http::execute(
            &base,
            "sql_session.create",
            serde_json::json!({"space_id": space_id}),
            Some(serde_json::json!({"sql": sql})),
        )
        .await?;
        let session_id = session
            .get("id")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("SQL session response did not include an id"))?;
        let payload = http::execute(
            &base,
            "sql_session.rows",
            serde_json::json!({
                "space_id": space_id,
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
    let service = UgoiteService::new_without_background_refresh(&root)?;
    let results = service.execute_sql_query(&space_id, sql).await?;
    print_json(&results);
    Ok(())
}
