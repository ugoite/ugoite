use crate::config::{
    base_url, load_config, operator_for_path, parse_space_path, print_json, space_ws_path,
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
    List { space_path: String },
    /// Get a form
    Get {
        space_path: String,
        form_name: String,
    },
    /// Upsert a form from a JSON file
    Update {
        space_path: String,
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
            let (root, space_id) = parse_space_path(&space_path);
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
            let (root, space_id) = parse_space_path(&space_path);
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
            let (root, space_id) = parse_space_path(&space_path);
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
                let v: serde_json::Value = serde_json::from_str(&s)?;
                let strats: Vec<String> = v
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                if !strats.is_empty() {
                    let integrity = RealIntegrityProvider::from_space(&op, &space_id).await?;
                    ugoite_core::form::migrate_form(
                        &op,
                        &ws,
                        &form_def,
                        Some(serde_json::json!(strats)),
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
