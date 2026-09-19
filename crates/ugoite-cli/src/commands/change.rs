use crate::cli_config::{resolve_command_triple, split_space_and_id};
use crate::http;
use crate::output::{effective_format, emit_success, print_json_table, Format, UsageError};
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
    #[command(
        long_about = "List the append-only Change history of a Space.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or a bare immutable `SPACE_UID`.\n\nExamples:\n  # Core mode\n  ugoite change list /root/spaces/my-space\n\n  # Backend mode (immutable Space UID)\n  ugoite change list 019f1234-5678-7abc-8def-0123456789ab"
    )]
    List {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Legacy explicit Space (immutable UID or local path). Omit to use the selected context."
        )]
        space_path: Option<String>,
    },
    /// Revert a Change by appending its inverse
    #[command(
        long_about = "Revert a Change by appending its inverse as a new Change. The reverted Change is kept; history never shortens.\n\nThe command invocation itself is the explicit intent; no interactive prompt is shown. When the server requires human approval or reauthentication, the canonical step-up error is returned.\n\nExamples:\n  # Core mode\n  ugoite change revert /root/spaces/my-space change-1\n\n  # Backend mode (immutable Space UID)\n  ugoite change revert 019f1234-5678-7abc-8def-0123456789ab change-1"
    )]
    Revert {
        #[arg(
            value_name = "SPACE_OR_CHANGE_ID",
            num_args(1..=2),
            required = true,
            help = "CHANGE_ID against the selected context (Change ID to revert; never inferred), or legacy SPACE CHANGE_ID."
        )]
        space_and_id: Vec<String>,
        #[arg(
            long,
            default_value = "cli",
            help = "Author name to record on the appended Change (core mode only)"
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
        ChangeSubCmd::List { space_path } => {
            let (root, space_id, base) = resolve_command_triple(
                space_path.as_deref(),
                explicit_config,
                context_override,
                "change list",
            )?;
            if let Some(base) = base {
                let result = http::execute(
                    &base,
                    "change.list",
                    serde_json::json!({"space_id": space_id}),
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
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let changes = service.list_changes(&space_id).await?;
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
        ChangeSubCmd::Revert {
            space_and_id,
            author,
        } => {
            let (legacy_space, change_id) =
                split_space_and_id(&space_and_id, "CHANGE_ID", "change revert")?;
            let change_id = change_id.to_string();
            if change_id.trim().is_empty() {
                return Err(UsageError("CHANGE_ID must not be blank".to_string()).into());
            }
            let (root, space_id, base) = resolve_command_triple(
                legacy_space,
                explicit_config,
                context_override,
                "change revert",
            )?;
            if let Some(base) = base {
                if author != "cli" {
                    return Err(UsageError(
                        "change revert --author is only supported in core mode; backend/api derive author from the authenticated identity"
                            .to_string(),
                    )
                    .into());
                }
                let result = http::execute(
                    &base,
                    "change.revert",
                    serde_json::json!({"space_id": space_id, "change_id": change_id}),
                    Some(serde_json::json!({})),
                )
                .await?;
                let human = result
                    .get("change_id")
                    .and_then(|value| value.as_str())
                    .map(|id| format!("reverted as change {id}"));
                emit_success(&result, &fmt, human);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .revert_change(&space_id, &change_id, &author, None, None)
                .await?;
            let human = result
                .get("change_id")
                .and_then(|value| value.as_str())
                .map(|id| format!("reverted as change {id}"));
            emit_success(&result, &fmt, human);
        }
    }
    Ok(())
}
