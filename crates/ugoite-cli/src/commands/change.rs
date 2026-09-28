use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use crate::output::{
    effective_format, emit_mutation, emit_success, print_json_table, Format, MutationReceipt,
    UsageError,
};
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct ChangeCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: ChangeSubCmd,
}

#[derive(Subcommand)]
pub enum ChangeSubCmd {
    /// List Space Change history
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    List,
    /// Inspect one committed Change and a bounded page of its targets
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Show {
        #[arg(value_name = "CHANGE_ID")]
        change_id: String,
        /// Number of affected Entries to show (1..=10)
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Continue an inspection using its opaque cursor
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Show typed before/after evidence for one affected Entry
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Target {
        #[arg(value_name = "CHANGE_ID")]
        change_id: String,
        #[arg(value_name = "ENTRY_ID")]
        entry_id: String,
    },
    /// Revert a Change by appending its inverse
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Revert {
        #[arg(value_name = "CHANGE_ID")]
        change_id: String,
        /// Validate the complete revert without appending a Change
        #[arg(long)]
        dry_run: bool,
        #[arg(long, value_name = "MESSAGE")]
        message: Option<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record on the appended Change (local only)"
        )]
        author: String,
    },
}

/// Concise TTY projection for Change history rows. Piped output keeps full JSON.
fn change_rows_table(rows: &[serde_json::Value]) -> Vec<serde_json::Value> {
    rows.iter()
        .map(|row| {
            let change = row.get("change");
            let cell = |keys: &[&str]| {
                keys.iter()
                    .find_map(|key| {
                        row.get(key)
                            .or_else(|| change.and_then(|change| change.get(key)))
                    })
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_owned()
            };
            serde_json::json!({
                "change_id": cell(&["change_id"]),
                "actor": cell(&["actor_principal_id"]),
                "message": change
                    .and_then(|change| change.get("message"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
                "run_id": change
                    .and_then(|change| change.get("run_id"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn render_change_inspection(inspection: &serde_json::Value) -> String {
    let change_id = inspection
        .get("change_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let visibility = inspection
        .get("target_visibility")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("partial");
    let count = inspection
        .get("summary")
        .and_then(|summary| summary.get("affected_entry_count"))
        .and_then(serde_json::Value::as_u64)
        .map(|count| count.to_string())
        .unwrap_or_else(|| "visible".to_string());
    let mut lines = vec![format!(
        "Change {change_id}: {count} affected Entry(s) ({visibility} visibility)"
    )];
    if let Some(targets) = inspection
        .get("targets")
        .and_then(serde_json::Value::as_array)
    {
        for target in targets {
            let entry_id = target
                .get("entry_id")
                .map(|value| value.to_string())
                .unwrap_or_default();
            let fields = target
                .get("fields")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            lines.push(format!("  Entry {entry_id}: {fields} field change(s)"));
        }
    }
    if inspection.get("next_cursor").is_some() {
        lines.push("More targets are available with --cursor.".to_string());
    }
    lines.join("\n")
}

fn render_change_target(target: &serde_json::Value) -> String {
    let entry_id = target
        .get("target")
        .and_then(|value| value.get("entry_id"))
        .map(|value| value.to_string())
        .unwrap_or_default();
    let mut lines = vec![format!("Entry {entry_id}")];
    if let Some(fields) = target
        .get("target")
        .and_then(|value| value.get("fields"))
        .and_then(serde_json::Value::as_array)
    {
        for field in fields {
            let field_id = field
                .get("field_id")
                .map(|value| value.to_string())
                .unwrap_or_default();
            let before = field
                .get("before")
                .map(serde_json::Value::to_string)
                .unwrap_or_else(|| "null".to_string());
            let after = field
                .get("after")
                .map(serde_json::Value::to_string)
                .unwrap_or_else(|| "null".to_string());
            lines.push(format!("  Field {field_id}: {before} -> {after}"));
        }
    }
    lines.join("\n")
}

pub async fn run(
    cmd: ChangeCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        ChangeSubCmd::List => {
            let target = resolve_command_target(explicit_config, context_override, "change list")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "change.list",
                    serde_json::json!({"space_id": space_uid}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    if let Some(rows) = result.as_array() {
                        let table = change_rows_table(rows);
                        print_json_table(
                            &table,
                            &[
                                ("CHANGE_ID", "change_id"),
                                ("ACTOR", "actor"),
                                ("MESSAGE", "message"),
                                ("RUN_ID", "run_id"),
                            ],
                        );
                        return Ok(());
                    }
                }
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation change.list does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let changes = service.list_changes(space_id).await?;
            if fmt != Format::Json {
                if let Some(rows) = changes.as_array() {
                    let table = change_rows_table(rows);
                    print_json_table(
                        &table,
                        &[
                            ("CHANGE_ID", "change_id"),
                            ("ACTOR", "actor"),
                            ("MESSAGE", "message"),
                            ("RUN_ID", "run_id"),
                        ],
                    );
                } else {
                    emit_success(&changes, &fmt, None);
                }
            } else {
                emit_success(&changes, &fmt, None);
            }
        }
        ChangeSubCmd::Show {
            change_id,
            limit,
            cursor,
        } => {
            if change_id.trim().is_empty() {
                return Err(UsageError("CHANGE_ID must not be blank".to_string()).into());
            }
            if !(1..=10).contains(&limit) {
                return Err(UsageError("--limit must be between 1 and 10".to_string()).into());
            }
            let target = resolve_command_target(explicit_config, context_override, "change show")?;
            let inspection = if let SpaceTarget::Remote { space_uid, .. } = &target {
                let mut args = serde_json::json!({
                    "space_id": space_uid,
                    "change_id": change_id,
                    "limit": limit,
                });
                if let Some(cursor) = &cursor {
                    args["cursor"] = serde_json::json!(cursor);
                }
                http::execute_for_target(&target, "change.inspect", args, None).await?
            } else if let SpaceTarget::Core { root, space_id } = &target {
                UgoiteService::new_without_background_refresh(root)?
                    .inspect_change(space_id, &change_id, Some(limit), cursor.as_deref())
                    .await?
            } else {
                anyhow::bail!("operation change.inspect does not use the remote transport")
            };
            let human = (fmt != Format::Json && fmt != Format::Ndjson)
                .then(|| render_change_inspection(&inspection));
            emit_success(&inspection, &fmt, human);
        }
        ChangeSubCmd::Target {
            change_id,
            entry_id,
        } => {
            if change_id.trim().is_empty() || entry_id.trim().is_empty() {
                return Err(
                    UsageError("CHANGE_ID and ENTRY_ID must not be blank".to_string()).into(),
                );
            }
            let target =
                resolve_command_target(explicit_config, context_override, "change target")?;
            let evidence = if let SpaceTarget::Remote { space_uid, .. } = &target {
                http::execute_for_target(
                    &target,
                    "change.affected.get",
                    serde_json::json!({"space_id": space_uid, "change_id": change_id, "entry_id": entry_id}),
                    None,
                )
                .await?
            } else if let SpaceTarget::Core { root, space_id } = &target {
                UgoiteService::new_without_background_refresh(root)?
                    .change_affected_entry(space_id, &change_id, &entry_id)
                    .await?
            } else {
                anyhow::bail!("operation change.affected.get does not use the remote transport")
            };
            let human = (fmt != Format::Json && fmt != Format::Ndjson)
                .then(|| render_change_target(&evidence));
            emit_success(&evidence, &fmt, human);
        }
        ChangeSubCmd::Revert {
            change_id,
            dry_run,
            message,
            author,
        } => {
            if change_id.trim().is_empty() {
                return Err(UsageError("CHANGE_ID must not be blank".to_string()).into());
            }
            let target =
                resolve_command_target(explicit_config, context_override, "change revert")?;
            if matches!(&target, SpaceTarget::Remote { .. }) && author != "cli" {
                return Err(UsageError(
                    "change revert --author is only supported on a local core connection; remote backend/api connections derive author from the authenticated identity"
                        .to_string(),
                )
                .into());
            }
            if dry_run {
                let preview = if let SpaceTarget::Remote { space_uid, .. } = &target {
                    http::execute_for_target(
                        &target,
                        "change.revert.preview",
                        serde_json::json!({"space_id": space_uid, "change_id": change_id}),
                        None,
                    )
                    .await?
                } else if let SpaceTarget::Core { root, space_id } = &target {
                    UgoiteService::new_without_background_refresh(root)?
                        .preview_revert_change(space_id, &change_id, &author)
                        .await?
                } else {
                    anyhow::bail!(
                        "operation change.revert.preview does not use the remote transport"
                    )
                };
                let human = preview
                    .get("target_entry_count")
                    .and_then(|count| count.as_u64())
                    .map(|count| {
                        let noun = if count == 1 { "Entry" } else { "Entries" };
                        format!("Ready to revert {count} {noun} in Change {change_id}.")
                    });
                emit_success(&preview, &fmt, human);
                return Ok(());
            }
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                if author != "cli" {
                    return Err(UsageError(
                        "change revert --author is only supported on a local core connection; remote backend/api connections derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute_for_target(
                    &target,
                    "change.revert",
                    serde_json::json!({"space_id": space_uid, "change_id": change_id}),
                    Some(serde_json::json!({"message": message})),
                )
                .await?;
                let new_change_id = result
                    .get("change_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string();
                let human = Some(format!("reverted as change {new_change_id}"));
                emit_mutation(&MutationReceipt::change(new_change_id), &fmt, human);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation change.revert does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .revert_change(space_id, &change_id, &author, None, message.as_deref())
                .await?;
            let new_change_id = result
                .get("change_id")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            let human = Some(format!("reverted as change {new_change_id}"));
            emit_mutation(&MutationReceipt::change(new_change_id), &fmt, human);
        }
    }
    Ok(())
}
