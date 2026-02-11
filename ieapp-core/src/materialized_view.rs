use anyhow::Result;
use chrono::Utc;
use opendal::Operator;
use serde_json::Value;

const VIEW_DIR: &str = "materialized_views";

fn views_root(ws_path: &str) -> String {
    format!("{}/{}", ws_path.trim_end_matches('/'), VIEW_DIR)
}

fn view_path(ws_path: &str, sql_id: &str) -> String {
    format!("{}/{}/{}", ws_path.trim_end_matches('/'), VIEW_DIR, sql_id)
}

fn meta_path(ws_path: &str, sql_id: &str) -> String {
    format!("{}/meta.json", view_path(ws_path, sql_id))
}

async fn ensure_views_dir(op: &Operator, ws_path: &str) -> Result<()> {
    let root = format!("{}/", views_root(ws_path));
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

pub async fn create_or_update_view(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    sql: &str,
) -> Result<Value> {
    ensure_views_dir(op, ws_path).await?;

    let view_dir = format!("{}/", view_path(ws_path, sql_id));
    if !op.exists(&view_dir).await? {
        op.create_dir(&view_dir).await?;
    }

    let now = Utc::now().to_rfc3339();
    let snapshot_id = Utc::now().timestamp_millis() as u64;
    let created_at = if op.exists(&meta_path(ws_path, sql_id)).await? {
        read_json(op, &meta_path(ws_path, sql_id))
            .await?
            .get("created_at")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string())
            .unwrap_or_else(|| now.clone())
    } else {
        now.clone()
    };

    let meta = serde_json::json!({
        "sql_id": sql_id,
        "created_at": created_at,
        "updated_at": now,
        "snapshot_id": snapshot_id,
        "sql": sql,
    });

    write_json(op, &meta_path(ws_path, sql_id), &meta).await?;
    Ok(meta)
}

pub async fn delete_view(op: &Operator, ws_path: &str, sql_id: &str) -> Result<()> {
    let dir = format!("{}/", view_path(ws_path, sql_id));
    if op.exists(&dir).await? {
        let _ = op.remove_all(&dir).await;
    }
    Ok(())
}

pub async fn read_view_meta(op: &Operator, ws_path: &str, sql_id: &str) -> Result<Value> {
    read_json(op, &meta_path(ws_path, sql_id)).await
}
