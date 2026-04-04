use crate::config::{
    base_url, load_config, operator_for_path, print_json, resolve_space_reference, space_ws_path,
};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_core::integrity::RealIntegrityProvider;

#[derive(Args)]
pub struct FormCmd {
    #[command(subcommand)]
    pub sub: FormSubCmd,
}

#[derive(Subcommand)]
pub enum FormSubCmd {
    /// List forms
    #[command(
        long_about = "List forms for a space.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<id>` path or a bare `SPACE_ID`.\n\nExamples:\n  # Core mode\n  ugoite form list /root/spaces/my-space\n\n  # Backend mode\n  ugoite form list my-space"
    )]
    List {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
    },
    /// Get a form
    #[command(
        long_about = "Get a form.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<id>` path or a bare `SPACE_ID`.\n\nExamples:\n  # Core mode\n  ugoite form get /root/spaces/my-space Note\n\n  # Backend mode\n  ugoite form get my-space Note"
    )]
    Get {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            value_name = "FORM_NAME",
            help = "Form name from the form definition (for example Note or Task)."
        )]
        form_name: String,
    },
    /// Upsert a form from a JSON file
    #[command(
        long_about = "Upsert a form from a JSON file.\n\nRun `ugoite config current` to check whether you should pass a local `/root/spaces/<id>` path or a bare `SPACE_ID`.\n\nExamples:\n  # Core mode\n  ugoite form update /root/spaces/my-space ./note-form.json\n\n  # Backend mode\n  ugoite form update my-space ./note-form.json"
    )]
    Update {
        #[arg(
            value_name = "SPACE_ID_OR_PATH",
            help = "Space ID in backend/api mode, or /root/spaces/<id> in core mode."
        )]
        space_path: String,
        #[arg(
            value_name = "FORM_FILE",
            help = "Path to a JSON form definition file."
        )]
        form_file: String,
        #[arg(long)]
        strategies: Option<String>,
    },
    /// List form column types
    ListTypes,
}

pub async fn run(cmd: FormCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        FormSubCmd::List { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "form list")?;
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/forms")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let forms = ugoite_core::form::list_forms(&op, &ws).await?;
            print_json(&forms);
        }
        FormSubCmd::Get {
            space_path,
            form_name,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "form get")?;
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_get(&format!("{base}/spaces/{space_id}/forms/{form_name}")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let form = ugoite_core::form::get_form(&op, &ws, &form_name).await?;
            print_json(&form);
        }
        FormSubCmd::Update {
            space_path,
            form_file,
            strategies,
        } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "form update")?;
            let form_text = std::fs::read_to_string(&form_file)?;
            let form_def: serde_json::Value = serde_json::from_str(&form_text)?;
            if let Some(base) = base_url(&config) {
                let form_name = form_def
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let result = http::http_put(
                    &format!("{base}/spaces/{space_id}/forms/{form_name}"),
                    &form_def,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            ugoite_core::form::upsert_form(&op, &ws, &form_def).await?;
            if let Some(s) = strategies {
                let strategies_value: serde_json::Value = serde_json::from_str(&s)?;
                if !strategies_value.is_null() {
                    let integrity = RealIntegrityProvider::from_space(&op, &space_id).await?;
                    ugoite_core::form::migrate_form(
                        &op,
                        &ws,
                        &form_def,
                        Some(strategies_value),
                        &integrity,
                    )
                    .await?;
                }
            }
            print_json(&serde_json::json!({"updated": true}));
        }
        FormSubCmd::ListTypes => {
            let types = ugoite_core::form::list_column_types().await?;
            print_json(&types);
        }
    }
    Ok(())
}
