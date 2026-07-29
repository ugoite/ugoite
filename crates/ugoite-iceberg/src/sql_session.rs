use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use opendal::Operator;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::index;
use crate::saved_sql;
use crate::SpaceCheckpoint;
use ugoite_core::error::{AppError, ErrorCode};

const SESSION_DIR: &str = "sql_sessions";

fn sessions_root(ws_path: &str) -> String {
    format!("{}/{}", ws_path.trim_end_matches('/'), SESSION_DIR)
}

fn session_path(ws_path: &str, session_id: &str) -> String {
    format!(
        "{}/{}/{}",
        ws_path.trim_end_matches('/'),
        SESSION_DIR,
        session_id
    )
}

fn meta_path(ws_path: &str, session_id: &str) -> String {
    format!("{}/meta.json", session_path(ws_path, session_id))
}

async fn ensure_sessions_dir(op: &Operator, ws_path: &str) -> Result<()> {
    let root = format!("{}/", sessions_root(ws_path));
    if !op.exists(&root).await? {
        op.create_dir(&root).await?;
    }
    Ok(())
}

async fn write_json(op: &Operator, path: &str, value: &Value) -> Result<()> {
    op.write(path, serde_json::to_vec_pretty(value)?).await?;
    Ok(())
}

async fn read_json(op: &Operator, path: &str) -> Result<Value> {
    let bytes = op.read(path).await?;
    Ok(serde_json::from_slice(&bytes.to_vec())?)
}

fn space_id_from_ws_path(ws_path: &str) -> String {
    let trimmed = ws_path.trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some((_, space_id)) = trimmed.rsplit_once('/') {
        if !space_id.is_empty() {
            return space_id.to_string();
        }
    }
    trimmed.to_string()
}

fn is_expired(meta: &Value) -> bool {
    let expires_at = match meta.get("expires_at").and_then(|v| v.as_str()) {
        Some(value) => value,
        None => return true,
    };
    match DateTime::parse_from_rfc3339(expires_at) {
        Ok(time) => time.with_timezone(&Utc) <= Utc::now(),
        Err(_) => true,
    }
}

async fn load_session_meta(op: &Operator, ws_path: &str, session_id: &str) -> Result<Value> {
    let mut meta = read_json(op, &meta_path(ws_path, session_id)).await?;
    if is_expired(&meta) {
        meta["status"] = Value::String("expired".to_string());
    }
    Ok(meta)
}

fn session_sql(meta: &Value) -> Result<&str> {
    meta.get("sql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("SQL session missing sql"))
}

async fn execute_session_page(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(AppError::expired(ErrorCode::SqlSessionExpired, "SQL session expired").into());
    }
    if let Some(by_form) = meta.get("readable_entries_by_form") {
        let by_form = serde_json::from_value(by_form.clone())?;
        return index::execute_sql_query_authorized_by_form_page_at_checkpoint(
            op,
            ws_path,
            session_sql(&meta)?,
            &by_form,
            index::AuthorizedSqlSessionPage {
                parameters: session_parameters(&meta)?,
                checkpoint: session_checkpoint(&meta)?,
                offset,
                limit,
            },
        )
        .await;
    }
    index::execute_sql_query_page_with_parameters(
        op,
        ws_path,
        session_sql(&meta)?,
        session_parameters(&meta)?,
        offset,
        limit,
    )
    .await
}

async fn execute_session_page_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    readable_entries_by_form: &std::collections::BTreeMap<
        String,
        std::collections::HashSet<String>,
    >,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(AppError::expired(ErrorCode::SqlSessionExpired, "SQL session expired").into());
    }
    index::execute_sql_query_authorized_by_form_page_at_checkpoint(
        op,
        ws_path,
        session_sql(&meta)?,
        readable_entries_by_form,
        index::AuthorizedSqlSessionPage {
            parameters: session_parameters(&meta)?,
            checkpoint: session_checkpoint(&meta)?,
            offset,
            limit,
        },
    )
    .await
}

fn session_checkpoint(meta: &Value) -> Result<SpaceCheckpoint> {
    serde_json::from_value(
        meta.get("checkpoint")
            .cloned()
            .ok_or_else(|| anyhow!("SQL session is missing its SpaceCheckpoint"))?,
    )
    .map_err(Into::into)
}

fn session_parameters(
    meta: &Value,
) -> Result<std::collections::HashMap<String, datafusion::scalar::ScalarValue>> {
    let values = meta
        .get("parameters")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let types = meta
        .get("parameter_types")
        .cloned()
        .map(serde_json::from_value)
        .transpose()?
        .unwrap_or_default();
    index::datafusion_parameters(&values, &types)
}

pub async fn create_sql_session_authorized_for_principals_by_form(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    readable_entries_by_form: &std::collections::BTreeMap<
        String,
        std::collections::HashSet<String>,
    >,
    principal_ids: &[Uuid],
) -> Result<Value> {
    create_sql_session_authorized_for_principals_by_form_with_parameters(
        op,
        ws_path,
        sql,
        serde_json::Map::new(),
        std::collections::BTreeMap::new(),
        readable_entries_by_form,
        principal_ids,
    )
    .await
}

pub async fn create_sql_session_authorized_for_principals_by_form_with_parameters(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    parameters: serde_json::Map<String, Value>,
    parameter_types: std::collections::BTreeMap<String, String>,
    readable_entries_by_form: &std::collections::BTreeMap<
        String,
        std::collections::HashSet<String>,
    >,
    principal_ids: &[Uuid],
) -> Result<Value> {
    let mut meta =
        create_sql_session_with_parameters(op, ws_path, sql, parameters, parameter_types).await?;
    meta["readable_entries_by_form"] = serde_json::to_value(readable_entries_by_form)?;
    meta["authorized_principal_ids"] = serde_json::to_value(principal_ids)?;
    let session_id = meta
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SQL session id missing"))?;
    write_json(op, &meta_path(ws_path, session_id), &meta).await?;
    Ok(meta)
}

async fn execute_session_page_scoped(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    offset: usize,
    limit: usize,
    readable_forms: &[String],
) -> Result<(Vec<Value>, u64)> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(AppError::expired(ErrorCode::SqlSessionExpired, "SQL session expired").into());
    }
    index::execute_sql_query_scoped_page(
        op,
        ws_path,
        session_sql(&meta)?,
        readable_forms,
        offset,
        limit,
    )
    .await
}

pub async fn create_sql_session(op: &Operator, ws_path: &str, sql: &str) -> Result<Value> {
    ensure_sessions_dir(op, ws_path).await?;

    let session_id = Uuid::new_v4().to_string();
    let session_dir = format!("{}/", session_path(ws_path, &session_id));
    op.create_dir(&session_dir).await?;

    let sql_id = match saved_sql::find_sql_id_by_text(op, ws_path, sql).await? {
        Some(existing_id) => existing_id,
        None => Uuid::new_v4().to_string(),
    };
    let checkpoint = crate::iceberg_store::native_workspace(op, ws_path)
        .await?
        .capture_checkpoint()
        .await?;

    let now = Utc::now();
    let expires_at = (now + Duration::minutes(10)).to_rfc3339();
    let created_at = now.to_rfc3339();
    let space_id = space_id_from_ws_path(ws_path);

    let meta = json!({
        "id": session_id,
        "space_id": space_id,
        "sql_id": sql_id,
        "sql": sql,
        "parameters": {},
        "parameter_types": {},
        "status": "ready",
        "created_at": created_at,
        "expires_at": expires_at,
        "error": Value::Null,
        "checkpoint": checkpoint,
        "pagination": {
            "strategy": "offset",
            "order_by": "sql_order_by_required",
            "default_limit": 50,
            "max_limit": 1000,
        },
        "count": {
            "mode": "on_demand",
            "cached_at": Value::Null,
            "value": Value::Null,
        }
    });

    write_json(op, &meta_path(ws_path, &session_id), &meta).await?;
    Ok(meta)
}

pub async fn create_sql_session_with_parameters(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    parameters: serde_json::Map<String, Value>,
    parameter_types: std::collections::BTreeMap<String, String>,
) -> Result<Value> {
    // Validate at the API/storage boundary. The executor repeats exact
    // placeholder matching after parse so missing or extra values fail closed.
    index::datafusion_parameters(&parameters, &parameter_types)?;
    let mut meta = create_sql_session(op, ws_path, sql).await?;
    meta["parameters"] = Value::Object(parameters);
    meta["parameter_types"] = serde_json::to_value(parameter_types)?;
    let session_id = meta
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("SQL session id missing"))?;
    write_json(op, &meta_path(ws_path, session_id), &meta).await?;
    Ok(meta)
}

pub async fn require_session_principals(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    principal_ids: &[Uuid],
) -> Result<()> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    let stored = meta
        .get("authorized_principal_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::forbidden("SQL session is not bound to an authorized principal"))?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|value| Uuid::parse_str(value).ok())
        .collect::<std::collections::BTreeSet<_>>();
    let requested = principal_ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if stored != requested {
        return Err(
            AppError::forbidden("SQL session belongs to a different principal context").into(),
        );
    }
    Ok(())
}

pub async fn get_sql_session_status(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
) -> Result<Value> {
    load_session_meta(op, ws_path, session_id).await
}

pub async fn get_sql_session_count(op: &Operator, ws_path: &str, session_id: &str) -> Result<u64> {
    let (_, count) = execute_session_page(op, ws_path, session_id, 0, 1).await?;
    Ok(count)
}

pub async fn get_sql_session_count_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    readable_entries_by_form: &std::collections::BTreeMap<
        String,
        std::collections::HashSet<String>,
    >,
) -> Result<u64> {
    let (_, count) = execute_session_page_authorized_by_form(
        op,
        ws_path,
        session_id,
        readable_entries_by_form,
        0,
        1,
    )
    .await?;
    Ok(count)
}

pub async fn get_sql_session_count_scoped(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    readable_forms: &[String],
) -> Result<u64> {
    let (_, count) =
        execute_session_page_scoped(op, ws_path, session_id, 0, 1, readable_forms).await?;
    Ok(count)
}

pub async fn get_sql_session_rows(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let (rows, total) = execute_session_page(op, ws_path, session_id, offset, limit).await?;

    Ok(serde_json::json!({
        "rows": rows,
        "offset": offset,
        "limit": limit,
        "total_count": total,
    }))
}

pub async fn get_sql_session_rows_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    readable_entries_by_form: &std::collections::BTreeMap<
        String,
        std::collections::HashSet<String>,
    >,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let (rows, total) = execute_session_page_authorized_by_form(
        op,
        ws_path,
        session_id,
        readable_entries_by_form,
        offset,
        limit,
    )
    .await?;
    Ok(serde_json::json!({
        "rows": rows,
        "offset": offset,
        "limit": limit,
        "total_count": total,
    }))
}

pub async fn get_sql_session_rows_scoped(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    offset: usize,
    limit: usize,
    readable_forms: &[String],
) -> Result<Value> {
    let (rows, total) =
        execute_session_page_scoped(op, ws_path, session_id, offset, limit, readable_forms).await?;

    Ok(serde_json::json!({
        "rows": rows,
        "offset": offset,
        "limit": limit,
        "total_count": total,
    }))
}
