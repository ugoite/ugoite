use crate::cli_config::{resolve_command_target, SpaceTarget};
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
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Create {
        #[arg(value_name = "PIN_NAME")]
        name: String,
    },
    /// List pins in a space
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    List,
    /// Read one pin without mutating knowledge
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Read {
        #[arg(value_name = "PIN_NAME")]
        name: String,
    },
    /// Diff two named pins explicitly
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Diff {
        #[arg(long, help = "Base pin name")]
        from: String,
        #[arg(long, help = "Target pin name")]
        to: String,
    },
    /// Delete a pin identity without deleting knowledge
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Delete {
        #[arg(value_name = "PIN_NAME")]
        name: String,
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
        PinSubCmd::Create { name } => {
            if name.trim().is_empty() {
                return Err(UsageError("PIN_NAME must not be blank".to_string()).into());
            }
            let target = resolve_command_target(explicit_config, context_override, "pin create")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = step_up::execute_with_step_up_for_target(
                    &target,
                    "pin.create",
                    serde_json::json!({"space_id": space_uid}),
                    Some(serde_json::json!({"name": name})),
                    Some(space_uid.as_str()),
                )
                .await?;
                emit_success(&result, &fmt, Some(format!("created pin {name}")));
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation pin.create does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let pin = service
                .create_pin(space_id, &name, "cli", &uuid::Uuid::now_v7().to_string())
                .await?;
            emit_success(&pin, &fmt, Some(format!("created pin {name}")));
        }
        PinSubCmd::List => {
            let target = resolve_command_target(explicit_config, context_override, "pin list")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "pin.list",
                    serde_json::json!({"space_id": space_uid}),
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
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation pin.list does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let pins = service.list_pins(space_id).await?;
            if fmt != Format::Json {
                print_json_table(&pin_rows_table(&pins), &[("NAME", "name")]);
                return Ok(());
            }
            print_json(&pins);
        }
        PinSubCmd::Read { name } => {
            let target = resolve_command_target(explicit_config, context_override, "pin read")?;
            let pins = match &target {
                SpaceTarget::Remote { space_uid, .. } => {
                    http::execute_for_target(
                        &target,
                        "pin.list",
                        serde_json::json!({"space_id": space_uid}),
                        None,
                    )
                    .await?
                }
                SpaceTarget::Core { root, space_id } => {
                    let service = UgoiteService::new_without_background_refresh(root)?;
                    service.list_pins(space_id).await?
                }
            };
            let pin = find_pin(&pins, &name)?;
            let mut value = pin.clone();
            if value.get("name").is_none() {
                value["name"] = serde_json::Value::String(name.clone());
            }
            emit_success(&value, &fmt, None);
        }
        PinSubCmd::Diff { from, to } => {
            if from.trim().is_empty() || to.trim().is_empty() {
                return Err(
                    UsageError("--from and --to pin names must not be blank".to_string()).into(),
                );
            }
            let target = resolve_command_target(explicit_config, context_override, "pin diff")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "space.pin_diff",
                    serde_json::json!({"space_id": space_uid, "from": from, "to": to}),
                    None,
                )
                .await?;
                emit_success(&result, &fmt, None);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation space.pin_diff does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let diff = service.diff_pins(space_id, &from, &to).await?;
            emit_success(&diff, &fmt, None);
        }
        PinSubCmd::Delete { name } => {
            let target = resolve_command_target(explicit_config, context_override, "pin delete")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = step_up::execute_with_step_up_for_target(
                    &target,
                    "pin.delete",
                    serde_json::json!({"space_id": space_uid, "pin_name": name}),
                    None,
                    Some(space_uid.as_str()),
                )
                .await?;
                emit_success(&result, &fmt, Some(format!("deleted pin {name}")));
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation pin.delete does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            service
                .delete_pin(space_id, &name, &uuid::Uuid::now_v7().to_string())
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
