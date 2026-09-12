use crate::config::{load_config, resolve_space_reference, validated_base_url};
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
        long_about = "Undo a Run by appending one inverse Change per Change correlated to the Run, in reverse publication order. The Run itself has no status record; repeating the request resumes it.\n\nThe command invocation itself is the explicit intent; no interactive prompt is shown. When the server requires human approval or reauthentication, the canonical step-up error is returned.\n\nExamples:\n  # Core mode\n  ugoite run undo /root/spaces/my-space run-1\n\n  # Backend mode\n  ugoite run undo my-space run-1"
    )]
    Undo {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            value_name = "RUN_ID",
            help = "Run ID to undo. Take it from change list output; never inferred."
        )]
        run_id: String,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record on the appended Changes (core mode only)"
        )]
        author: String,
    },
}

pub async fn run(cmd: RunCmd) -> Result<()> {
    let config = load_config();
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        RunSubCmd::Undo {
            space_path,
            run_id,
            author,
        } => {
            if run_id.trim().is_empty() {
                return Err(UsageError("RUN_ID must not be blank".to_string()).into());
            }
            let (root, space_id) = resolve_space_reference(&config, &space_path, "run undo")?;
            if let Some(base) = validated_base_url(&config)? {
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
