use crate::class;
use crate::integrity::IntegrityProvider;
use crate::note;
use crate::sql;
use anyhow::{anyhow, Context, Result};
use opendal::Operator;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::sync::OnceLock;
use uuid::Uuid;

const SQL_CLASS_NAME: &str = "SQL";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlVariable {
    #[serde(rename = "type")]
    pub var_type: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqlPayload {
    pub name: String,
    pub sql: String,
    #[serde(default)]
    pub variables: Value,
}

fn sql_class_definition() -> Value {
    serde_json::json!({
        "name": SQL_CLASS_NAME,
        "version": 1,
        "fields": {
            "sql": {"type": "markdown", "required": true},
            "variables": {"type": "object_list", "required": false}
        },
        "allow_extra_attributes": "deny"
    })
}

async fn ensure_sql_class(op: &Operator, ws_path: &str) -> Result<Value> {
    let class_def = sql_class_definition();
    class::upsert_metadata_class(op, ws_path, &class_def).await?;
    Ok(class_def)
}

fn normalize_sql_variables(value: Option<&Value>) -> Result<Value> {
    let items = match value {
        None => Vec::new(),
        Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => return Err(anyhow!("variables must be an array")),
    };

    let mut normalized = Vec::new();
    for item in items {
        let obj = item
            .as_object()
            .context("variables items must be objects")?;
        let var_type = obj
            .get("type")
            .and_then(|v| v.as_str())
            .context("variables.type must be a string")?;
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .context("variables.name must be a string")?;
        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .context("variables.description must be a string")?;
        normalized.push(serde_json::json!({
            "type": var_type,
            "name": name,
            "description": description,
        }));
    }
    Ok(Value::Array(normalized))
}

fn sql_placeholder_regex() -> &'static Regex {
    static SQL_PLACEHOLDER_REGEX: OnceLock<Regex> = OnceLock::new();
    SQL_PLACEHOLDER_REGEX.get_or_init(|| {
        Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}")
            .expect("sql placeholder regex must compile")
    })
}

fn validate_sql_payload(sql_text: &str, variables: &Value) -> Result<()> {
    let items = variables.as_array().context("variables must be an array")?;
    let mut var_names = BTreeSet::new();
    for item in items {
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if name.is_empty() {
            return Err(anyhow!("variables.name must be a non-empty string"));
        }
        var_names.insert(name.to_string());
    }

    let regex = sql_placeholder_regex();
    let mut embedded_names = BTreeSet::new();
    for capture in regex.captures_iter(sql_text) {
        if let Some(matched) = capture.get(1) {
            embedded_names.insert(matched.as_str().to_string());
        }
    }

    for name in &var_names {
        if !embedded_names.contains(name) {
            return Err(anyhow!(
                "variables must be embedded in sql: missing {{{{{name}}}}}",
            ));
        }
    }

    for name in &embedded_names {
        if !var_names.contains(name) {
            return Err(anyhow!("sql contains undefined variables: {name}",));
        }
    }

    let sanitized = regex.replace_all(sql_text, "1");
    sql::parse_sql(&sanitized).map_err(|err| anyhow!("sql must be valid: {err}"))?;
    Ok(())
}

fn sql_integrity_payload(
    integrity: &dyn IntegrityProvider,
    payload: &SqlPayload,
    variables: &Value,
) -> note::IntegrityPayload {
    let payload = serde_json::json!({
        "name": payload.name,
        "sql": payload.sql,
        "variables": variables,
    });
    let serialized = serde_json::to_string(&payload).unwrap_or_default();
    note::IntegrityPayload {
        checksum: integrity.checksum(&serialized),
        signature: integrity.signature(&serialized),
    }
}

fn sql_entry_from_row(row: &note::NoteRow) -> Result<Value> {
    let fields = row
        .fields
        .as_object()
        .context("SQL row fields must be an object")?;
    let sql_value = fields.get("sql").and_then(|v| v.as_str()).unwrap_or("");
    let variables = fields
        .get("variables")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));

    Ok(serde_json::json!({
        "id": row.note_id,
        "name": row.title,
        "sql": sql_value,
        "variables": variables,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "revision_id": row.revision_id,
    }))
}

pub async fn list_sql(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    ensure_sql_class(op, ws_path).await?;
    let class_def = class::read_class_definition(op, ws_path, SQL_CLASS_NAME).await?;
    let rows = note::list_class_note_rows(op, ws_path, SQL_CLASS_NAME, &class_def).await?;
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
    ensure_sql_class(op, ws_path).await?;
    let row = note::read_note_row(op, ws_path, SQL_CLASS_NAME, sql_id).await?;
    if row.deleted {
        return Err(anyhow!("SQL entry not found: {}", sql_id));
    }
    sql_entry_from_row(&row)
}

pub async fn create_sql<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    if note::find_note_class(op, ws_path, sql_id).await?.is_some() {
        return Err(anyhow!("SQL entry already exists: {}", sql_id));
    }

    let class_def = ensure_sql_class(op, ws_path).await?;
    let variables = normalize_sql_variables(Some(&payload.variables))?;
    validate_sql_payload(&payload.sql, &variables)?;

    let timestamp = note::now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let integrity_payload = sql_integrity_payload(integrity, payload, &variables);

    let mut fields = Map::new();
    fields.insert("sql".to_string(), Value::String(payload.sql.to_string()));
    fields.insert("variables".to_string(), variables.clone());

    let row = note::NoteRow {
        note_id: sql_id.to_string(),
        title: payload.name.to_string(),
        class: SQL_CLASS_NAME.to_string(),
        tags: Vec::new(),
        links: Vec::new(),
        canvas_position: Value::Object(Map::new()),
        created_at: timestamp,
        updated_at: timestamp,
        fields: Value::Object(fields),
        extra_attributes: Value::Object(Map::new()),
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        attachments: Vec::new(),
        integrity: integrity_payload.clone(),
        deleted: false,
        deleted_at: None,
        author: author.to_string(),
    };

    note::write_note_row(op, ws_path, SQL_CLASS_NAME, sql_id, &row).await?;

    let revision = note::RevisionRow {
        revision_id: revision_id.clone(),
        note_id: sql_id.to_string(),
        parent_revision_id: None,
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: integrity_payload.checksum.clone(),
        integrity: integrity_payload,
        restored_from: None,
    };
    note::append_revision_row_for_class(op, ws_path, SQL_CLASS_NAME, &revision, &class_def).await?;

    sql_entry_from_row(&row)
}

pub async fn update_sql<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    sql_id: &str,
    payload: &SqlPayload,
    parent_revision_id: Option<&str>,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    ensure_sql_class(op, ws_path).await?;
    let class_def = class::read_class_definition(op, ws_path, SQL_CLASS_NAME).await?;
    let mut row = note::read_note_row(op, ws_path, SQL_CLASS_NAME, sql_id).await?;
    if row.deleted {
        return Err(anyhow!("SQL entry not found: {}", sql_id));
    }

    if let Some(expected_parent) = parent_revision_id {
        if row.revision_id != expected_parent {
            return Err(anyhow!(
                "Revision conflict: expected {}, got {}",
                expected_parent,
                row.revision_id
            ));
        }
    }

    let variables = normalize_sql_variables(Some(&payload.variables))?;
    validate_sql_payload(&payload.sql, &variables)?;
    let mut timestamp = note::now_ts();
    if timestamp <= row.updated_at {
        timestamp = row.updated_at + 0.001;
    }
    let revision_id = Uuid::new_v4().to_string();
    let integrity_payload = sql_integrity_payload(integrity, payload, &variables);

    let mut fields = Map::new();
    fields.insert("sql".to_string(), Value::String(payload.sql.to_string()));
    fields.insert("variables".to_string(), variables.clone());

    row.title = payload.name.to_string();
    row.updated_at = timestamp;
    row.fields = Value::Object(fields);
    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = revision_id.clone();
    row.author = author.to_string();
    row.integrity = integrity_payload.clone();

    note::write_note_row(op, ws_path, SQL_CLASS_NAME, sql_id, &row).await?;

    let revision = note::RevisionRow {
        revision_id: revision_id.clone(),
        note_id: sql_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: integrity_payload.checksum.clone(),
        integrity: integrity_payload,
        restored_from: None,
    };
    note::append_revision_row_for_class(op, ws_path, SQL_CLASS_NAME, &revision, &class_def).await?;

    sql_entry_from_row(&row)
}

pub async fn delete_sql(op: &Operator, ws_path: &str, sql_id: &str) -> Result<()> {
    ensure_sql_class(op, ws_path).await?;
    let mut row = note::read_note_row(op, ws_path, SQL_CLASS_NAME, sql_id).await?;
    if row.deleted {
        return Err(anyhow!("SQL entry not found: {}", sql_id));
    }

    let mut delete_ts = note::now_ts();
    if delete_ts <= row.updated_at {
        delete_ts = row.updated_at + 0.001;
    }
    row.deleted = true;
    row.deleted_at = Some(delete_ts);
    row.updated_at = delete_ts;
    note::write_note_row(op, ws_path, SQL_CLASS_NAME, sql_id, &row).await?;
    Ok(())
}
