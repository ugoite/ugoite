use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use opendal::Operator;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use uuid::Uuid;

use crate::index;
use crate::saved_sql;
use crate::SpaceCheckpoint;
use ugoite_core::error::{AppError, ErrorCode};

const SESSION_DIR: &str = "sql_sessions";
const SESSION_LIFETIME: Duration = Duration::minutes(10);
pub const DEFAULT_PAGE_SIZE: usize = 50;
pub const MAX_PAGE_SIZE: usize = index::SQL_SESSION_MAX_ROWS;

pub type ReadableEntriesByForm = BTreeMap<String, HashSet<String>>;

#[derive(Clone, Copy)]
pub struct SqlSessionAuthorization<'a> {
    pub principal_ids: &'a [Uuid],
    pub policy_hash: &'a str,
    pub readable_entries_by_form: &'a ReadableEntriesByForm,
}

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
        write_json(op, &meta_path(ws_path, session_id), &meta).await?;
    }
    Ok(meta)
}

fn session_sql(meta: &Value) -> Result<&str> {
    meta.get("sql")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("SQL session missing sql"))
}

fn session_checkpoint(meta: &Value) -> Result<SpaceCheckpoint> {
    serde_json::from_value(
        meta.get("checkpoint")
            .cloned()
            .ok_or_else(|| anyhow!("SQL session is missing its SpaceCheckpoint"))?,
    )
    .map_err(Into::into)
}

fn session_parameters(meta: &Value) -> Result<HashMap<String, datafusion::scalar::ScalarValue>> {
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

fn session_query_policy(meta: &Value) -> Result<index::SqlSessionQueryPolicy> {
    serde_json::from_value(
        meta.get("query_policy")
            .cloned()
            .ok_or_else(|| anyhow!("SQL session is missing its frozen query policy"))?,
    )
    .map_err(|error| anyhow!("SQL session query policy is invalid: {error}"))
}

/// Reads only session metadata and returns its frozen query policy. Service
/// callers use this before calculating a current policy fingerprint, so a
/// status request never needs to enumerate live Entries.
pub async fn get_session_query_policy(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
) -> Result<index::SqlSessionQueryPolicy> {
    session_query_policy(&load_session_meta(op, ws_path, session_id).await?)
}

fn validate_session_authorization(
    meta: &Value,
    principal_ids: &[Uuid],
    authorization_policy_hash: &str,
) -> Result<()> {
    let stored = meta
        .get("authorized_principal_ids")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::forbidden("SQL session is not bound to an authorized principal"))?
        .iter()
        .map(|value| {
            let value = value.as_str().ok_or_else(|| {
                AppError::forbidden("SQL session authorized principal metadata is malformed")
            })?;
            Uuid::parse_str(value).map_err(|_| {
                AppError::forbidden("SQL session authorized principal metadata is malformed")
            })
        })
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    let requested = principal_ids.iter().copied().collect::<BTreeSet<_>>();
    if stored != requested {
        return Err(
            AppError::forbidden("SQL session belongs to a different principal context").into(),
        );
    }
    if meta
        .get("authorization_policy_hash")
        .and_then(Value::as_str)
        != Some(authorization_policy_hash)
    {
        return Err(AppError::forbidden("SQL session authorization policy has changed").into());
    }
    Ok(())
}

fn session_query_error(error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<AppError>().is_some() {
        error
    } else {
        AppError::invalid_input(ErrorCode::InvalidInput, error.to_string()).into()
    }
}

async fn execute_session_page_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    authorization: SqlSessionAuthorization<'_>,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(AppError::expired(ErrorCode::SqlSessionExpired, "SQL session expired").into());
    }
    validate_session_authorization(
        &meta,
        authorization.principal_ids,
        authorization.policy_hash,
    )?;
    index::execute_sql_query_authorized_by_form_page_at_checkpoint(
        op,
        ws_path,
        session_sql(&meta)?,
        &session_query_policy(&meta)?,
        index::AuthorizedSqlSessionPage {
            parameters: session_parameters(&meta)?,
            checkpoint: session_checkpoint(&meta)?,
            offset,
            limit,
        },
    )
    .await
    .map_err(session_query_error)
}

pub async fn create_sql_session_authorized_for_principals_by_form(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    authorization: SqlSessionAuthorization<'_>,
) -> Result<Value> {
    create_sql_session_authorized_for_principals_by_form_with_parameters(
        op,
        ws_path,
        sql,
        serde_json::Map::new(),
        BTreeMap::new(),
        authorization,
    )
    .await
}

pub async fn create_sql_session_authorized_for_principals_by_form_with_parameters(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    parameters: serde_json::Map<String, Value>,
    parameter_types: BTreeMap<String, String>,
    authorization: SqlSessionAuthorization<'_>,
) -> Result<Value> {
    let bound_parameters = index::datafusion_parameters(&parameters, &parameter_types)?;
    let checkpoint = crate::iceberg_store::native_workspace(op, ws_path)
        .await?
        .capture_checkpoint()
        .await?;
    let query_policy = index::sql_session_query_policy_at_checkpoint(
        op,
        ws_path,
        authorization.readable_entries_by_form,
        &checkpoint,
    )
    .await?;
    create_sql_session_authorized_for_principals_with_frozen_policy(
        op,
        ws_path,
        sql,
        parameters,
        parameter_types,
        authorization,
        bound_parameters,
        checkpoint,
        query_policy,
    )
    .await
}

/// Creates a session from a checkpoint and derived policy that the service
/// already bound to the same authorization read. This is the production path;
/// it never resolves Forms or Entry scope from the live head at later use.
#[allow(clippy::too_many_arguments)]
pub async fn create_sql_session_authorized_for_principals_with_frozen_policy(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    parameters: serde_json::Map<String, Value>,
    parameter_types: BTreeMap<String, String>,
    authorization: SqlSessionAuthorization<'_>,
    bound_parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    checkpoint: SpaceCheckpoint,
    query_policy: index::SqlSessionQueryPolicy,
) -> Result<Value> {
    index::validate_sql_session_query_at_checkpoint(
        op,
        ws_path,
        sql,
        &query_policy,
        bound_parameters,
        checkpoint.clone(),
    )
    .await
    .map_err(session_query_error)?;

    ensure_sessions_dir(op, ws_path).await?;
    let session_id = Uuid::new_v4().to_string();
    let session_dir = format!("{}/", session_path(ws_path, &session_id));
    op.create_dir(&session_dir).await?;

    let sql_id = match saved_sql::find_sql_id_by_text(op, ws_path, sql).await? {
        Some(existing_id) => existing_id,
        None => Uuid::new_v4().to_string(),
    };
    let now = Utc::now();
    let space_id = space_id_from_ws_path(ws_path);
    let authorized_principal_ids = authorization
        .principal_ids
        .iter()
        .map(Uuid::to_string)
        .collect::<BTreeSet<_>>();
    let meta = json!({
        "id": session_id,
        "space_id": space_id,
        "sql_id": sql_id,
        "sql": sql,
        "parameters": parameters,
        "parameter_types": parameter_types,
        "authorized_principal_ids": authorized_principal_ids,
        "authorization_policy_hash": authorization.policy_hash,
        "status": "ready",
        "created_at": now.to_rfc3339(),
        "expires_at": (now + SESSION_LIFETIME).to_rfc3339(),
        "error": Value::Null,
        "checkpoint": checkpoint,
        "query_policy": query_policy,
        "pagination": {
            "strategy": "offset",
            "total_order": "ORDER BY ending with _ugoite_id",
            "default_limit": DEFAULT_PAGE_SIZE,
            "max_limit": MAX_PAGE_SIZE,
            "max_offset": MAX_PAGE_SIZE - 1,
        },
        "limits": {
            "max_rows": index::SQL_SESSION_MAX_ROWS,
            "max_memory_bytes": index::SQL_SESSION_MAX_MEMORY_BYTES,
            "timeout_ms": index::SQL_SESSION_TIMEOUT.as_millis(),
            "max_concurrency": 1,
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

pub async fn require_session_authorization(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    principal_ids: &[Uuid],
    authorization_policy_hash: &str,
) -> Result<()> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    validate_session_authorization(&meta, principal_ids, authorization_policy_hash)
}

pub async fn get_sql_session_status(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
) -> Result<Value> {
    load_session_meta(op, ws_path, session_id).await
}

pub async fn get_sql_session_count_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    authorization: SqlSessionAuthorization<'_>,
) -> Result<u64> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(AppError::expired(ErrorCode::SqlSessionExpired, "SQL session expired").into());
    }
    validate_session_authorization(
        &meta,
        authorization.principal_ids,
        authorization.policy_hash,
    )?;
    index::execute_sql_query_authorized_by_form_count_at_checkpoint(
        op,
        ws_path,
        session_sql(&meta)?,
        &session_query_policy(&meta)?,
        session_parameters(&meta)?,
        session_checkpoint(&meta)?,
    )
    .await
    .map_err(session_query_error)
}

pub async fn get_sql_session_rows_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    authorization: SqlSessionAuthorization<'_>,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let (rows, total) = execute_session_page_authorized_by_form(
        op,
        ws_path,
        session_id,
        authorization,
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
