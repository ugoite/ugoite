use crate::cli_config::{resolve_command_target, SpaceTarget};
use crate::config::print_json;
use crate::http;
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_iceberg::service::UgoiteService;
use ugoite_iceberg::{
    index::{
        execute_sql_query_page, sql_session_page_relation, validate_read_only_sql,
        validate_sql_syntax,
    },
    saved_sql::{SqlKind, SqlPayload},
    sql_session::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE},
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
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SavedList,
    /// Get a saved SQL query
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SavedGet {
        #[arg(value_name = "SQL_ID")]
        sql_id: String,
    },
    /// Create a saved SQL query
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SavedCreate {
        #[arg(long)]
        name: String,
        #[arg(long)]
        sql: String,
        #[arg(long)]
        variables: Option<String>,
    },
    /// Update a saved SQL query
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SavedUpdate {
        #[arg(value_name = "SQL_ID")]
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
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SavedDelete {
        #[arg(value_name = "SQL_ID")]
        sql_id: String,
        #[arg(long)]
        human_approval: Option<String>,
    },
    /// Execute a saved SQL query with bounded pagination.
    ///
    /// Reuses the shared read-only admission (`validate_read_only_sql`) and
    /// the shared paged executor. Write/DDL input is rejected pre-execution
    /// with READ_ONLY_SQL_REQUIRED on every transport. Output is stable JSON
    /// with result/count/offset/limit metadata. Local mode carries no
    /// `session_id`; remote mode adds the `session_id` continuation handle.
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SavedExecute {
        #[arg(value_name = "SQL_ID")]
        sql_id: String,
        #[arg(long)]
        offset: Option<usize>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Create a SQL session for a read-only SELECT.
    ///
    /// Core mode stores the session SQL in a CLI-local session directory
    /// under the workspace root and executes through the shared read-only
    /// admission and paged executor. Backend/api mode uses
    /// `sql_session.create`. Write SQL is rejected pre-execution.
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SessionCreate {
        #[arg(long)]
        sql: String,
    },
    /// Get SQL session metadata.
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SessionGet {
        #[arg(value_name = "SESSION_ID")]
        session_id: String,
    },
    /// Get the total row count for a SQL session.
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SessionCount {
        #[arg(value_name = "SESSION_ID")]
        session_id: String,
    },
    /// Read one bounded page of SQL session rows.
    #[command(long_about = "Use the selected context or --context NAME for this command.")]
    SessionRows {
        #[arg(value_name = "SESSION_ID")]
        session_id: String,
        #[arg(long)]
        offset: Option<usize>,
        #[arg(long)]
        limit: Option<usize>,
    },
}

/// Default page size mirrors the server/SQL-session contract.
fn default_sql_limit() -> usize {
    DEFAULT_PAGE_SIZE
}

/// Parse and bound a SQL page request. Fail-closed: no unbounded fetch.
fn parse_sql_page(offset: Option<usize>, limit: Option<usize>) -> Result<(usize, usize)> {
    let offset_value = offset.unwrap_or(0);
    let limit_value = limit.unwrap_or_else(default_sql_limit);
    if limit_value == 0 || limit_value > MAX_PAGE_SIZE {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!("SQL session limit must be between 1 and {MAX_PAGE_SIZE}"),
        )
        .into());
    }
    let page_end = offset_value.checked_add(limit_value).ok_or_else(|| {
        AppError::invalid_input(
            ErrorCode::InvalidInput,
            "SQL session page overflows the addressable window",
        )
    })?;
    if page_end > MAX_PAGE_SIZE {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            format!("SQL session offset+limit must not exceed {MAX_PAGE_SIZE}"),
        )
        .into());
    }
    Ok((offset_value, limit_value))
}

/// Stable row-page envelope shared by core and remote transports.
///
/// Rows-only output: `{rows, count, total_count, offset, limit}`. The v0.1
/// `result` alias was removed; consumers must read `rows`.
fn sql_page_output_with_alias(
    rows: Vec<serde_json::Value>,
    total_count: u64,
    offset: usize,
    limit: usize,
) -> serde_json::Value {
    let count = rows.len() as u64;
    serde_json::json!({
        "rows": rows,
        "count": count,
        "total_count": total_count,
        "offset": offset,
        "limit": limit,
    })
}

/// Strict decoder for a remote `sql_session.rows` payload into the stable
/// envelope inputs.
///
/// The documented server envelope carries `rows` (array), `total_count`,
/// `offset`, and `limit` (integers). A 2xx body that misses those fields or
/// carries the wrong types is protocol drift: fail loudly through the shared
/// remote-response error path instead of synthesizing values, reinterpreting
/// drift as zero, or printing partial success JSON to stdout.
pub(crate) fn decode_remote_rows(
    payload: &serde_json::Value,
    operation: &'static str,
) -> Result<(Vec<serde_json::Value>, u64, usize, usize)> {
    let rows = payload
        .get("rows")
        .and_then(|value| value.as_array())
        .cloned()
        .ok_or_else(|| malformed_remote_response(operation, "expected \"rows\" to be an array"))?;
    let total_count = payload
        .get("total_count")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            malformed_remote_response(operation, "expected \"total_count\" to be an integer")
        })?;
    let offset = decode_remote_page_int(payload, operation, "offset")?;
    let limit = decode_remote_page_int(payload, operation, "limit")?;
    Ok((rows, total_count, offset, limit))
}

fn decode_remote_page_int(
    payload: &serde_json::Value,
    operation: &str,
    field: &str,
) -> Result<usize> {
    let raw = payload
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            malformed_remote_response(operation, &format!("expected \"{field}\" to be an integer"))
        })?;
    usize::try_from(raw).map_err(|_| {
        malformed_remote_response(
            operation,
            &format!("\"{field}\" exceeds the addressable page window"),
        )
    })
}

pub(crate) fn decode_remote_count(
    payload: &serde_json::Value,
    session_id: &str,
) -> Result<serde_json::Value> {
    let count = payload
        .get("count")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            malformed_remote_response("sql_session.count", "expected \"count\" to be an integer")
        })?;
    Ok(serde_json::json!({
        "count": count,
        "total_count": count,
        "session_id": session_id,
    }))
}

/// Fail-closed protocol drift through the existing remote-response error
/// path: `ApiProtocolError` with kind `invalid_response` projects to a
/// stable machine `stderr` envelope, a non-zero exit, and an empty stdout.
fn malformed_remote_response(operation: &str, what: &str) -> anyhow::Error {
    ugoite_api_client::ApiProtocolError {
        kind: "invalid_response".to_string(),
        message: format!("{operation}: malformed remote response: {what}"),
        operation: Some(operation.to_string()),
        status: Some(200),
        detail: None,
        payload: None,
    }
    .into()
}

/// Redacted diagnostics for CLI-local SQL session state I/O.
///
/// Mentions the logical session ID and the file/state role; the chained OS
/// error keeps the root cause. The absolute configured session directory is
/// never interpolated, keeping machine-readable output clean.
fn session_state_context(session_id: &str, action: &str, role: &str) -> String {
    format!("failed to {action} SQL session {session_id} {role}")
}

fn cli_sql_session_dir(root: &str, space_id: &str) -> std::path::PathBuf {
    std::path::Path::new(root)
        .join(".ugoite-cli-sql-sessions")
        .join(space_id)
}

fn validate_local_session_id(session_id: &str) -> Result<()> {
    ugoite_domain::id::validate_sql_session_id(session_id).map_err(|error| {
        AppError::invalid_identifier(format!("invalid SQL session id: {error}")).into()
    })
}

fn local_session_file(root: &str, space_id: &str, session_id: &str) -> Result<std::path::PathBuf> {
    validate_local_session_id(session_id)?;
    Ok(cli_sql_session_dir(root, space_id).join(format!("{session_id}.json")))
}

fn write_local_sql_session(root: &str, space_id: &str, sql: &str) -> Result<serde_json::Value> {
    // Shared admission first: write/DDL/multi-statement input never creates
    // local session state.
    validate_read_only_sql(sql)
        .map_err(|error| anyhow::anyhow!(error).context("read-only SQL is required"))?;
    sql_session_page_relation(sql)
        .map_err(|error| AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()))?;
    let session_id = Uuid::now_v7().to_string();
    let dir = cli_sql_session_dir(root, space_id);
    std::fs::create_dir_all(&dir)
        .with_context(|| session_state_context(&session_id, "create", "state directory"))?;
    let meta = serde_json::json!({
        "id": session_id,
        "space_id": space_id,
        "sql": sql,
        "status": "ready",
        "pagination": {
            "strategy": "offset",
            "default_limit": DEFAULT_PAGE_SIZE,
            "max_limit": MAX_PAGE_SIZE,
            "max_offset": MAX_PAGE_SIZE - 1,
        },
    });
    let path = dir.join(format!("{session_id}.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(&meta)?)
        .with_context(|| session_state_context(&session_id, "persist", "state"))?;
    Ok(meta)
}

fn read_local_sql_session(
    root: &str,
    space_id: &str,
    session_id: &str,
) -> Result<serde_json::Value> {
    let path = local_session_file(root, space_id, session_id)?;
    let bytes = std::fs::read(&path)
        .map_err(|_| AppError::not_found(ErrorCode::EntryNotFound, "SQL session not found"))?;
    let meta: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|_| AppError::not_found(ErrorCode::EntryNotFound, "SQL session not found"))?;
    Ok(meta)
}

fn local_session_sql(meta: &serde_json::Value) -> Result<String> {
    meta.get("sql")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::not_found(ErrorCode::EntryNotFound, "SQL session not found").into()
        })
}

fn saved_sql_text(saved: &serde_json::Value) -> Result<String> {
    saved
        .get("sql")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .ok_or_else(|| {
            AppError::invalid_input(
                ErrorCode::InvalidInput,
                "saved SQL is missing its query text",
            )
            .into()
        })
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
        SqlSubCmd::SavedList => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved-list")?;
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
        SqlSubCmd::SavedGet { sql_id } => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved-get")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "sql.get",
                    serde_json::json!({"space_id": space_uid, "sql_id": sql_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql.get does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let sql = service.get_saved_sql(space_id, &sql_id).await?;
            print_json(&sql);
        }
        SqlSubCmd::SavedCreate {
            name,
            sql,
            variables,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved-create")?;
            let vars: serde_json::Value = variables
                .map(|v| serde_json::from_str(&v))
                .transpose()?
                .unwrap_or(serde_json::json!([]));
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "sql.create",
                    serde_json::json!({"space_id": space_uid}),
                    Some(serde_json::json!({"name": name, "kind": "user-query", "sql": sql, "variables": vars})),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql.create does not use the remote transport")
            };
            let sql_id = Uuid::now_v7().to_string();
            let payload = SqlPayload {
                name: Some(name),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql,
                variables: vars,
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .create_saved_sql(space_id, &sql_id, &payload, "cli")
                .await?;
            print_json(&result);
        }
        SqlSubCmd::SavedUpdate {
            sql_id,
            name,
            sql,
            variables,
            parent_revision_id,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved-update")?;
            let vars: serde_json::Value = variables
                .map(|v| serde_json::from_str(&v))
                .transpose()?
                .unwrap_or(serde_json::json!([]));
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let mut body = serde_json::json!({
                    "name": name,
                    "kind": "user-query",
                    "sql": sql,
                    "variables": vars,
                });
                body["parent_revision_id"] = serde_json::json!(parent_revision_id);
                let result = http::execute_for_target(
                    &target,
                    "sql.update",
                    serde_json::json!({"space_id": space_uid, "sql_id": sql_id}),
                    Some(body),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql.update does not use the remote transport")
            };
            let payload = SqlPayload {
                name: Some(name),
                kind: SqlKind::UserQuery,
                metadata: None,
                sql,
                variables: vars,
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let result = service
                .update_saved_sql(space_id, &sql_id, &payload, &parent_revision_id, "cli")
                .await?;
            print_json(&result);
        }
        SqlSubCmd::SavedDelete {
            sql_id,
            human_approval,
        } => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved-delete")?;
            let human_approval =
                human_approval.or_else(|| std::env::var("UGOITE_HUMAN_APPROVAL").ok());
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "sql.delete",
                    serde_json::json!({"space_id": space_uid, "sql_id": sql_id, "human_approval": human_approval}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            if human_approval.is_some() {
                anyhow::bail!("--human-approval is only supported in backend/api mode");
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql.delete does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            service.delete_saved_sql(space_id, &sql_id, "cli").await?;
            print_json(&serde_json::json!({"deleted": true}));
        }
        SqlSubCmd::SavedExecute {
            sql_id,
            offset,
            limit,
        } => {
            let (offset_value, limit_value) = parse_sql_page(offset, limit)?;
            let target =
                resolve_command_target(explicit_config, context_override, "sql saved-execute")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let saved = http::execute_for_target(
                    &target,
                    "sql.get",
                    serde_json::json!({"space_id": space_uid, "sql_id": sql_id}),
                    None,
                )
                .await?;
                let sql = saved_sql_text(&saved)?;
                validate_read_only_sql(&sql)?;
                let session = http::execute_for_target(
                    &target,
                    "sql_session.create",
                    serde_json::json!({"space_id": space_uid}),
                    Some(serde_json::json!({"sql": sql})),
                )
                .await?;
                let session_id = session
                    .get("id")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| anyhow::anyhow!("SQL session response did not include an id"))?;
                let rows_payload = http::execute_for_target(
                    &target,
                    "sql_session.rows",
                    serde_json::json!({
                        "space_id": space_uid,
                        "session_id": session_id,
                        "offset": offset_value,
                        "limit": limit_value,
                    }),
                    None,
                )
                .await?;
                let (rows, total_count, offset, limit) =
                    decode_remote_rows(&rows_payload, "sql_session.rows")?;
                let mut output = sql_page_output_with_alias(rows, total_count, offset, limit);
                output["sql_id"] = serde_json::json!(sql_id);
                output["session_id"] = serde_json::json!(session_id);
                print_json(&output);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql_session.create does not use the remote transport")
            };
            let service = UgoiteService::new_without_background_refresh(root)?;
            let saved = service.get_saved_sql(space_id, &sql_id).await?;
            let sql = saved_sql_text(&saved)?;
            validate_read_only_sql(&sql)?;
            let (rows, total_count) = execute_sql_query_page(
                service.operator(),
                &service.workspace_path(space_id),
                &sql,
                offset_value,
                limit_value,
            )
            .await?;
            let mut output =
                sql_page_output_with_alias(rows, total_count, offset_value, limit_value);
            output["sql_id"] = serde_json::json!(sql_id);
            print_json(&output);
        }
        SqlSubCmd::SessionCreate { sql } => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql session-create")?;
            // Shared read-only admission before any state creation or network.
            validate_read_only_sql(&sql)?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "sql_session.create",
                    serde_json::json!({"space_id": space_uid}),
                    Some(serde_json::json!({"sql": sql})),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql_session.create does not use the remote transport")
            };
            let meta = write_local_sql_session(root, space_id, &sql)?;
            print_json(&meta);
        }
        SqlSubCmd::SessionGet { session_id } => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql session-get")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "sql_session.get",
                    serde_json::json!({"space_id": space_uid, "session_id": session_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql_session.get does not use the remote transport")
            };
            let meta = read_local_sql_session(root, space_id, &session_id)?;
            print_json(&meta);
        }
        SqlSubCmd::SessionCount { session_id } => {
            let target =
                resolve_command_target(explicit_config, context_override, "sql session-count")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let result = http::execute_for_target(
                    &target,
                    "sql_session.count",
                    serde_json::json!({"space_id": space_uid, "session_id": session_id}),
                    None,
                )
                .await?;
                print_json(&decode_remote_count(&result, &session_id)?);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql_session.count does not use the remote transport")
            };
            let meta = read_local_sql_session(root, space_id, &session_id)?;
            let sql = local_session_sql(&meta)?;
            validate_read_only_sql(&sql)?;
            sql_session_page_relation(&sql).map_err(|error| {
                AppError::invalid_input(ErrorCode::InvalidInput, error.to_string())
            })?;
            let service = UgoiteService::new_without_background_refresh(root)?;
            let (_rows, total_count) = execute_sql_query_page(
                service.operator(),
                &service.workspace_path(space_id),
                &sql,
                0,
                1,
            )
            .await?;
            print_json(&serde_json::json!({
                "count": total_count,
                "total_count": total_count,
                "session_id": session_id,
            }));
        }
        SqlSubCmd::SessionRows {
            session_id,
            offset,
            limit,
        } => {
            let (offset_value, limit_value) = parse_sql_page(offset, limit)?;
            let target =
                resolve_command_target(explicit_config, context_override, "sql session-rows")?;
            if let SpaceTarget::Remote { space_uid, .. } = &target {
                let rows_payload = http::execute_for_target(
                    &target,
                    "sql_session.rows",
                    serde_json::json!({
                        "space_id": space_uid,
                        "session_id": session_id,
                        "offset": offset_value,
                        "limit": limit_value,
                    }),
                    None,
                )
                .await?;
                let (rows, total_count, offset, limit) =
                    decode_remote_rows(&rows_payload, "sql_session.rows")?;
                let mut output = sql_page_output_with_alias(rows, total_count, offset, limit);
                output["session_id"] = serde_json::json!(session_id);
                print_json(&output);
                return Ok(());
            }
            let SpaceTarget::Core { root, space_id } = &target else {
                anyhow::bail!("operation sql_session.rows does not use the remote transport")
            };
            let meta = read_local_sql_session(root, space_id, &session_id)?;
            let sql = local_session_sql(&meta)?;
            validate_read_only_sql(&sql)?;
            sql_session_page_relation(&sql).map_err(|error| {
                AppError::invalid_input(ErrorCode::InvalidInput, error.to_string())
            })?;
            let service = UgoiteService::new_without_background_refresh(root)?;
            let (rows, total_count) = execute_sql_query_page(
                service.operator(),
                &service.workspace_path(space_id),
                &sql,
                offset_value,
                limit_value,
            )
            .await?;
            let mut output =
                sql_page_output_with_alias(rows, total_count, offset_value, limit_value);
            output["session_id"] = serde_json::json!(session_id);
            print_json(&output);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_payload() -> serde_json::Value {
        serde_json::json!({
            "rows": [{"_ugoite_id": "task-a"}],
            "total_count": 3,
            "offset": 1,
            "limit": 1,
        })
    }

    #[test]
    fn remote_rows_decode_accepts_the_documented_envelope() {
        let (rows, total_count, offset, limit) =
            decode_remote_rows(&rows_payload(), "sql_session.rows").expect("valid envelope");
        assert_eq!(rows, vec![serde_json::json!({"_ugoite_id": "task-a"})]);
        assert_eq!(total_count, 3);
        assert_eq!(offset, 1);
        assert_eq!(limit, 1);
    }

    #[test]
    fn remote_rows_decode_rejects_protocol_drift() {
        // Missing total_count must not be synthesized from the row count.
        let missing_total = serde_json::json!({"rows": [], "offset": 0, "limit": 50});
        // A string offset must not fall back to the requested page.
        let string_offset = serde_json::json!({
            "rows": [], "total_count": 0, "offset": "0", "limit": 50,
        });
        // A missing rows array must not become an empty page.
        let missing_rows = serde_json::json!({"total_count": 0, "offset": 0, "limit": 50});
        // The undocumented camelCase alias is drift, not a synonym.
        let camel_total = serde_json::json!({
            "rows": [], "totalCount": 0, "offset": 0, "limit": 50,
        });
        // A missing limit must not fall back to the requested page.
        let missing_limit = serde_json::json!({"rows": [], "total_count": 0, "offset": 0});
        for payload in [
            missing_total,
            string_offset,
            missing_rows,
            camel_total,
            missing_limit,
        ] {
            let error = decode_remote_rows(&payload, "sql_session.rows")
                .expect_err("drift must fail loudly");
            let projected = crate::output::project_error(&error);
            assert_eq!(projected.kind, "invalid_input", "{payload}");
            assert_eq!(projected.exit_code(), 2, "{payload}");
            assert!(
                projected.message.contains("malformed remote response"),
                "{payload}: {}",
                projected.message
            );
        }
    }

    #[test]
    fn remote_count_decode_requires_an_integer_count() {
        let valid = serde_json::json!({"count": 3});
        let decoded = decode_remote_count(&valid, "session-1").expect("valid count envelope");
        assert_eq!(decoded["count"], serde_json::json!(3));
        assert_eq!(decoded["total_count"], serde_json::json!(3));
        assert_eq!(decoded["session_id"], serde_json::json!("session-1"));

        // A missing count must not be reinterpreted as zero.
        for payload in [
            serde_json::json!({}),
            serde_json::json!({"total_count": 3}),
            serde_json::json!({"count": "3"}),
        ] {
            let error =
                decode_remote_count(&payload, "session-1").expect_err("drift must fail loudly");
            let projected = crate::output::project_error(&error);
            assert_eq!(projected.kind, "invalid_input", "{payload}");
            assert_eq!(projected.exit_code(), 2, "{payload}");
        }
    }

    #[test]
    fn session_state_context_mentions_id_and_role_without_paths() {
        let message = session_state_context(
            "019f1234-5678-7abc-8def-0123456789ab",
            "create",
            "state directory",
        );
        assert!(message.contains("019f1234-5678-7abc-8def-0123456789ab"));
        assert!(message.contains("state directory"));
        // NOTE: do not interpolate `message` here: it carries the logical
        // session ID by design (spec requires the ID in diagnostics), and
        // echoing it into a log sink trips cleartext-logging analysis.
        assert!(
            !message.contains('/'),
            "diagnostics must not embed absolute session-dir paths"
        );
    }
}
