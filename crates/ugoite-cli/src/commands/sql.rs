use crate::config::{load_config, print_json, resolve_space_reference, validated_base_url};
use crate::http;
use anyhow::Result;
use clap::{Args, Subcommand};
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::{
    index::validate_sql_syntax,
    saved_sql::{SqlKind, SqlPayload},
};
use uuid::Uuid;

#[derive(Args)]
pub struct SqlCmd {
    #[command(subcommand)]
    pub sub: SqlSubCmd,
}

#[derive(Subcommand)]
pub enum SqlSubCmd {
    /// Validate SQL syntax without executing it
    Lint { sql_text: String },
    /// List saved SQL queries
    SavedList { space_path: String },
    /// Get a saved SQL query
    SavedGet { space_path: String, sql_id: String },
    /// Create a saved SQL query
    SavedCreate {
        space_path: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        sql: String,
        #[arg(long)]
        variables: Option<String>,
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
        parent_revision_id: String,
    },
    /// Delete a saved SQL query
    SavedDelete {
        space_path: String,
        sql_id: String,
        #[arg(long)]
        human_approval: Option<String>,
    },
}

pub async fn run(cmd: SqlCmd) -> Result<()> {
    let config = load_config();
    match cmd.sub {
        SqlSubCmd::Lint { sql_text } => match validate_sql_syntax(&sql_text) {
            Ok(()) => print_json(&serde_json::json!({
                "syntax_valid": true,
                // Keep the existing key as an additive alias for current CLI
                // consumers; new consumers should use the explicit name.
                "valid": true,
                "sql": sql_text,
            })),
            Err(error) => print_json(&serde_json::json!({
                "syntax_valid": false,
                // Keep the existing key as an additive alias for current CLI
                // consumers; new consumers should use the explicit name.
                "valid": false,
                "sql": sql_text,
                "reason": error.to_string(),
            })),
        },
        SqlSubCmd::SavedList { space_path } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "sql saved-list")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql.list",
                    serde_json::json!({"space_id": space_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let sqls = service.list_saved_sql_operator_unscoped(&space_id).await?;
            print_json(&sqls);
        }
        SqlSubCmd::SavedGet { space_path, sql_id } => {
            let (root, space_id) = resolve_space_reference(&config, &space_path, "sql saved-get")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql.get",
                    serde_json::json!({"space_id": space_id, "sql_id": sql_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let sql = service.get_saved_sql(&space_id, &sql_id).await?;
            print_json(&sql);
        }
        SqlSubCmd::SavedCreate {
            space_path,
            name,
            sql,
            variables,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql saved-create")?;
            let vars: serde_json::Value = variables
                .map(|v| serde_json::from_str(&v))
                .transpose()?
                .unwrap_or(serde_json::json!([]));
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql.create",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::json!({"name": name, "kind": "user-query", "sql": sql, "variables": vars})),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let sql_id = Uuid::now_v7().to_string();
            let payload = SqlPayload {
                name: Some(name),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql,
                variables: vars,
            };
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .create_saved_sql(&space_id, &sql_id, &payload, "cli")
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
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql saved-update")?;
            let vars: serde_json::Value = variables
                .map(|v| serde_json::from_str(&v))
                .transpose()?
                .unwrap_or(serde_json::json!([]));
            if let Some(base) = validated_base_url(&config)? {
                let mut body = serde_json::json!({
                    "name": name,
                    "kind": "user-query",
                    "sql": sql,
                    "variables": vars,
                });
                body["parent_revision_id"] = serde_json::json!(parent_revision_id);
                let result = http::execute(
                    &base,
                    "sql.update",
                    serde_json::json!({"space_id": space_id, "sql_id": sql_id}),
                    Some(body),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let payload = SqlPayload {
                name: Some(name),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql,
                variables: vars,
            };
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let result = service
                .update_saved_sql(&space_id, &sql_id, &payload, &parent_revision_id, "cli")
                .await?;
            print_json(&result);
        }
        SqlSubCmd::SavedDelete {
            space_path,
            sql_id,
            human_approval,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql saved-delete")?;
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql.delete",
                    serde_json::json!({"space_id": space_id, "sql_id": sql_id, "human_approval": human_approval}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            if human_approval.is_some() {
                anyhow::bail!("--human-approval is only supported in backend/api mode");
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            service.delete_saved_sql(&space_id, &sql_id, "cli").await?;
            print_json(&serde_json::json!({"deleted": true}));
        }
    }
    Ok(())
}
