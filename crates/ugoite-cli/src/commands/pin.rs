use crate::cli_config::{resolve_command_triple, split_space_and_id};
use crate::config::{effective_format, print_json, print_json_table, Format};
use crate::http;
use crate::output::{emit_success, UsageError};
use crate::step_up;
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct PinCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: PinSubCmd,
}

#[derive(Subcommand)]
pub enum PinSubCmd {
    /// Create a pin at the current knowledge state
    #[command(
        long_about = "Create a pin capturing the current knowledge state.\n\nA pin is a read-only snapshot identity: it never becomes a mutable branch, and deleting it never deletes entries, revisions, assets, or changes.\n\nExamples:\n  # Core mode\n  ugoite pin create /root/spaces/my-space release-1\n\n  # Backend mode (immutable Space UID)\n  ugoite pin create 019f1234-5678-7abc-8def-0123456789ab release-1"
    )]
    Create {
        #[arg(
            value_name = "SPACE_OR_PIN_NAME",
            num_args(1..=2),
            required = true,
            help = "PIN_NAME against the selected context (New pin name), or legacy SPACE PIN_NAME."
        )]
        space_and_name: Vec<String>,
    },
    /// List pins in a space
    #[command(
        long_about = "List pins in a space.\n\nExamples:\n  # Core mode\n  ugoite pin list /root/spaces/my-space\n\n  # Backend mode (immutable Space UID)\n  ugoite pin list 019f1234-5678-7abc-8def-0123456789ab"
    )]
    List {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Legacy explicit Space (immutable UID or local path). Omit to use the selected context."
        )]
        space_path: Option<String>,
    },
    /// Read one pin without mutating knowledge
    #[command(
        long_about = "Read one pin by name.\n\nRead-only: entry history is unchanged by pin reads. The pin target revision is never confused with the current revision.\n\nExamples:\n  # Core mode\n  ugoite pin read /root/spaces/my-space release-1\n\n  # Backend mode (immutable Space UID)\n  ugoite pin read 019f1234-5678-7abc-8def-0123456789ab release-1"
    )]
    Read {
        #[arg(
            value_name = "SPACE_OR_PIN_NAME",
            num_args(1..=2),
            required = true,
            help = "PIN_NAME against the selected context (Pin name to read), or legacy SPACE PIN_NAME."
        )]
        space_and_name: Vec<String>,
    },
    /// Diff two named pins explicitly
    #[command(
        long_about = "Diff two named pins.\n\nBoth pins are named explicitly; no implicit latest revision is ever selected. Pins never span spaces: a pin from another space is rejected.\n\nExamples:\n  # Core mode\n  ugoite pin diff /root/spaces/my-space --from release-1 --to release-2\n\n  # Backend mode (immutable Space UID)\n  ugoite pin diff 019f1234-5678-7abc-8def-0123456789ab --from release-1 --to release-2"
    )]
    Diff {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Legacy explicit Space (immutable UID or local path). Omit to use the selected context."
        )]
        space_path: Option<String>,
        #[arg(long, help = "Base pin name")]
        from: String,
        #[arg(long, help = "Target pin name")]
        to: String,
    },
    /// Delete a pin identity without deleting knowledge
    #[command(
        long_about = "Delete a pin identity.\n\nOnly the pin identity is removed; entries, revisions, assets, and changes are untouched.\n\nExamples:\n  # Core mode\n  ugoite pin delete /root/spaces/my-space release-1\n\n  # Backend mode (immutable Space UID)\n  ugoite pin delete 019f1234-5678-7abc-8def-0123456789ab release-1"
    )]
    Delete {
        #[arg(
            value_name = "SPACE_OR_PIN_NAME",
            num_args(1..=2),
            required = true,
            help = "PIN_NAME against the selected context (Pin name to delete), or legacy SPACE PIN_NAME."
        )]
        space_and_name: Vec<String>,
    },
}

fn pin_not_found(name: &str) -> AppError {
    AppError::not_found(
        ErrorCode::CheckpointUnavailable,
        format!("Pin {name} not found"),
    )
}

/// Finds one pin in a pin listing without crossing spaces.
///
/// Listings are keyed by pin name inside one Space: a name that is absent
/// here is absent in this Space, and lookup never falls back to another
/// Space or to an implicit revision.
fn find_pin<'a>(pins: &'a serde_json::Value, name: &str) -> Result<&'a serde_json::Value> {
    pins.as_object()
        .and_then(|map| map.get(name))
        .ok_or_else(|| pin_not_found(name).into())
}

fn pin_rows_table(pins: &serde_json::Value) -> Vec<serde_json::Value> {
    pins.as_object()
        .map(|map| {
            map.iter()
                .map(|(name, pin)| {
                    serde_json::json!({
                        "name": name,
                        "created_at_micros": pin.get("created_at_micros"),
                        "created_by": pin.get("created_by_principal_id"),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub async fn run(
    cmd: PinCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        PinSubCmd::Create { space_and_name } => {
            let (legacy_space, name) =
                split_space_and_id(&space_and_name, "PIN_NAME", "pin create")?;
            let name = name.to_string();
            if name.trim().is_empty() {
                return Err(UsageError("PIN_NAME must not be blank".to_string()).into());
            }
            let (root, space_id, base) = resolve_command_triple(
                legacy_space,
                explicit_config,
                context_override,
                "pin create",
            )?;
            if let Some(base) = base {
                let result = step_up::execute_with_step_up(
                    &base,
                    "pin.create",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::json!({"name": name})),
                    Some(space_id.as_str()),
                )
                .await?;
                emit_success(&result, &fmt, Some(format!("created pin {name}")));
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let pin = service
                .create_pin(&space_id, &name, "cli", &uuid::Uuid::now_v7().to_string())
                .await?;
            emit_success(&pin, &fmt, Some(format!("created pin {name}")));
        }
        PinSubCmd::List { space_path } => {
            let (root, space_id, base) = resolve_command_triple(
                space_path.as_deref(),
                explicit_config,
                context_override,
                "pin list",
            )?;
            if let Some(base) = base {
                let result = http::execute(
                    &base,
                    "pin.list",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                if fmt != Format::Json {
                    print_json_table(&pin_rows_table(&result), &[("NAME", "name")]);
                    return Ok(());
                }
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let pins = service.list_pins(&space_id).await?;
            if fmt != Format::Json {
                print_json_table(&pin_rows_table(&pins), &[("NAME", "name")]);
                return Ok(());
            }
            print_json(&pins);
        }
        PinSubCmd::Read { space_and_name } => {
            let (legacy_space, name) = split_space_and_id(&space_and_name, "PIN_NAME", "pin read")?;
            let name = name.to_string();
            let (root, space_id, base) = resolve_command_triple(
                legacy_space,
                explicit_config,
                context_override,
                "pin read",
            )?;
            let pins = if let Some(base) = base {
                http::execute(
                    &base,
                    "pin.list",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?
            } else {
                let service = UgoiteService::new_without_background_refresh(&root)?;
                service.list_pins(&space_id).await?
            };
            let pin = find_pin(&pins, &name)?;
            let mut value = pin.clone();
            if value.get("name").is_none() {
                value["name"] = serde_json::Value::String(name.clone());
            }
            emit_success(&value, &fmt, None);
        }
        PinSubCmd::Diff {
            space_path,
            from,
            to,
        } => {
            if from.trim().is_empty() || to.trim().is_empty() {
                return Err(
                    UsageError("--from and --to pin names must not be blank".to_string()).into(),
                );
            }
            let (root, space_id, base) = resolve_command_triple(
                space_path.as_deref(),
                explicit_config,
                context_override,
                "pin diff",
            )?;
            if let Some(base) = base {
                let result = http::execute(
                    &base,
                    "space.pin_diff",
                    serde_json::json!({"space_id": space_id, "from": from, "to": to}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let diff = service.diff_pins(&space_id, &from, &to).await?;
            emit_success(&diff, &fmt, None);
        }
        PinSubCmd::Delete { space_and_name } => {
            let (legacy_space, name) =
                split_space_and_id(&space_and_name, "PIN_NAME", "pin delete")?;
            let name = name.to_string();
            let (root, space_id, base) = resolve_command_triple(
                legacy_space,
                explicit_config,
                context_override,
                "pin delete",
            )?;
            if let Some(base) = base {
                let result = step_up::execute_with_step_up(
                    &base,
                    "pin.delete",
                    serde_json::json!({"space_id": space_id, "pin_name": name}),
                    None,
                    Some(space_id.as_str()),
                )
                .await?;
                emit_success(&result, &fmt, Some(format!("deleted pin {name}")));
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            service
                .delete_pin(&space_id, &name, &uuid::Uuid::now_v7().to_string())
                .await?;
            emit_success(
                &serde_json::json!({"name": name, "status": "deleted"}),
                &fmt,
                Some(format!("deleted pin {name}")),
            );
        }
    }
    Ok(())
}
