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
    /// Revert a Change by appending its inverse
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Revert {
        #[arg(value_name = "CHANGE_ID")]
        change_id: String,
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
        ChangeSubCmd::Revert { change_id, author } => {
            if change_id.trim().is_empty() {
                return Err(UsageError("CHANGE_ID must not be blank".to_string()).into());
            }
            let target =
                resolve_command_target(explicit_config, context_override, "change revert")?;
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
                    Some(serde_json::json!({})),
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
                .revert_change(space_id, &change_id, &author, None, None)
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
