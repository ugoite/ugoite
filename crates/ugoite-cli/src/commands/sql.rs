use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::http;
use crate::output::{
    effective_format, emit_mutation, print_json, print_json_table, Format, MutationReceipt,
    UsageError,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use ugoite_core::sql_query::{SqlQueryCountRequest, SqlQueryPage, SqlQueryRequest};
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::{
    index::validate_sql_syntax,
    saved_sql::{SqlGeneratedName, SqlKind, SqlMetadata, SqlPayload},
};

#[derive(Args)]
pub struct SqlCmd {
    #[command(subcommand)]
    pub sub: SqlSubCmd,
}

#[derive(Subcommand)]
pub enum SqlSubCmd {
    /// Validate SQL syntax without executing it
    Lint { sql_text: String },
    /// Execute one stateless, read-only SQL page
    Query {
        #[arg(value_name = "SQL_OR_FILE")]
        sql_text: String,
        #[arg(long = "param", value_name = "NAME=VALUE")]
        parameters: Vec<String>,
        #[arg(long = "param-type", value_name = "NAME=TYPE")]
        parameter_types: Vec<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
        #[arg(long)]
        continuation: Option<String>,
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
    },
    /// Count rows for one explicit, read-only SQL query
    Count {
        #[arg(value_name = "SQL_OR_FILE")]
        sql_text: String,
        #[arg(long = "param", value_name = "NAME=VALUE")]
        parameters: Vec<String>,
        #[arg(long = "param-type", value_name = "NAME=TYPE")]
        parameter_types: Vec<String>,
    },
    /// Manage durable saved SQL queries
    #[command(subcommand)]
    Saved(SavedSqlSubCmd),
}

#[derive(Subcommand)]
pub enum SavedSqlSubCmd {
    /// List saved SQL queries
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    List,
    /// Get a saved SQL query
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Get {
        #[arg(value_name = "SQL_ID")]
        sql_id: String,
    },
    /// Create a saved SQL query
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Create {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_name = "SQL_OR_FILE")]
        sql: String,
        #[arg(long)]
        variables: Option<String>,
    },
    /// Update a saved SQL query
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Update {
        #[arg(value_name = "SQL_ID")]
        sql_id: String,
        #[arg(long, conflicts_with = "untitled")]
        name: Option<String>,
        #[arg(long)]
        untitled: bool,
        #[arg(long, value_name = "SQL_OR_FILE")]
        sql: Option<String>,
        #[arg(long)]
        variables: Option<String>,
        #[arg(long)]
        parent_revision_id: Option<String>,
    },
    /// Delete a saved SQL query
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    Delete {
        #[arg(value_name = "SQL_ID")]
        sql_id: String,
        #[arg(long)]
        human_approval: Option<String>,
    },
}

fn sql_text_from_argument(argument: &str) -> Result<String> {
    let path = std::path::Path::new(argument);
    if path.is_file() {
        return std::fs::read_to_string(path)
            .with_context(|| format!("failed to read SQL file {}", path.display()));
    }
    Ok(argument.to_string())
}

fn parse_sql_bindings(
    raw_parameters: &[String],
    raw_types: &[String],
) -> Result<(
    serde_json::Map<String, serde_json::Value>,
    std::collections::BTreeMap<String, String>,
)> {
    let mut parameters = serde_json::Map::new();
    for raw in raw_parameters {
        let (name, value) = raw
            .split_once('=')
            .ok_or_else(|| UsageError(format!("--param must be NAME=VALUE, got {raw:?}")))?;
        if name.trim().is_empty() {
            return Err(UsageError("SQL parameter name must not be empty".to_string()).into());
        }
        let value = serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.to_string()));
        if parameters.insert(name.to_string(), value).is_some() {
            return Err(UsageError(format!(
                "SQL parameter {name:?} was provided more than once"
            ))
            .into());
        }
    }
    let mut parameter_types = std::collections::BTreeMap::new();
    for raw in raw_types {
        let (name, kind) = raw
            .split_once('=')
            .ok_or_else(|| UsageError(format!("--param-type must be NAME=TYPE, got {raw:?}")))?;
        if name.trim().is_empty() || kind.trim().is_empty() {
            return Err(UsageError("SQL parameter type requires NAME=TYPE".to_string()).into());
        }
        if parameter_types
            .insert(name.to_string(), kind.to_string())
            .is_some()
        {
            return Err(UsageError(format!(
                "SQL parameter type {name:?} was provided more than once"
            ))
            .into());
        }
    }
    for (name, value) in &parameters {
        if parameter_types.contains_key(name) {
            continue;
        }
        let kind = match value {
            serde_json::Value::String(_) => "string",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(number) if number.is_i64() || number.is_u64() => "long",
            serde_json::Value::Number(_) => "double",
            serde_json::Value::Null => {
                return Err(UsageError(format!(
                    "SQL null parameter {name:?} requires --param-type"
                ))
                .into())
            }
            serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
                return Err(UsageError(format!(
                    "SQL parameter {name:?} must be a scalar JSON value"
                ))
                .into())
            }
        };
        parameter_types.insert(name.clone(), kind.to_string());
    }
    Ok((parameters, parameter_types))
}

fn opt_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn target_space_id(target: &SpaceTarget) -> &str {
    match target {
        SpaceTarget::Core { space_id, .. }
        | SpaceTarget::Remote {
            space_uid: space_id,
            ..
        } => space_id,
    }
}

async fn get_saved_sql(target: &SpaceTarget, sql_id: &str) -> Result<serde_json::Value> {
    let space_id = target_space_id(target);
    match target {
        SpaceTarget::Remote { .. } => {
            let result = http::execute_for_target(
                target,
                "sql.get",
                serde_json::json!({"space_id": space_id, "sql_id": sql_id}),
                None,
            )
            .await?;
            Ok(result)
        }
        SpaceTarget::Core { root, .. } => {
            UgoiteService::new_without_background_refresh(root)?
                .get_saved_sql(space_id, sql_id)
                .await
        }
    }
}

async fn query_sql_page(target: &SpaceTarget, request: SqlQueryRequest) -> Result<SqlQueryPage> {
    let space_id = target_space_id(target);
    match target {
        SpaceTarget::Remote { .. } => {
            let payload = http::execute_for_target(
                target,
                "sql.query",
                serde_json::json!({"space_id": space_id}),
                Some(serde_json::to_value(request)?),
            )
            .await?;
            serde_json::from_value(payload).context("sql.query returned an invalid page")
        }
        SpaceTarget::Core { root, .. } => {
            UgoiteService::new_without_background_refresh(root)?
                .query_sql(space_id, request)
                .await
        }
    }
}

async fn query_sql_count(
    target: &SpaceTarget,
    request: SqlQueryCountRequest,
) -> Result<serde_json::Value> {
    let space_id = target_space_id(target);
    match target {
        SpaceTarget::Remote { .. } => {
            http::execute_for_target(
                target,
                "sql.query.count",
                serde_json::json!({"space_id": space_id}),
                Some(serde_json::to_value(request)?),
            )
            .await
        }
        SpaceTarget::Core { root, .. } => {
            let count = UgoiteService::new_without_background_refresh(root)?
                .count_sql(space_id, request)
                .await?;
            Ok(serde_json::json!({"count": count}))
        }
    }
}

fn print_sql_page(page: &SqlQueryPage, format: &Format) -> Result<()> {
    match format {
        Format::Json | Format::Plain => print_json(page),
        Format::Ndjson => {
            for row in &page.rows {
                println!("{}", serde_json::to_string(row)?);
            }
            if page.next.is_some() {
                eprintln!("more results available; use --format json to retrieve the continuation");
            }
        }
        Format::Table => {
            let columns = page
                .columns
                .iter()
                .map(|column| (column.as_str(), column.as_str()))
                .collect::<Vec<_>>();
            print_json_table(&page.rows, &columns);
            if page.next.is_some() {
                eprintln!("more results available; use --format json to retrieve the continuation");
            }
        }
    }
    Ok(())
}

pub async fn run(
    cmd: SqlCmd,
    explicit_config: Option<&std::path::Path>,
    context_override: Option<&str>,
) -> Result<()> {
    match cmd.sub {
        SqlSubCmd::Lint { sql_text } => match validate_sql_syntax(&sql_text) {
            Ok(()) => print_json(&serde_json::json!({
                "syntax_valid": true,
                "sql": sql_text,
            })),
            Err(error) => print_json(&serde_json::json!({
                "syntax_valid": false,
                "sql": sql_text,
                "reason": error.to_string(),
            })),
        },
        SqlSubCmd::Query {
            sql_text,
            parameters,
            parameter_types,
            limit,
            continuation,
            format,
        } => {
            let sql = sql_text_from_argument(&sql_text)?;
            let (parameters, parameter_types) = parse_sql_bindings(&parameters, &parameter_types)?;
            let target = resolve_command_target(explicit_config, context_override, "sql query")?;
            let page = query_sql_page(
                &target,
                SqlQueryRequest {
                    sql,
                    parameters,
                    parameter_types,
                    limit,
                    continuation,
                },
            )
            .await?;
            print_sql_page(&page, &format)?;
        }
        SqlSubCmd::Count {
            sql_text,
            parameters,
            parameter_types,
        } => {
            let sql = sql_text_from_argument(&sql_text)?;
            let (parameters, parameter_types) = parse_sql_bindings(&parameters, &parameter_types)?;
            let target =
                resolve_command_target(explicit_config, context_override, "sql query count")?;
            let result = query_sql_count(
                &target,
                SqlQueryCountRequest {
                    sql,
                    parameters,
                    parameter_types,
                },
            )
            .await?;
            print_json(&result);
        }
        SqlSubCmd::Saved(SavedSqlSubCmd::List) => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved list")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "sql.list",
                    serde_json::json!({"space_id": space_uid}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql.list does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let sqls = service.list_saved_sql_operator_unscoped(space_id).await?;
            print_json(&sqls);
        }
        SqlSubCmd::Saved(SavedSqlSubCmd::Get { sql_id }) => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved get")?;
            print_json(&get_saved_sql(&target, &sql_id).await?);
        }
        SqlSubCmd::Saved(SavedSqlSubCmd::Create {
            name,
            sql,
            variables,
        }) => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved create")?;
            let fmt = effective_format(None);
            let vars: serde_json::Value = variables
                .map(|v| serde_json::from_str(&v))
                .transpose()?
                .unwrap_or(serde_json::json!([]));
            let payload = SqlPayload {
                metadata: if name.is_none() {
                    Some(SqlMetadata {
                        search_criteria: None,
                        generated_name: Some(SqlGeneratedName::Untitled),
                    })
                } else {
                    None
                },
                name,
                kind: SqlKind::UserQuery,
                sql: sql_text_from_argument(&sql)?,
                variables: vars,
            };
            let result = match &target {
                SpaceTarget::Remote { space_uid, .. } => {
                    http::execute_for_target(
                        &target,
                        "sql.create",
                        serde_json::json!({"space_id": space_uid}),
                        Some(serde_json::to_value(payload)?),
                    )
                    .await?
                }
                SpaceTarget::Core { root, space_id } => {
                    UgoiteService::new_without_background_refresh(root)?
                        .create_saved_sql(space_id, None, &payload, "cli")
                        .await?
                }
            };
            let receipt = MutationReceipt::sql(
                opt_str(&result, "id").context("sql.create returned no id")?,
                opt_str(&result, "revision_id"),
                opt_str(&result, "change_id"),
            );
            emit_mutation(&receipt, &fmt, None);
        }
        SqlSubCmd::Saved(SavedSqlSubCmd::Update {
            sql_id,
            name,
            untitled,
            sql,
            variables,
            parent_revision_id,
        }) => {
            if name.is_some() && untitled {
                return Err(
                    UsageError("--name and --untitled are mutually exclusive".into()).into(),
                );
            }
            if name.is_none() && !untitled && sql.is_none() && variables.is_none() {
                return Err(UsageError(
                    "provide at least one of --name, --untitled, --sql, or --variables".into(),
                )
                .into());
            }

            let target =
                resolve_command_target(explicit_config, context_override, "sql saved update")?;
            let current = get_saved_sql(&target, &sql_id).await?;
            let current_revision_id = current
                .get("revision_id")
                .and_then(serde_json::Value::as_str)
                .context("sql.get returned no revision_id")?;
            let current_name = current
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let current_metadata = if current
                .get("metadata")
                .is_some_and(|value| !value.is_null())
            {
                Some(serde_json::from_value::<SqlMetadata>(
                    current["metadata"].clone(),
                )?)
            } else {
                None
            };
            let rename_requested = name.is_some();
            let merged_name = if untitled {
                None
            } else {
                name.or(current_name)
            };
            let metadata = if untitled {
                Some(SqlMetadata {
                    search_criteria: None,
                    generated_name: Some(SqlGeneratedName::Untitled),
                })
            } else if rename_requested {
                None
            } else {
                current_metadata
            };
            let merged_sql = sql
                .map(|value| sql_text_from_argument(&value))
                .transpose()?
                .or_else(|| {
                    current
                        .get("sql")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                })
                .context("sql.get returned no SQL text")?;
            let merged_variables = variables
                .map(|value| serde_json::from_str(&value))
                .transpose()?
                .or_else(|| current.get("variables").cloned())
                .context("sql.get returned no variables")?;
            let payload = SqlPayload {
                name: merged_name,
                kind: SqlKind::UserQuery,
                metadata,
                sql: merged_sql,
                variables: merged_variables,
            };
            let revision_id = parent_revision_id.unwrap_or_else(|| current_revision_id.to_owned());
            let fmt = effective_format(None);
            let result = match &target {
                SpaceTarget::Remote { space_uid, .. } => {
                    let mut body = serde_json::to_value(payload)?;
                    body["parent_revision_id"] = serde_json::json!(revision_id);
                    http::execute_for_target(
                        &target,
                        "sql.update",
                        serde_json::json!({"space_id": space_uid, "sql_id": sql_id}),
                        Some(body),
                    )
                    .await?
                }
                SpaceTarget::Core { root, space_id } => {
                    UgoiteService::new_without_background_refresh(root)?
                        .update_saved_sql(space_id, &sql_id, &payload, &revision_id, "cli")
                        .await?
                }
            };
            let receipt = MutationReceipt::sql(
                sql_id,
                opt_str(&result, "revision_id"),
                opt_str(&result, "change_id"),
            );
            emit_mutation(&receipt, &fmt, None);
        }
        SqlSubCmd::Saved(SavedSqlSubCmd::Delete {
            sql_id,
            human_approval,
        }) => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved delete")?;
            let fmt = effective_format(None);
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let _result = http::execute_for_target(
                    &target,
                    "sql.delete",
                    serde_json::json!({"space_id": space_uid, "sql_id": sql_id, "human_approval": human_approval}),
                    None,
                )
                .await?;
                let receipt = MutationReceipt::sql(sql_id.clone(), None, None);
                emit_mutation(&receipt, &fmt, Some(format!("deleted sql {sql_id}")));
                return Ok(());
            }
            if human_approval.is_some() {
                anyhow::bail!(
                    "--human-approval is only supported on a remote backend/api connection"
                );
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql.delete does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            service.delete_saved_sql(space_id, &sql_id, "cli").await?;
            let receipt = MutationReceipt::sql(sql_id.clone(), None, None);
            emit_mutation(&receipt, &fmt, Some(format!("deleted sql {sql_id}")));
        }
    }
    Ok(())
}
