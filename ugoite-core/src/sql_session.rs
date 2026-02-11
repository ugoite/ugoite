use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use opendal::Operator;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::index;
use crate::materialized_view;
use crate::saved_sql;

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

pub async fn create_sql_session(op: &Operator, ws_path: &str, sql: &str) -> Result<Value> {
    ensure_sessions_dir(op, ws_path).await?;

    let session_id = Uuid::new_v4().to_string();
    let session_dir = format!("{}/", session_path(ws_path, &session_id));
    op.create_dir(&session_dir).await?;

    let sql_id = match saved_sql::find_sql_id_by_text(op, ws_path, sql).await? {
        Some(existing_id) => existing_id,
        None => Uuid::new_v4().to_string(),
    };

    let view_meta = match materialized_view::read_view_meta(op, ws_path, &sql_id).await {
        Ok(meta) => meta,
        Err(_) => materialized_view::create_or_update_view(op, ws_path, &sql_id, sql).await?,
    };

    let snapshot_id = view_meta
        .get("snapshot_id")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();
    let snapshot_at = view_meta
        .get("updated_at")
        .or_else(|| view_meta.get("created_at"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    let now = Utc::now();
    let expires_at = (now + Duration::minutes(10)).to_rfc3339();
    let created_at = now.to_rfc3339();
    let space_id = space_id_from_ws_path(ws_path);

    let meta = json!({
        "id": session_id,
        "space_id": space_id,
        "sql_id": sql_id,
        "sql": sql,
        "status": "ready",
        "created_at": created_at,
        "expires_at": expires_at,
        "error": Value::Null,
        "view": {
            "sql_id": sql_id,
            "snapshot_id": snapshot_id,
            "snapshot_at": snapshot_at,
            "schema_version": 1,
        },
        "pagination": {
            "strategy": "offset",
            "order_by": ["updated_at", "id"],
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

pub async fn get_sql_session_status(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
) -> Result<Value> {
    load_session_meta(op, ws_path, session_id).await
}

pub async fn get_sql_session_count(op: &Operator, ws_path: &str, session_id: &str) -> Result<u64> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(anyhow!("SQL session expired"));
    }

    let sql = meta
        .get("sql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("SQL session missing sql"))?;
    let rows = index::execute_sql_query(op, ws_path, sql).await?;
    Ok(rows.len() as u64)
}

pub async fn get_sql_session_rows(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(anyhow!("SQL session expired"));
    }
    let sql = meta
        .get("sql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("SQL session missing sql"))?;
    let rows = index::execute_sql_query(op, ws_path, sql).await?;
    let total = rows.len();
    let start = offset.min(total);
    let end = (offset + limit).min(total);
    let slice: Vec<Value> = rows[start..end].to_vec();

    Ok(serde_json::json!({
        "rows": slice,
        "offset": offset,
        "limit": limit,
        "total_count": total,
    }))
}

pub async fn get_sql_session_rows_all(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
) -> Result<Vec<Value>> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(anyhow!("SQL session expired"));
    }
    let sql = meta
        .get("sql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("SQL session missing sql"))?;
    index::execute_sql_query(op, ws_path, sql).await
}
