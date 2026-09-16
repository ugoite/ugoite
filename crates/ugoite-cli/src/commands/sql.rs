use crate::config::{load_config, print_json, resolve_space_reference, validated_base_url};
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
    /// Execute a saved SQL query with bounded pagination.
    ///
    /// Reuses the shared read-only admission (`validate_read_only_sql`) and
    /// the shared paged executor. Write/DDL input is rejected pre-execution
    /// with READ_ONLY_SQL_REQUIRED on every transport. Output is stable JSON
    /// with result/count/offset/limit metadata.
    SavedExecute {
        space_path: String,
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
    SessionCreate {
        space_path: String,
        #[arg(long)]
        sql: String,
    },
    /// Get SQL session metadata.
    SessionGet {
        space_path: String,
        session_id: String,
    },
    /// Get SQL session metadata (explicit alias for session-get).
    SessionMetadata {
        space_path: String,
        session_id: String,
    },
    /// Get the total row count for a SQL session.
    SessionCount {
        space_path: String,
        session_id: String,
    },
    /// Read one bounded page of SQL session rows.
    SessionRows {
        space_path: String,
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
fn sql_page_output_with_alias(
    rows: Vec<serde_json::Value>,
    total_count: u64,
    offset: usize,
    limit: usize,
) -> serde_json::Value {
    let count = rows.len() as u64;
    serde_json::json!({
        "rows": rows.clone(),
        "result": rows,
        "count": count,
        "total_count": total_count,
        "offset": offset,
        "limit": limit,
    })
}

/// Normalize a remote `sql_session.rows` payload into the stable envelope.
fn normalize_remote_rows(
    payload: &serde_json::Value,
    fallback_offset: usize,
    fallback_limit: usize,
) -> Result<serde_json::Value> {
    let rows = payload
        .get("rows")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let total_count = payload
        .get("total_count")
        .or_else(|| payload.get("totalCount"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(rows.len() as u64);
    let offset = payload
        .get("offset")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(fallback_offset);
    let limit = payload
        .get("limit")
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(fallback_limit);
    Ok(sql_page_output_with_alias(rows, total_count, offset, limit))
}

fn normalize_remote_count(payload: &serde_json::Value, session_id: &str) -> serde_json::Value {
    let count = payload
        .get("count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    serde_json::json!({
        "count": count,
        "total_count": count,
        "session_id": session_id,
    })
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
    std::fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create CLI SQL session directory {}",
            dir.display()
        )
    })?;
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
        .with_context(|| format!("failed to persist CLI SQL session {}", path.display()))?;
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

pub async fn run(cmd: SqlCmd) -> Result<()> {
    let config = load_config()?;
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
        SqlSubCmd::SavedExecute {
            space_path,
            sql_id,
            offset,
            limit,
        } => {
            let (offset_value, limit_value) = parse_sql_page(offset, limit)?;
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql saved-execute")?;
            if let Some(base) = validated_base_url(&config)? {
                let saved = http::execute(
                    &base,
                    "sql.get",
                    serde_json::json!({"space_id": space_id, "sql_id": sql_id}),
                    None,
                )
                .await?;
                let sql = saved_sql_text(&saved)?;
                validate_read_only_sql(&sql)?;
                let session = http::execute(
                    &base,
                    "sql_session.create",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::json!({"sql": sql})),
                )
                .await?;
                let session_id = session
                    .get("id")
                    .and_then(|value| value.as_str())
                    .ok_or_else(|| anyhow::anyhow!("SQL session response did not include an id"))?;
                let rows = http::execute(
                    &base,
                    "sql_session.rows",
                    serde_json::json!({
                        "space_id": space_id,
                        "session_id": session_id,
                        "offset": offset_value,
                        "limit": limit_value,
                    }),
                    None,
                )
                .await?;
                let mut output = normalize_remote_rows(&rows, offset_value, limit_value)?;
                output["sql_id"] = serde_json::json!(sql_id);
                output["session_id"] = serde_json::json!(session_id);
                print_json(&output);
                return Ok(());
            }
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let saved = service.get_saved_sql(&space_id, &sql_id).await?;
            let sql = saved_sql_text(&saved)?;
            validate_read_only_sql(&sql)?;
            let (rows, total_count) = execute_sql_query_page(
                service.operator(),
                &service.workspace_path(&space_id),
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
        SqlSubCmd::SessionCreate { space_path, sql } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql session-create")?;
            // Shared read-only admission before any state creation or network.
            validate_read_only_sql(&sql)?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql_session.create",
                    serde_json::json!({"space_id": space_id}),
                    Some(serde_json::json!({"sql": sql})),
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let meta = write_local_sql_session(&root, &space_id, &sql)?;
            print_json(&meta);
        }
        SqlSubCmd::SessionGet {
            space_path,
            session_id,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql session-get")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql_session.get",
                    serde_json::json!({"space_id": space_id, "session_id": session_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let meta = read_local_sql_session(&root, &space_id, &session_id)?;
            print_json(&meta);
        }
        SqlSubCmd::SessionMetadata {
            space_path,
            session_id,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql session-metadata")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql_session.get",
                    serde_json::json!({"space_id": space_id, "session_id": session_id}),
                    None,
                )
                .await?;
                print_json(&result);
                return Ok(());
            }
            let meta = read_local_sql_session(&root, &space_id, &session_id)?;
            print_json(&meta);
        }
        SqlSubCmd::SessionCount {
            space_path,
            session_id,
        } => {
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql session-count")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql_session.count",
                    serde_json::json!({"space_id": space_id, "session_id": session_id}),
                    None,
                )
                .await?;
                print_json(&normalize_remote_count(&result, &session_id));
                return Ok(());
            }
            let meta = read_local_sql_session(&root, &space_id, &session_id)?;
            let sql = local_session_sql(&meta)?;
            validate_read_only_sql(&sql)?;
            sql_session_page_relation(&sql).map_err(|error| {
                AppError::invalid_input(ErrorCode::InvalidInput, error.to_string())
            })?;
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let (_rows, total_count) = execute_sql_query_page(
                service.operator(),
                &service.workspace_path(&space_id),
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
            space_path,
            session_id,
            offset,
            limit,
        } => {
            let (offset_value, limit_value) = parse_sql_page(offset, limit)?;
            let (root, space_id) =
                resolve_space_reference(&config, &space_path, "sql session-rows")?;
            if let Some(base) = validated_base_url(&config)? {
                let result = http::execute(
                    &base,
                    "sql_session.rows",
                    serde_json::json!({
                        "space_id": space_id,
                        "session_id": session_id,
                        "offset": offset_value,
                        "limit": limit_value,
                    }),
                    None,
                )
                .await?;
                let mut output = normalize_remote_rows(&result, offset_value, limit_value)?;
                output["session_id"] = serde_json::json!(session_id);
                print_json(&output);
                return Ok(());
            }
            let meta = read_local_sql_session(&root, &space_id, &session_id)?;
            let sql = local_session_sql(&meta)?;
            validate_read_only_sql(&sql)?;
            sql_session_page_relation(&sql).map_err(|error| {
                AppError::invalid_input(ErrorCode::InvalidInput, error.to_string())
            })?;
            let service = UgoiteService::new_without_background_refresh(&root)?;
            let (rows, total_count) = execute_sql_query_page(
                service.operator(),
                &service.workspace_path(&space_id),
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
