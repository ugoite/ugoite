use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use crate::output::{effective_format, emit_success, print_json, FormSaveReceipt, Format};
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct FormCmd {
    /// Output format (default: table when TTY, json when piped)
    #[arg(short = 'o', long, value_enum, global = true)]
    pub format: Option<Format>,
    #[command(subcommand)]
    pub sub: FormSubCmd,
}

#[derive(Subcommand)]
pub enum FormSubCmd {
    /// List forms
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    List,
    /// Get a form
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Get {
        #[arg(value_name = "FORM_NAME")]
        form_name: String,
    },
    /// Save a form from a JSON file
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Save {
        #[arg(value_name = "FORM_FILE")]
        form_file: String,
    },
    /// List form column types
    ListTypes,
}

pub async fn run(
    cmd: FormCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    let fmt = effective_format(cmd.format);
    match cmd.sub {
        FormSubCmd::List => {
            let target = resolve_command_target(explicit_config, context_override, "form list")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "form.list",
                    serde_json::json!({"space_id": space_uid}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation form.list does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let forms = service.list_forms(space_id).await?;
            print_json(&forms);
        }
        FormSubCmd::Get { form_name } => {
            let target = resolve_command_target(explicit_config, context_override, "form get")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "form.get",
                    serde_json::json!({"space_id": space_uid, "form_name": form_name}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation form.get does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let form = service.get_form(space_id, &form_name).await?;
            print_json(&form);
        }
        FormSubCmd::Save { form_file } => {
            let target = resolve_command_target(explicit_config, context_override, "form save")?;
            let form_text = std::fs::read_to_string(&form_file)?;
            let form_def: serde_json::Value = serde_json::from_str(&form_text)?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let (result, metadata) = http::execute_form_upsert_for_target(
                    &target,
                    serde_json::json!({"space_id": space_uid}),
                    form_def,
                )
                .await?;
                let name = result["name"].as_str().unwrap_or_default().to_owned();
                let receipt = if let Some(metadata) = metadata {
                    FormSaveReceipt::verified(
                        name.clone(),
                        metadata.form_id,
                        metadata.form_version,
                        metadata.applied,
                        metadata.change_id,
                    )
                } else {
                    FormSaveReceipt::unverified(name.clone())
                };
                let human = render_form_save(&receipt);
                emit_success(&receipt, &fmt, Some(human));
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation form.upsert does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let outcome = service.upsert_form_result(space_id, &form_def).await?;
            let receipt = FormSaveReceipt::verified(
                outcome.form_name.clone(),
                outcome.form_id.to_string(),
                outcome.form_version.get(),
                outcome.applied,
                outcome.change_id,
            );
            emit_success(&receipt, &fmt, Some(render_form_save(&receipt)));
        }
        FormSubCmd::ListTypes => {
            let types = ugoite_iceberg::form::list_column_types().await?;
            print_json(&types);
        }
    }
    Ok(())
}

fn render_form_save(receipt: &FormSaveReceipt) -> String {
    if receipt.identity_unverified {
        return format!(
            "saved form {}; identity unverified by the server",
            receipt.name
        );
    }
    match receipt.applied {
        Some(true) => format!(
            "saved form {} (Form ID {}, version {}, Change {})",
            receipt.name,
            receipt.form_id.as_deref().unwrap_or_default(),
            receipt.form_version.unwrap_or_default(),
            receipt.mutation.change_id.as_deref().unwrap_or_default()
        ),
        Some(false) => format!(
            "form {} is unchanged (Form ID {}, version {})",
            receipt.name,
            receipt.form_id.as_deref().unwrap_or_default(),
            receipt.form_version.unwrap_or_default()
        ),
        None => format!("saved form {}", receipt.name),
    }
}
