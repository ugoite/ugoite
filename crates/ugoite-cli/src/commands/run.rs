use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use crate::output::{
    effective_format, emit_mutation, emit_success, Format, MutationReceipt, UsageError,
};
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
        /// Validate the pending inverses without appending Changes
        #[arg(long)]
        dry_run: bool,
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
        RunSubCmd::Undo {
            run_id,
            dry_run,
            author,
        } => {
            if run_id.trim().is_empty() {
                return Err(UsageError("RUN_ID must not be blank".to_string()).into());
            }
            let target = resolve_command_target(explicit_config, context_override, "run undo")?;
            if matches!(&target, SpaceTarget::Remote { .. }) && author != "cli" {
                return Err(UsageError(
                    "run undo --author is only supported on a local core connection; remote backend/api connections derive author from the authenticated identity"
                        .to_string(),
                )
                .into());
            }
            if dry_run {
                let preview = if let SpaceTarget::Remote { space_uid, .. } = &target {
                    http::execute_for_target(
                        &target,
                        "run.undo.preview",
                        serde_json::json!({"space_id": space_uid, "run_id": run_id}),
                        None,
                    )
                    .await?
                } else if let SpaceTarget::Core { root, space_id } = &target {
                    UgoiteService::new_without_background_refresh(root)?
                        .preview_undo_run(space_id, &run_id, &author)
                        .await?
                } else {
                    anyhow::bail!("operation run.undo.preview does not use the remote transport")
                };
                let mut lines =
                    vec![format!(
                    "Run {run_id}: {} committed Change(s), {} already reverted, {} pending; {}.",
                    preview
                        .get("committed_change_count")
                        .and_then(|count| count.as_u64())
                        .unwrap_or_default(),
                    preview
                        .get("already_reverted_count")
                        .and_then(|count| count.as_u64())
                        .unwrap_or_default(),
                    preview
                        .get("pending_change_count")
                        .and_then(|count| count.as_u64())
                        .unwrap_or_default(),
                    if preview.get("ready").and_then(|ready| ready.as_bool()) == Some(true) {
                        "ready to undo"
                    } else if preview.get("complete").and_then(|complete| complete.as_bool())
                        == Some(false)
                    {
                        "preview incomplete"
                    } else {
                        "not ready to undo"
                    }
                )];
                if preview
                    .get("complete")
                    .and_then(|complete| complete.as_bool())
                    == Some(false)
                {
                    let limit = preview
                        .get("change_limit")
                        .and_then(|limit| limit.as_u64())
                        .unwrap_or_default();
                    lines.push(format!("Only the first {limit} Changes are shown."));
                }
                if let Some(changes) = preview.get("changes").and_then(|value| value.as_array()) {
                    for change in changes {
                        let change_id = change
                            .get("change_id")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default();
                        let count = change
                            .get("target_entry_count")
                            .and_then(|value| value.as_u64())
                            .unwrap_or_default();
                        let noun = if count == 1 { "Entry" } else { "Entries" };
                        let state = change
                            .get("state")
                            .and_then(|value| value.as_str())
                            .unwrap_or("unknown");
                        lines.push(format!("  {change_id}: {count} {noun}, {state}"));
                    }
                }
                let human = Some(lines.join("\n"));
                emit_success(&preview, &fmt, human);
                return Ok(());
            }
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "run undo --author is only supported on a local core connection; remote backend/api connections derive author from the authenticated identity"
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
                emit_mutation(&MutationReceipt::run(run_id.clone()), &fmt, human);
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
            emit_mutation(&MutationReceipt::run(run_id.clone()), &fmt, human);
        }
    }
    Ok(())
}
