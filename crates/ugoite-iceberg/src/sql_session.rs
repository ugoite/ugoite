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
use ugoite_core::query::EntryScope;

const SESSION_DIR: &str = "sql_sessions";
const SESSION_LIFETIME: Duration = Duration::minutes(10);
pub const DEFAULT_PAGE_SIZE: usize = 50;
pub const MAX_PAGE_SIZE: usize = index::SQL_SESSION_MAX_ROWS;

pub type ReadableEntriesByForm = BTreeMap<String, HashSet<String>>;

#[derive(Clone, Copy)]
pub struct SqlSessionAuthorization<'a> {
    pub principal_ids: &'a [Uuid],
    pub policy_hash: &'a str,
}

/// Use-time authorization inputs. The query policy must be rebuilt from the
/// immutable checkpoint and current authorization state by the caller; the
/// persisted policy is compared with it only as derived metadata.
#[derive(Clone, Copy)]
pub struct SqlSessionExecutionAuthorization<'a> {
    pub authorization: SqlSessionAuthorization<'a>,
    pub query_policy: &'a index::SqlSessionQueryPolicy,
}

/// Creation-only authorization inputs. The public convenience constructor
/// accepts an explicit readable-ID map for callers that already have one; the
/// production service instead supplies a frozen sparse scope directly.
#[derive(Clone, Copy)]
pub struct SqlSessionCreateAuthorization<'a> {
    pub authorization: SqlSessionAuthorization<'a>,
    pub readable_entries_by_form: &'a ReadableEntriesByForm,
}

impl SqlSessionAuthorization<'_> {
    fn require_principals(self) -> Result<()> {
        if self.principal_ids.is_empty() {
            return Err(AppError::forbidden(
                "SQL session requires at least one authorized principal",
            )
            .into());
        }
        Ok(())
    }
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
        crate::authorization::Authorizer::new(op.clone())
            .ensure_authoritative_mutation_contract()?;
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

/// The durable inputs used to recreate an expected execution policy.
#[derive(Clone)]
pub struct SqlSessionExecutionInputs {
    pub sql: String,
    pub checkpoint: SpaceCheckpoint,
}

/// Reads the durable query coordinate, not the stored derived policy.
pub async fn get_session_execution_inputs(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
) -> Result<SqlSessionExecutionInputs> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    Ok(SqlSessionExecutionInputs {
        sql: session_sql(&meta)?.to_string(),
        checkpoint: session_checkpoint(&meta)?,
    })
}

fn validate_session_authorization(
    meta: &Value,
    principal_ids: &[Uuid],
    authorization_policy_hash: &str,
) -> Result<()> {
    if principal_ids.is_empty() {
        return Err(
            AppError::forbidden("SQL session requires at least one authorized principal").into(),
        );
    }
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
    if stored.is_empty() {
        return Err(
            AppError::forbidden("SQL session is not bound to an authorized principal").into(),
        );
    }
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

fn validate_session_execution_authorization(
    meta: &Value,
    authorization: SqlSessionExecutionAuthorization<'_>,
) -> Result<()> {
    validate_session_authorization(
        meta,
        authorization.authorization.principal_ids,
        authorization.authorization.policy_hash,
    )?;
    if session_query_policy(meta)? != *authorization.query_policy {
        return Err(AppError::forbidden("SQL session query policy has changed").into());
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
    authorization: SqlSessionExecutionAuthorization<'_>,
    offset: usize,
    limit: usize,
) -> Result<(Vec<Value>, u64)> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(AppError::expired(ErrorCode::SqlSessionExpired, "SQL session expired").into());
    }
    validate_session_execution_authorization(&meta, authorization)?;
    index::execute_sql_query_authorized_by_form_page_at_checkpoint(
        op,
        ws_path,
        session_sql(&meta)?,
        authorization.query_policy,
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
    authorization: SqlSessionCreateAuthorization<'_>,
    saved_sql_entry_scope: EntryScope,
) -> Result<Value> {
    create_sql_session_authorized_for_principals_by_form_with_parameters(
        op,
        ws_path,
        sql,
        serde_json::Map::new(),
        BTreeMap::new(),
        authorization,
        saved_sql_entry_scope,
    )
    .await
}

pub async fn create_sql_session_authorized_for_principals_by_form_with_parameters(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    parameters: serde_json::Map<String, Value>,
    parameter_types: BTreeMap<String, String>,
    authorization: SqlSessionCreateAuthorization<'_>,
    saved_sql_entry_scope: EntryScope,
) -> Result<Value> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    authorization.authorization.require_principals()?;
    let relation = index::sql_session_page_relation(sql).map_err(session_query_error)?;
    let readable_entry_ids = authorization
        .readable_entries_by_form
        .get(&relation)
        .ok_or_else(|| anyhow!("SQL session has no readable checkpoint Form {relation}"))?;
    if readable_entry_ids.len() > index::SQL_SESSION_MAX_AUTHORIZATION_SCOPE_IDS {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "SQL session authorization scope exceeds the configured maximum",
        )
        .into());
    }
    let bound_parameters = index::datafusion_parameters(&parameters, &parameter_types)?;
    let checkpoint = crate::iceberg_store::native_workspace(op, ws_path)
        .await?
        .capture_checkpoint()
        .await?;
    let query_policy = index::sql_session_query_policy_at_checkpoint(
        op,
        ws_path,
        &relation,
        index::SqlSessionEntryScope::Only(readable_entry_ids.iter().cloned().collect()),
        &checkpoint,
    )
    .await?;
    create_sql_session_authorized_for_principals_with_frozen_policy_and_saved_sql_scope(
        op,
        ws_path,
        sql,
        parameters,
        parameter_types,
        authorization.authorization,
        bound_parameters,
        checkpoint,
        query_policy,
        &saved_sql_entry_scope,
    )
    .await
}

/// Creates a session with the Saved SQL ACL already reduced to a provider
/// EntryScope. The scope is applied to the SQL Form before its payload is
/// decoded or used to populate session metadata.
#[allow(clippy::too_many_arguments)]
pub async fn create_sql_session_authorized_for_principals_with_frozen_policy_and_saved_sql_scope(
    op: &Operator,
    ws_path: &str,
    sql: &str,
    parameters: serde_json::Map<String, Value>,
    parameter_types: BTreeMap<String, String>,
    authorization: SqlSessionAuthorization<'_>,
    bound_parameters: HashMap<String, datafusion::scalar::ScalarValue>,
    checkpoint: SpaceCheckpoint,
    query_policy: index::SqlSessionQueryPolicy,
    saved_sql_entry_scope: &EntryScope,
) -> Result<Value> {
    crate::authorization::Authorizer::new(op.clone()).ensure_authoritative_mutation_contract()?;
    authorization.require_principals()?;
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

    let sql_id =
        match saved_sql::find_sql_id_by_text(op, ws_path, sql, saved_sql_entry_scope.clone())
            .await?
        {
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

pub async fn get_sql_session_status_authorized(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    authorization: SqlSessionExecutionAuthorization<'_>,
) -> Result<Value> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    validate_session_execution_authorization(&meta, authorization)?;
    Ok(meta)
}

pub async fn get_sql_session_count_authorized_by_form(
    op: &Operator,
    ws_path: &str,
    session_id: &str,
    authorization: SqlSessionExecutionAuthorization<'_>,
) -> Result<u64> {
    let meta = load_session_meta(op, ws_path, session_id).await?;
    if meta.get("status").and_then(|v| v.as_str()) == Some("expired") {
        return Err(AppError::expired(ErrorCode::SqlSessionExpired, "SQL session expired").into());
    }
    validate_session_execution_authorization(&meta, authorization)?;
    index::execute_sql_query_authorized_by_form_count_at_checkpoint(
        op,
        ws_path,
        session_sql(&meta)?,
        authorization.query_policy,
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
    authorization: SqlSessionExecutionAuthorization<'_>,
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
