use crate::cli_config::{resolve_command_triple, split_space_and_id};
use crate::http;
use crate::output::print_json;
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;

#[derive(Args)]
pub struct FormCmd {
    #[command(subcommand)]
    pub sub: FormSubCmd,
}

#[derive(Subcommand)]
pub enum FormSubCmd {
    /// List forms
    #[command(
        long_about = "List forms for a space.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or a bare immutable `SPACE_UID`.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite form list\n\n  # Legacy explicit Space (v0.1.x compatibility)\n  ugoite form list /root/spaces/my-space\n  ugoite form list 019f1234-5678-7abc-8def-0123456789ab"
    )]
    List {
        #[arg(
            value_name = "SPACE_UID_OR_PATH",
            help = "Legacy explicit Space (immutable UID or local path). Omit to use the selected context."
        )]
        space_path: Option<String>,
    },
    /// Get a form
    #[command(
        long_about = "Get a form.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or a bare immutable `SPACE_UID`.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite form get Note\n\n  # Legacy explicit Space (v0.1.x compatibility)\n  ugoite form get /root/spaces/my-space Note\n  ugoite form get 019f1234-5678-7abc-8def-0123456789ab Note"
    )]
    Get {
        #[arg(
            value_name = "SPACE_OR_FORM_NAME",
            num_args(1..=2),
            required = true,
            help = "FORM_NAME against the selected context (Form name from the form definition, for example Note or Task), or legacy SPACE FORM_NAME."
        )]
        space_and_name: Vec<String>,
    },
    /// Upsert a form from a JSON file
    #[command(
        long_about = "Upsert a form from a JSON file.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<slug>` path or a bare immutable `SPACE_UID`.\n\nExamples:\n  # Selected context (no Space argument)\n  ugoite form update ./note-form.json\n\n  # Legacy explicit Space (v0.1.x compatibility)\n  ugoite form update /root/spaces/my-space ./note-form.json\n  ugoite form update 019f1234-5678-7abc-8def-0123456789ab ./note-form.json"
    )]
    Update {
        #[arg(
            value_name = "SPACE_OR_FORM_FILE",
            num_args(1..=2),
            required = true,
            help = "FORM_FILE against the selected context (Path to a JSON form definition file), or legacy SPACE FORM_FILE."
        )]
        space_and_file: Vec<String>,
    },
    /// List form column types
    ListTypes,
}

pub async fn run(
    cmd: FormCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        FormSubCmd::List { space_path } => {
            let (root, space_id, base) = resolve_command_triple(
                space_path.as_deref(),
                explicit_config,
                context_override,
                "form list",
            )?;
            if let Some(base) = base {
                let result = http::execute(
                    &base,
                    "form.list",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let forms = service.list_forms(&space_id).await?;
            print_json(&forms);
        }
        FormSubCmd::Get { space_and_name } => {
            let (legacy_space, form_name) =
                split_space_and_id(&space_and_name, "FORM_NAME", "form get")?;
            let form_name = form_name.to_string();
            let (root, space_id, base) = resolve_command_triple(
                legacy_space,
                explicit_config,
                context_override,
                "form get",
            )?;
            if let Some(base) = base {
                let result = http::execute(
                    &base,
                    "form.get",
                    serde_json::json!({"space_id": space_id, "form_name": form_name}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let form = service.get_form(&space_id, &form_name).await?;
            print_json(&form);
        }
        FormSubCmd::Update { space_and_file } => {
            let (legacy_space, form_file) =
                split_space_and_id(&space_and_file, "FORM_FILE", "form update")?;
            let form_file = form_file.to_string();
            let (root, space_id, base) = resolve_command_triple(
                legacy_space,
                explicit_config,
                context_override,
                "form update",
            )?;
            let form_text = std::fs::read_to_string(&form_file)?;
            let form_def: serde_json::Value = serde_json::from_str(&form_text)?;
            if let Some(base) = base {
                let result = http::execute(
                    &base,
                    "form.upsert",
                    serde_json::json!({"space_id": space_id}),
                    Some(form_def),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            service.upsert_form(&space_id, &form_def).await?;
            print_json(&serde_json::json!({"updated": true}));
        }
        FormSubCmd::ListTypes => {
            let types = ugoite_iceberg::form::list_column_types().await?;
            print_json(&types);
        }
    }
    Ok(())
}
