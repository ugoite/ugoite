use crate::cli_config::{resolve_command_triple, split_space_and_id};
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
    #[command(
        long_about = "Undo a Run by appending one inverse Change per Change correlated to the Run, in reverse publication order. The Run itself has no status record; repeating the request resumes it.\n\nThe command invocation itself is the explicit intent; no interactive prompt is shown. When the server requires human approval or reauthentication, the canonical step-up error is returned.\n\nExamples:\n  # Core mode\n  ugoite run undo /root/spaces/my-space run-1\n\n  # Backend mode (immutable Space UID)\n  ugoite run undo 019f1234-5678-7abc-8def-0123456789ab run-1"
    )]
    Undo {
        #[arg(
            value_name = "SPACE_OR_RUN_ID",
            num_args(1..=2),
            required = true,
            help = "RUN_ID against the selected context (Run ID to undo; never inferred), or legacy SPACE RUN_ID."
        )]
        space_and_id: Vec<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record on the appended Changes (core mode only)"
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
            space_and_id,
            author,
        } => {
            let (legacy_space, run_id) = split_space_and_id(&space_and_id, "RUN_ID", "run undo")?;
            let run_id = run_id.to_string();
            if run_id.trim().is_empty() {
                return Err(UsageError("RUN_ID must not be blank".to_string()).into());
            }
            let (root, space_id, base) = resolve_command_triple(
                legacy_space,
                explicit_config,
                context_override,
                "run undo",
            )?;
            if let Some(base) = base {
                if author != "cli" {
                    return Err(UsageError(
                        "run undo --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute(
                    &base,
                    "run.undo",
                    serde_json::json!({"space_id": space_id, "run_id": run_id}),
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
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service.undo_run(&space_id, &run_id, &author).await?;
            let human = result
                .get("reverted_change_count")
                .and_then(|value| value.as_u64())
                .map(|count| format!("undid {count} change(s) for this run"));
            emit_success(&result, &fmt, human);
        }
    }
    Ok(())
}
