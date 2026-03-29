use crate::config::{
    base_url, load_config, operator_for_path, print_json, resolve_space_reference, space_ws_path,
};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_core::integrity::RealIntegrityProvider;
use ugoite_core::saved_sql::SqlPayload;

#[derive(Args)]
pub struct SqlCmd {
    #[command(subcommand)]
    pub sub: SqlSubCmd,
}

#[derive(Subcommand)]
pub enum SqlSubCmd {
    /// Lint SQL text
    Lint { sql_text: String },
    /// List saved SQL queries
    SavedList { space_path: String },
    /// Get a saved SQL query
    SavedGet { space_path: String, sql_id: String },
    /// Create a saved SQL query
    SavedCreate {
        space_path: String,
        sql_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        sql: String,
        #[arg(long)]
        variables: Option<String>,
        #[arg(long, default_value = "cli")]
        author: String,
    },
    /// Update a saved SQL query
    SavedUpdate {
        space_path: String,
        sql_id: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        sql: String,
        #[arg(long)]
        variables: Option<String>,
        #[arg(long)]
        parent_revision_id: Option<String>,
        #[arg(long, default_value = "cli")]
        author: String,
    },
    /// Delete a saved SQL query
    SavedDelete { space_path: String, sql_id: String },
}

pub async fn run(cmd: SqlCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        SqlSubCmd::Lint { sql_text } => {
            let upper = sql_text.trim().to_uppercase();
            let valid = upper.contains("SELECT");
            print_json(&serde_json::json!({"valid": valid, "sql": sql_text}));
        }
        SqlSubCmd::SavedList { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "sql saved-list")?;
            if let Some(base) = base_url(&config) {
                let result = http::http_get(&format!("{base}/spaces/{space_id}/sql")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let sqls = ugoite_core::saved_sql::list_sql(&op, &ws).await?;
            print_json(&sqls);
        }
        SqlSubCmd::SavedGet { space_path, sql_id } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "sql saved-get")?;
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_get(&format!("{base}/spaces/{space_id}/sql/{sql_id}")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let sql = ugoite_core::saved_sql::get_sql(&op, &ws, &sql_id).await?;
            print_json(&sql);
        }
        SqlSubCmd::SavedCreate {
            space_path,
            sql_id,
            name,
            sql,
            variables,
            author,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql saved-create")?;
            let vars: serde_json::Value = variables
                .map(|v| serde_json::from_str(&v).unwrap_or(serde_json::json!([])))
                .unwrap_or(serde_json::json!([]));
            if let Some(base) = base_url(&config) {
                let result = http::http_post(
                    &format!("{base}/spaces/{space_id}/sql"),
                    &serde_json::json!({"id": sql_id, "name": name, "sql": sql, "variables": vars, "author": author}),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let integrity = RealIntegrityProvider::from_space(&op, &space_id).await?;
            let payload = SqlPayload {
                name,
                sql,
                variables: vars,
            };
            let result = ugoite_core::saved_sql::create_sql(
                &op, &ws, &sql_id, &payload, &author, &integrity,
            )
            .await?;
            print_json(&result);
        }
        SqlSubCmd::SavedUpdate {
            space_path,
            sql_id,
            name,
            sql,
            variables,
            parent_revision_id,
            author,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql saved-update")?;
            let vars: serde_json::Value = variables
                .map(|v| serde_json::from_str(&v).unwrap_or(serde_json::json!([])))
                .unwrap_or(serde_json::json!([]));
            if let Some(base) = base_url(&config) {
                let result = http::http_put(
                    &format!("{base}/spaces/{space_id}/sql/{sql_id}"),
                    &serde_json::json!({"name": name, "sql": sql, "variables": vars, "parent_revision_id": parent_revision_id, "author": author}),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            let integrity = RealIntegrityProvider::from_space(&op, &space_id).await?;
            let payload = SqlPayload {
                name,
                sql,
                variables: vars,
            };
            let result = ugoite_core::saved_sql::update_sql(
                &op,
                &ws,
                &sql_id,
                &payload,
                parent_revision_id.as_deref(),
                &author,
                &integrity,
            )
            .await?;
            print_json(&result);
        }
        SqlSubCmd::SavedDelete { space_path, sql_id } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql saved-delete")?;
            if let Some(base) = base_url(&config) {
                let result =
                    http::http_delete(&format!("{base}/spaces/{space_id}/sql/{sql_id}")).await?;
                print_json(&result);
                return Ok(());
            }
            let op = operator_for_path(&root)?;
            let ws = space_ws_path(&root, &space_id);
            ugoite_core::saved_sql::delete_sql(&op, &ws, &sql_id).await?;
            print_json(&serde_json::json!({"deleted": true}));
        }
    }
    Ok(())
}
