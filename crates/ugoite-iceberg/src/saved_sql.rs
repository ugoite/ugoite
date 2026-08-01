use crate::entry;
use crate::form;
use crate::integrity::IntegrityProvider;
use anyhow::{anyhow, Context, Result};
use opendal::Operator;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use ugoite_core::error::{AppError, ErrorCode};
use uuid::Uuid;

const SQL_FORM_NAME: &str = "SQL";
const SQL_VALIDATION_PREFIX: &str = "UGOITE_SQL_VALIDATION";

fn validation_error(message: impl std::fmt::Display) -> anyhow::Error {
    anyhow!("{SQL_VALIDATION_PREFIX}: {message}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlVariable {
    #[serde(rename = "type")]
    pub var_type: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SqlKind {
    UserQuery,
    SearchHistory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchHistoryOperator {
    Equals,
    Contains,
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SearchHistoryFieldCondition {
    pub field: String,
    pub operator: SearchHistoryOperator,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SearchHistoryCriteria {
    pub form_name: String,
    pub tags: Vec<String>,
    pub updated_from: String,
    pub updated_to: String,
    pub field_conditions: Vec<SearchHistoryFieldCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SqlGeneratedName {
    Untitled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct SqlMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_criteria: Option<SearchHistoryCriteria>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_name: Option<SqlGeneratedName>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlPayload {
    pub name: Option<String>,
    pub kind: SqlKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SqlMetadata>,
    pub sql: String,
    #[serde(default)]
    pub variables: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlUpdatePayload {
    pub name: Option<String>,
    pub kind: SqlKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SqlMetadata>,
    pub sql: String,
    #[serde(default)]
    pub variables: Value,
    pub parent_revision_id: String,
}

impl SqlUpdatePayload {
    pub fn into_sql_payload(self) -> SqlPayload {
        SqlPayload {
            name: self.name,
            kind: self.kind,
            metadata: self.metadata,
            sql: self.sql,
            variables: self.variables,
        }
    }
}

fn validate_sql_metadata(payload: &SqlPayload) -> Result<()> {
    if payload
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(validation_error("name must be null or a non-blank string"));
    }

    match payload.kind {
        SqlKind::UserQuery => match (&payload.name, &payload.metadata) {
            (Some(_), None) => {}
            (Some(_), Some(metadata))
                if metadata.generated_name.is_none() && metadata.search_criteria.is_none() =>
            {
                return Err(validation_error(
                    "user-query metadata must be omitted for named queries",
                ));
            }
            (Some(_), Some(_)) => {
                return Err(validation_error(
                    "named user-query cannot declare generated metadata",
                ));
            }
            (None, Some(metadata))
                if metadata.search_criteria.is_none()
                    && matches!(metadata.generated_name, Some(SqlGeneratedName::Untitled)) => {}
            (None, _) => {
                return Err(validation_error(
                    "unnamed user-query must declare generated_name=untitled",
                ));
            }
        },
        SqlKind::SearchHistory => {
            if payload.name.is_some() {
                return Err(validation_error(
                    "search-history requires a structured search_criteria and no name",
                ));
            }
            match payload.metadata.as_ref() {
                Some(metadata)
                    if metadata.search_criteria.is_some() && metadata.generated_name.is_none() => {}
                Some(_) => {
                    return Err(validation_error(
                        "search-history metadata must contain only search_criteria",
                    ));
                }
                None => {
                    return Err(validation_error(
                        "search-history requires a structured search_criteria and no name",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn sql_form_definition() -> Value {
    serde_json::json!({
        "name": SQL_FORM_NAME,
        "version": 1,
        "fields": {
            "sql": {"type": "sql", "required": true},
            "variables": {"type": "object_list", "required": false}
        },
        "allow_extra_attributes": "allow_json"
    })
}

async fn ensure_sql_form(op: &Operator, ws_path: &str) -> Result<Value> {
    let form_def = sql_form_definition();
    form::upsert_metadata_form(op, ws_path, &form_def).await?;
    form::read_form_definition(op, ws_path, SQL_FORM_NAME).await
}

fn normalize_sql_variables(value: Option<&Value>) -> Result<Value> {
    let items = match value {
        None => Vec::new(),
        Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(validation_error("variables must be an array")),
    };

    let mut normalized = Vec::new();
    for item in items {
        let obj = item
            .as_object()
            .ok_or_else(|| validation_error("variables items must be objects"))?;
        let var_type = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| validation_error("variables.type must be a string"))?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| validation_error("variables.name must be a string"))?;
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| validation_error("variables.description must be a string"))?;
        normalized.push(serde_json::json!({
            "type": var_type,
            "name": name,
            "description": description,
        }));
    }
    Ok(Value::Array(normalized))
}

async fn validate_sql_payload(
    op: &Operator,
    ws_path: &str,
    sql_text: &str,
    variables: &Value,
) -> Result<()> {
    let items = variables
        .as_array()
        .ok_or_else(|| validation_error("variables must be an array"))?;
    let mut var_names = BTreeSet::new();
    for item in items {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if name.is_empty() {
            return Err(validation_error(
                "variables.name must be a non-empty string",
            ));
        }
        var_names.insert(name.to_string());
    }

    let embedded_names = crate::index::datafusion_parameter_names(op, ws_path, sql_text)
        .await
        .map_err(|_| validation_error("sql is not valid DataFusion SQL"))?
        .into_iter()
        .map(|name| name.trim_start_matches('$').to_string())
        .collect::<BTreeSet<_>>();

    for name in &var_names {
        if !embedded_names.contains(name) {
            return Err(validation_error(
                "variables must be embedded in SQL as ${name}",
            ));
        }
    }

    for name in &embedded_names {
        if !var_names.contains(name) {
            return Err(validation_error(format!(
                "sql contains undefined variables: {name}",
            )));
        }
    }

    Ok(())
}

fn sql_integrity_payload(
    integrity: &dyn IntegrityProvider,
    payload: &SqlPayload,
    variables: &Value,
) -> entry::IntegrityPayload {
    let payload = serde_json::json!({
        "name": payload.name,
        "kind": payload.kind,
        "metadata": payload.metadata,
        "sql": payload.sql,
        "variables": variables,
    });
    let serialized = serde_json::to_string(&payload).unwrap_or_default();
    entry::IntegrityPayload {
        checksum: integrity.checksum(&serialized),
        signature: integrity.signature(&serialized),
    }
}

fn sql_extra_attributes(payload: &SqlPayload) -> Value {
    serde_json::json!({
        "kind": payload.kind,
        "metadata": payload.metadata,
    })
}

fn sql_entry_from_row(row: &entry::EntryRow) -> Result<Value> {
    let fields = row
        .fields
        .as_object()
        .context("SQL row fields must be an object")?;
    let sql_value = fields.get("sql").and_then(|v| v.as_str()).unwrap_or("");
    let variables = fields
        .get("variables")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let extra_attributes = row
        .extra_attributes
        .as_object()
        .context("SQL row extra_attributes must be an object")?;
    let kind = extra_attributes
        .get("kind")
        .cloned()
        .context("SQL row kind is missing")?;
    let kind: SqlKind = serde_json::from_value(kind).context("SQL row kind is invalid")?;
    let metadata = extra_attributes
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);
    let metadata = if metadata.is_null() {
        None
    } else {
        Some(
            serde_json::from_value::<SqlMetadata>(metadata)
                .context("SQL row metadata is invalid")?,
        )
    };

    Ok(serde_json::json!({
        "id": row.entry_id,
        "name": if row.title.is_empty() { Value::Null } else { Value::String(row.title.clone()) },
        "kind": kind,
        "metadata": metadata,
        "sql": sql_value,
        "variables": variables,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "revision_id": row.revision_id,
    }))
}

pub async fn list_sql(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    ensure_sql_form(op, ws_path).await?;
    let form_def = form::read_form_definition(op, ws_path, SQL_FORM_NAME).await?;
    let rows = entry::list_form_entry_rows(op, ws_path, SQL_FORM_NAME, &form_def).await?;
    let mut entries = Vec::new();
    for row in rows {
        if row.deleted {
            continue;
        }
        entries.push(sql_entry_from_row(&row)?);
    }
    Ok(entries)
}

pub async fn get_sql(op: &Operator, ws_path: &str, sql_id: &str) -> Result<Value> {
    ensure_sql_form(op, ws_path).await?;
    let row = entry::read_entry_row(op, ws_path, SQL_FORM_NAME, sql_id).await?;
    if row.deleted {
        return Err(anyhow!("SQL entry not found: {}", sql_id));
    }
    sql_entry_from_row(&row)
}

pub async fn find_sql_id_by_text(
    op: &Operator,
    ws_path: &str,
    sql_text: &str,
) -> Result<Option<String>> {
    ensure_sql_form(op, ws_path).await?;
    let form_def = form::read_form_definition(op, ws_path, SQL_FORM_NAME).await?;
    let rows = entry::list_form_entry_rows(op, ws_path, SQL_FORM_NAME, &form_def).await?;
    for row in rows {
        if row.deleted {
            continue;
        }
        let fields = row.fields.as_object();
        let sql_value = fields
            .and_then(|obj| obj.get("sql"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if sql_value == sql_text {
            return Ok(Some(row.entry_id));
        }
    }
    Ok(None)
}

pub async fn create_sql<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    if entry::find_entry_form(op, ws_path, sql_id).await?.is_some() {
        return Err(anyhow!("SQL entry already exists: {}", sql_id));
    }

    let form_def = ensure_sql_form(op, ws_path).await?;
    validate_sql_metadata(payload)?;
    let variables = normalize_sql_variables(Some(&payload.variables))?;
    validate_sql_payload(op, ws_path, &payload.sql, &variables).await?;

    let timestamp = entry::now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let integrity_payload = sql_integrity_payload(integrity, payload, &variables);

    let mut fields = Map::new();
    fields.insert("sql".to_string(), Value::String(payload.sql.to_string()));
    fields.insert("variables".to_string(), variables.clone());
    let extra_attributes = sql_extra_attributes(payload);

    let row = entry::EntryRow {
        entry_id: sql_id.to_string(),
        title: payload.name.clone().unwrap_or_default(),
        form: SQL_FORM_NAME.to_string(),
        tags: Vec::new(),
        created_at: timestamp,
        updated_at: timestamp,
        fields: Value::Object(fields),
        extra_attributes,
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        integrity: integrity_payload.clone(),
        deleted: false,
        deleted_at: None,
        author: author.to_string(),
        entry_version: 1,
    };

    let revision = entry::RevisionRow {
        revision_id: revision_id.clone(),
        entry_id: sql_id.to_string(),
        parent_revision_id: None,
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: integrity_payload.checksum.clone(),
        integrity: integrity_payload,
        restored_from: None,
        state: Some(row.clone()),
        entry_version: row.entry_version,
        operation: "upsert".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
    };
    entry::append_revision_row_for_form(op, ws_path, SQL_FORM_NAME, &revision, &form_def).await?;

    sql_entry_from_row(&row)
}

pub async fn update_sql<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    parent_revision_id: &str,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    ensure_sql_form(op, ws_path).await?;
    let form_def = form::read_form_definition(op, ws_path, SQL_FORM_NAME).await?;
    let mut row = entry::read_entry_row(op, ws_path, SQL_FORM_NAME, sql_id).await?;
    if row.deleted {
        return Err(anyhow!("SQL entry not found: {}", sql_id));
    }

    if parent_revision_id.trim().is_empty() {
        return Err(AppError::invalid_input(
            ErrorCode::InvalidInput,
            "parent_revision_id must not be blank",
        )
        .into());
    }
    if row.revision_id != parent_revision_id {
        return Err(AppError::conflict(
            ErrorCode::RevisionConflict,
            format!(
                "Revision conflict: expected {}, got {}",
                parent_revision_id, row.revision_id
            ),
        )
        .into());
    }

    let variables = normalize_sql_variables(Some(&payload.variables))?;
    validate_sql_metadata(payload)?;
    validate_sql_payload(op, ws_path, &payload.sql, &variables).await?;
    let mut timestamp = entry::now_ts();
    if timestamp <= row.updated_at {
        timestamp = row.updated_at + 0.001;
    }
    let revision_id = Uuid::new_v4().to_string();
    let integrity_payload = sql_integrity_payload(integrity, payload, &variables);

    let mut fields = Map::new();
    fields.insert("sql".to_string(), Value::String(payload.sql.to_string()));
    fields.insert("variables".to_string(), variables.clone());
    let extra_attributes = sql_extra_attributes(payload);

    row.title = payload.name.clone().unwrap_or_default();
    row.updated_at = timestamp;
    row.fields = Value::Object(fields);
    row.extra_attributes = extra_attributes;
    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = revision_id.clone();
    row.entry_version = row.entry_version.saturating_add(1);
    row.author = author.to_string();
    row.integrity = integrity_payload.clone();

    let revision = entry::RevisionRow {
        revision_id: revision_id.clone(),
        entry_id: sql_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: integrity_payload.checksum.clone(),
        integrity: integrity_payload,
        restored_from: None,
        state: Some(row.clone()),
        entry_version: row.entry_version,
        operation: "upsert".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
    };
    entry::append_revision_row_for_form(op, ws_path, SQL_FORM_NAME, &revision, &form_def).await?;

    sql_entry_from_row(&row)
}

pub async fn delete_sql(op: &Operator, ws_path: &str, sql_id: &str) -> Result<()> {
    ensure_sql_form(op, ws_path).await?;
    let mut row = entry::read_entry_row(op, ws_path, SQL_FORM_NAME, sql_id).await?;
    if row.deleted {
        return Err(anyhow!("SQL entry not found: {}", sql_id));
    }

    let mut delete_ts = entry::now_ts();
    if delete_ts <= row.updated_at {
        delete_ts = row.updated_at + 0.001;
    }
    row.deleted = true;
    row.deleted_at = Some(delete_ts);
    row.updated_at = delete_ts;
    entry::write_entry_row(op, ws_path, SQL_FORM_NAME, sql_id, &row).await?;
    Ok(())
}
