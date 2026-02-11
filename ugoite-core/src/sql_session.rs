use anyhow::Result;
use chrono::Utc;
use opendal::Operator;
use serde_json::Value;
use uuid::Uuid;

use crate::index;

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

fn rows_path(ws_path: &str, session_id: &str) -> String {
    format!("{}/rows.json", session_path(ws_path, session_id))
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

pub async fn create_sql_session(op: &Operator, ws_path: &str, sql: &str) -> Result<Value> {
    ensure_sessions_dir(op, ws_path).await?;

    let session_id = Uuid::new_v4().to_string();
    let session_dir = format!("{}/", session_path(ws_path, &session_id));
    op.create_dir(&session_dir).await?;

    let now = Utc::now().to_rfc3339();
    let mut meta = serde_json::json!({
        "id": session_id,
        "sql": sql,
        "status": "running",
        "created_at": now,
        "updated_at": now,
        "progress": {"processed": 0, "total": Value::Null},
        "row_count": Value::Null,
        "error": Value::Null,
    });

    let session_id = meta["id"].as_str().unwrap_or_default().to_string();
    write_json(op, &meta_path(ws_path, &session_id), &meta).await?;

    match index::execute_sql_query(op, ws_path, sql).await {
        Ok(rows) => {
            let total = rows.len() as u64;
            write_json(op, &rows_path(ws_path, &session_id), &Value::Array(rows)).await?;
            let now = Utc::now().to_rfc3339();
            meta["status"] = Value::String("completed".to_string());
            meta["updated_at"] = Value::String(now);
            meta["row_count"] = Value::Number(total.into());
            meta["progress"] = serde_json::json!({"processed": total, "total": total});
            meta["error"] = Value::Null;
            write_json(op, &meta_path(ws_path, &session_id), &meta).await?;
        }
        Err(err) => {
            let now = Utc::now().to_rfc3339();
            meta["status"] = Value::String("failed".to_string());
            meta["updated_at"] = Value::String(now);
            meta["row_count"] = Value::Number(0.into());
            meta["progress"] = serde_json::json!({"processed": 0, "total": 0});
            meta["error"] = Value::String(err.to_string());
            write_json(op, &meta_path(ws_path, &session_id), &meta).await?;
        }
    }

    Ok(meta)
}

pub async fn get_sql_session_status(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
) -> Result<Value> {
    read_json(op, &meta_path(ws_path, session_id)).await
}

pub async fn get_sql_session_count(op: &Operator, ws_path: &str, session_id: &str) -> Result<u64> {
    let meta = get_sql_session_status(op, ws_path, session_id).await?;
    if let Some(count) = meta.get("row_count").and_then(|v| v.as_u64()) {
        return Ok(count);
    }

    let rows_value = read_json(op, &rows_path(ws_path, session_id)).await?;
    let rows = rows_value.as_array().cloned().unwrap_or_default();
    Ok(rows.len() as u64)
}

pub async fn get_sql_session_rows(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let rows_value = read_json(op, &rows_path(ws_path, session_id)).await?;
    let rows = rows_value.as_array().cloned().unwrap_or_default();
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
    let rows_value = read_json(op, &rows_path(ws_path, session_id)).await?;
    Ok(rows_value.as_array().cloned().unwrap_or_default())
}
