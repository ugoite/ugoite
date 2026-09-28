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
    /// Inspect committed Changes correlated to a Run
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Show {
        #[arg(value_name = "RUN_ID")]
        run_id: String,
        /// Number of committed Changes to show (1..=10)
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Continue paging using the opaque cursor
        #[arg(long)]
        cursor: Option<String>,
    },
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
        RunSubCmd::Show {
            run_id,
            limit,
            cursor,
        } => {
            if run_id.trim().is_empty() {
                return Err(UsageError("RUN_ID must not be blank".to_string()).into());
            }
            if !(1..=10).contains(&limit) {
                return Err(UsageError("--limit must be between 1 and 10".to_string()).into());
            }
            if cursor
                .as_deref()
                .is_some_and(|cursor| cursor.trim().is_empty())
            {
                return Err(UsageError("--cursor must not be blank".to_string()).into());
            }
            let target = resolve_command_target(explicit_config, context_override, "run show")?;
            let result = if let SpaceTarget::Remote { space_uid, .. } = &target {
                let mut args = serde_json::json!({
                    "space_id": space_uid,
                    "run_id": run_id,
                    "limit": limit,
                });
                if let Some(cursor) = cursor.as_ref() {
                    args["cursor"] = serde_json::Value::String(cursor.clone());
                }
                http::execute_for_target(&target, "run.inspect", args, None).await?
            } else if let SpaceTarget::Core { root, space_id } = &target {
                UgoiteService::new_without_background_refresh(root)?
                    .inspect_run(space_id, &run_id, Some(limit), cursor.as_deref())
                    .await?
            } else {
                anyhow::bail!("operation run.inspect does not use the remote transport")
            };
            emit_success(&result, &fmt, Some(render_run_inspection(&result)));
        }
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
                let human = render_run_undo_preview(&run_id, &preview);
                emit_success(&preview, &fmt, Some(human));
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

fn render_run_undo_preview(run_id: &str, preview: &serde_json::Value) -> String {
    let mut lines = vec![format!(
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
        } else if preview
            .get("complete")
            .and_then(|complete| complete.as_bool())
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
    lines.join("\n")
}

fn render_run_inspection(inspection: &serde_json::Value) -> String {
    let run_id = inspection
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let changes = inspection
        .get("changes")
        .and_then(serde_json::Value::as_array);
    let mut lines = vec![format!(
        "Run {run_id}: {} committed Change(s) on this page",
        changes.map_or(0, Vec::len)
    )];
    if let Some(changes) = changes {
        for change in changes {
            let change_id = change
                .get("change_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let generation = change
                .get("generation")
                .map(serde_json::Value::to_string)
                .unwrap_or_else(|| "?".to_string());
            let visibility = change
                .get("target_visibility")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("partial");
            let count = change
                .get("summary")
                .and_then(|summary| summary.get("affected_entry_count"))
                .and_then(serde_json::Value::as_u64)
                .map(|count| format!("{count} affected Entry(s)"))
                .unwrap_or_else(|| "affected count unavailable".to_string());
            lines.push(format!(
                "  {change_id} (generation {generation}): {count}, {visibility} visibility"
            ));
        }
    }
    if inspection
        .get("next_cursor")
        .and_then(serde_json::Value::as_str)
        .is_some()
    {
        lines.push("More committed Changes are available with --cursor.".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{render_run_inspection, render_run_undo_preview};
    use serde_json::json;

    #[test]
    fn run_inspection_hides_counts_without_a_complete_summary() {
        let rendered = render_run_inspection(&json!({
            "run_id": "run-1",
            "changes": [{
                "change_id": "change-1",
                "generation": 7,
                "target_visibility": "partial",
                "summary": null
            }],
            "next_cursor": null
        }));

        assert!(rendered.contains("Run run-1: 1 committed Change(s) on this page"));
        assert!(rendered
            .contains("change-1 (generation 7): affected count unavailable, partial visibility"));
        assert!(!rendered.contains("More committed Changes"));
    }

    #[test]
    fn run_undo_summary_discloses_bounded_preview() {
        let rendered = render_run_undo_preview(
            "run-1",
            &json!({
                "committed_change_count": 3,
                "already_reverted_count": 1,
                "pending_change_count": 2,
                "ready": false,
                "complete": false,
                "change_limit": 2,
                "changes": [
                    {"change_id": "change-3", "target_entry_count": 2, "state": "ready"},
                    {"change_id": "change-2", "target_entry_count": 1, "state": "already_reverted"}
                ]
            }),
        );

        assert!(rendered.contains(
            "Run run-1: 3 committed Change(s), 1 already reverted, 2 pending; preview incomplete."
        ));
        assert!(rendered.contains("Only the first 2 Changes are shown."));
        assert!(rendered.contains("change-3: 2 Entries, ready"));
        assert!(rendered.contains("change-2: 1 Entry, already_reverted"));
        assert_eq!(rendered.matches(": 1 Entry").count(), 1);
    }
}
