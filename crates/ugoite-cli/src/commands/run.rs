use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use crate::output::{effective_format, emit_success, Format, UsageError};
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct RunCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: RunSubCmd,
}

#[derive(Subcommand)]
pub enum RunSubCmd {
    /// Undo a Run by appending inverses for its Changes
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Undo {
        #[arg(value_name = "RUN_ID")]
        run_id: String,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record on the appended Changes (local only)"
        )]
        author: String,
    },
}

pub async fn run(
    cmd: RunCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        RunSubCmd::Undo { run_id, author } => {
            if run_id.trim().is_empty() {
                return Err(UsageError("RUN_ID must not be blank".to_string()).into());
            }
            let target = resolve_command_target(explicit_config, context_override, "run undo")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "run undo --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute_for_target(
                    &target,
                    "run.undo",
                    serde_json::json!({"space_id": space_uid, "run_id": run_id}),
                    Some(serde_json::json!({})),
                )
                .await?;
                let human = result
                    .get("reverted_change_count")
                    .and_then(|value| value.as_u64())
                    .map(|count| format!("undid {count} change(s) for this run"));
                emit_success(&result, &fmt, human);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation run.undo does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service.undo_run(space_id, &run_id, &author).await?;
            let human = result
                .get("reverted_change_count")
                .and_then(|value| value.as_u64())
                .map(|count| format!("undid {count} change(s) for this run"));
            emit_success(&result, &fmt, human);
        }
    }
    Ok(())
}
