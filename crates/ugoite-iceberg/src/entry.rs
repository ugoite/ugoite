use crate::form;
use crate::iceberg_store;
use crate::index;
use crate::integrity::IntegrityProvider;
use crate::{IcebergWorkspace, RevisionView, SpaceCheckpoint};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use opendal::Operator;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_core::query::EntryScope;
use ugoite_domain::entry::{
    AssetReference, EntryIntegrity, EntryMetadata, EntryOperation, EntryRevision, FieldValue,
    RevisionError,
};
use ugoite_domain::form::{sql_relation_name, FieldType, FormField, ListItemDefinition};
use ugoite_domain::id::{validate_asset_id, FieldId, FormId, RevisionId};
use uuid::Uuid;

pub const MAX_ENTRY_CREATE_BATCH_SIZE: usize = 256;

fn entry_not_found(entry_id: &str) -> AppError {
    AppError::not_found(
        ErrorCode::EntryNotFound,
        format!("Entry not found: {entry_id}"),
    )
}

fn entry_content_not_found(entry_id: &str) -> AppError {
    AppError::not_found(
        ErrorCode::EntryNotFound,
        format!("Entry content not found: {entry_id}"),
    )
}

fn revision_not_found(entry_id: &str, revision_id: &str) -> AppError {
    AppError::not_found(
        ErrorCode::RevisionNotFound,
        format!("Revision {revision_id} not found for entry {entry_id}"),
    )
}

fn invalid_entry_input(message: impl Into<String>) -> anyhow::Error {
    AppError::invalid_input(ErrorCode::InvalidInput, message).into()
}

fn invalid_revision_input(
    error: RevisionError,
    form: &ugoite_domain::form::FormDefinition,
) -> anyhow::Error {
    let field_name = |field_id: FieldId| {
        form.fields
            .iter()
            .find(|field| field.id == field_id)
            .map(|field| field.name.as_str())
            .unwrap_or("unknown")
    };
    let message = match error {
        RevisionError::InvalidAssetReference(field_id) => {
            format!("Field '{}': invalid AssetReference", field_name(field_id))
        }
        RevisionError::DuplicateAssetReference(field_id) => {
            format!("Field '{}': duplicate AssetReference", field_name(field_id))
        }
        RevisionError::RequiredField(field_id) => {
            format!(
                "Field '{}': required value is missing",
                field_name(field_id)
            )
        }
        RevisionError::UnknownField(field_id) => {
            format!("Field '{}': unknown field", field_name(field_id))
        }
        RevisionError::WrongType(field_id) => {
            format!("Field '{}': value has the wrong type", field_name(field_id))
        }
        other => format!("Invalid Entry revision: {other}"),
    };
    invalid_entry_input(message)
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct IntegrityPayload {
    #[serde(default)]
    pub checksum: String,
    #[serde(default)]
    pub signature: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntryContent {
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    #[serde(default)]
    pub timestamp: f64,
    pub author: String,
    pub markdown: String,
    #[serde(default)]
    pub frontmatter: Value,
    #[serde(default)]
    pub sections: Value,
    #[serde(default)]
    pub computed: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntryMeta {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub space_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub form: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: f64,
    #[serde(default)]
    pub updated_at: f64,
    #[serde(default)]
    pub integrity: IntegrityPayload,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub deleted_at: Option<f64>,
    #[serde(default)]
    pub properties: Value,
}

#[derive(Debug, Clone)]
pub struct EntryCreateRequest {
    pub entry_id: String,
    pub content: String,
}

impl EntryCreateRequest {
    pub fn new(entry_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            entry_id: entry_id.into(),
            content: content.into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EntryRow {
    pub entry_id: String,
    pub title: String,
    pub form: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(default)]
    pub fields: Value,
    #[serde(default)]
    pub extra_attributes: Value,
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    #[serde(default)]
    pub integrity: IntegrityPayload,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub deleted_at: Option<f64>,
    #[serde(default)]
    pub author: String,
    #[serde(default = "initial_entry_version")]
    pub entry_version: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RevisionRow {
    pub revision_id: String,
    pub entry_id: String,
    pub parent_revision_id: Option<String>,
    pub timestamp: f64,
    pub author: String,
    #[serde(default)]
    pub fields: Value,
    #[serde(default)]
    pub extra_attributes: Value,
    pub markdown_checksum: String,
    #[serde(default)]
    pub integrity: IntegrityPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
    #[serde(default)]
    pub state: Option<EntryRow>,
    #[serde(default = "initial_entry_version")]
    pub entry_version: u64,
    #[serde(default = "default_operation")]
    pub operation: String,
    #[serde(default = "default_source_kind")]
    pub source_kind: String,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub extension_metadata: Value,
}

const fn initial_entry_version() -> u64 {
    1
}
fn default_operation() -> String {
    "upsert".to_string()
}
fn default_source_kind() -> String {
    "api".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EntrySummary {
    pub id: String,
    pub title: String,
    pub form: String,
}

pub(crate) fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
}

fn to_timestamp_micros(ts: f64) -> i64 {
    (ts * 1_000_000.0).round() as i64
}

fn from_timestamp_micros(micros: i64) -> f64 {
    micros as f64 / 1_000_000.0
}

fn extract_title(content: &str, fallback: &str) -> String {
    for line in content.lines() {
        if let Some(stripped) = line.strip_prefix("# ") {
            return stripped.trim().to_string();
        }
    }
    fallback.to_string()
}

fn extract_frontmatter(content: &str) -> (Value, String) {
    let re = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").unwrap();
    if let Some(caps) = re.captures(content) {
        let yaml_str = caps.get(1).unwrap().as_str();
        let fm_yaml: Option<serde_yaml::Value> = serde_yaml::from_str(yaml_str).ok();
        let fm_json = fm_yaml
            .and_then(|y| serde_json::to_value(y).ok())
            .unwrap_or_else(|| Value::Object(Map::new()));
        let end = caps.get(0).unwrap().end();
        return (fm_json, content[end..].to_string());
    }
    (Value::Object(Map::new()), content.to_string())
}

fn extract_sections(body: &str) -> Value {
    let mut sections: Map<String, Value> = Map::new();
    let header_re = Regex::new(r"^##\s+(.+)$").unwrap();
    let mut current_key: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();

    for line in body.lines() {
        if let Some(caps) = header_re.captures(line) {
            if let Some(key) = current_key.take() {
                sections.insert(key, Value::String(buffer.join("\n").trim().to_string()));
            }
            current_key = Some(caps.get(1).unwrap().as_str().trim().to_string());
            buffer.clear();
            continue;
        }

        if line.starts_with('#') {
            if let Some(key) = current_key.take() {
                sections.insert(key, Value::String(buffer.join("\n").trim().to_string()));
            }
            buffer.clear();
            continue;
        }

        if current_key.is_some() {
            buffer.push(line.to_string());
        }
    }

    if let Some(key) = current_key {
        sections.insert(key, Value::String(buffer.join("\n").trim().to_string()));
    }

    Value::Object(sections)
}

fn parse_markdown(content: &str) -> (Value, Value) {
    let (frontmatter, body) = extract_frontmatter(content);
    let sections = extract_sections(&body);
    (frontmatter, sections)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExtraAttributesPolicy {
    Deny,
    AllowJson,
    AllowColumns,
}

fn extra_attributes_policy(form_def: &Value) -> ExtraAttributesPolicy {
    match form_def
        .get("allow_extra_attributes")
        .and_then(|v| v.as_str())
    {
        Some("allow_json") => ExtraAttributesPolicy::AllowJson,
        Some("allow_columns") => ExtraAttributesPolicy::AllowColumns,
        _ => ExtraAttributesPolicy::Deny,
    }
}

fn collect_extra_attributes(sections: &Value, form_set: &HashSet<String>) -> (Vec<String>, Value) {
    let mut extras = Vec::new();
    let mut entries = Vec::new();

    if let Some(section_map) = sections.as_object() {
        for (key, value) in section_map {
            if !form_set.contains(key) {
                extras.push(key.clone());
                entries.push((key.clone(), value.clone()));
            }
        }
    }

    extras.sort();
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut map = Map::new();
    for (key, value) in entries {
        map.insert(key, value);
    }

    (extras, Value::Object(map))
}

pub(crate) fn merge_entry_fields(fields: &Value, extra_attributes: &Value) -> Value {
    let mut merged = Map::new();
    if let Some(map) = fields.as_object() {
        for (key, value) in map {
            merged.insert(key.clone(), value.clone());
        }
    }
    if let Some(map) = extra_attributes.as_object() {
        for (key, value) in map {
            merged.insert(key.clone(), value.clone());
        }
    }
    Value::Object(merged)
}

fn form_field_names(form_def: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(fields) = form_def.get("fields") {
        match fields {
            Value::Object(map) => {
                for key in map.keys() {
                    names.push(key.clone());
                }
            }
            Value::Array(items) => {
                for item in items {
                    if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                        names.push(name.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    names
}

fn render_frontmatter(form_name: &str, tags: &[String]) -> String {
    let mut frontmatter = String::from("---\n");
    frontmatter.push_str(&format!("form: {}\n", form_name));
    if !tags.is_empty() {
        frontmatter.push_str("tags:\n");
        for tag in tags {
            frontmatter.push_str(&format!("  - {}\n", tag));
        }
    }
    frontmatter.push_str("---\n");
    frontmatter
}

fn section_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Array(items) => {
            let has_complex = items
                .iter()
                .any(|item| matches!(item, Value::Object(_) | Value::Array(_)));
            if has_complex {
                serde_json::to_string(value).unwrap_or_default()
            } else {
                items
                    .iter()
                    .map(|item| match item {
                        Value::String(s) => format!("- {}", s),
                        Value::Number(n) => format!("- {}", n),
                        Value::Bool(b) => format!("- {}", b),
                        _ => "-".to_string(),
                    })
                    .collect::<Vec<String>>()
                    .join("\n")
            }
        }
        Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

pub(crate) fn render_markdown(
    title: &str,
    form_name: &str,
    tags: &[String],
    fields: &Value,
    field_order: &[String],
) -> String {
    let mut markdown = String::new();
    markdown.push_str(&render_frontmatter(form_name, tags));
    markdown.push_str(&format!("# {}\n\n", title));

    let mut ordered_fields = Vec::new();
    let field_map = fields.as_object();
    if let Some(map) = field_map {
        let mut seen = HashSet::new();
        for name in field_order {
            if let Some(value) = map.get(name) {
                ordered_fields.push((name.clone(), value.clone()));
                seen.insert(name.clone());
            }
        }
        let mut remaining = Vec::new();
        for (name, value) in map {
            if !seen.contains(name) {
                remaining.push((name.clone(), value.clone()));
            }
        }
        remaining.sort_by(|a, b| a.0.cmp(&b.0));
        ordered_fields.extend(remaining);
    }

    for (name, value) in ordered_fields {
        markdown.push_str(&format!("## {}\n", name));
        let rendered = section_value_to_string(&value);
        if !rendered.is_empty() {
            markdown.push_str(&rendered);
            markdown.push('\n');
        }
        markdown.push('\n');
    }

    markdown.trim_end().to_string()
}

fn sections_from_fields(fields: &Value) -> Value {
    let mut sections = Map::new();
    if let Some(map) = fields.as_object() {
        for (key, value) in map {
            sections.insert(key.clone(), Value::String(section_value_to_string(value)));
        }
    }
    Value::Object(sections)
}

pub(crate) fn render_markdown_for_form(
    title: &str,
    form_name: &str,
    tags: &[String],
    fields: &Value,
    extra_attributes: &Value,
    form_def: &Value,
) -> String {
    let field_order = form_field_names(form_def);
    let merged_fields = merge_entry_fields(fields, extra_attributes);
    render_markdown(title, form_name, tags, &merged_fields, &field_order)
}

async fn append_revision_rows_to_workspace(
    op: &Operator,
    ws_path: &str,
    rows: &[RevisionRow],
    form_def: &Value,
) -> Result<()> {
    append_revision_rows_to_workspace_authorized(op, ws_path, rows, form_def, None).await
}

async fn append_revision_rows_to_workspace_authorized(
    op: &Operator,
    ws_path: &str,
    rows: &[RevisionRow],
    form_def: &Value,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
) -> Result<()> {
    if rows.is_empty() {
        return Err(anyhow!("revision batch must not be empty"));
    }
    let domain_form = form::to_domain_form(form_def)?;
    if let Some(scopes) = relation_scopes {
        if !scopes.contains_key(&domain_form.name.to_ascii_lowercase()) {
            return Err(AppError::forbidden("Form is not readable").into());
        }
    }
    let revisions = rows
        .iter()
        .map(|row| revision_row_to_domain(row, &domain_form))
        .collect::<Result<Vec<_>>>()
        .map_err(|error| invalid_entry_input(format!("Entry validation failed: {error:#}")))?;
    validate_asset_references_exist(op, ws_path, &domain_form, &revisions).await?;
    // Close the JSON-to-domain boundary before creating a publication
    // command. This keeps malformed Form-owned Asset values and all other
    // revision validation failures as client input errors, rather than
    // allowing them to surface as an internal commit failure.
    for revision in &revisions {
        revision
            .validate_payload(&domain_form)
            .map_err(|error| invalid_revision_input(error, &domain_form))?;
    }
    let workspace = iceberg_store::native_workspace(op, ws_path).await?;
    let command = crate::publication_context(
        format!(
            "entry-revision-batch:{}",
            revisions
                .iter()
                .map(|revision| revision.revision_id.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        "entry.append",
        &revisions,
    )?;
    workspace
        .commit(command)?
        .append_revisions_authorized(domain_form.id, revisions, relation_scopes)
        .await?;
    Ok(())
}

fn revision_row_to_domain(
    row: &RevisionRow,
    form: &ugoite_domain::form::FormDefinition,
) -> Result<EntryRevision> {
    let entry_id = Uuid::parse_str(&row.entry_id)
        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, row.entry_id.as_bytes()));
    let revision_id = Uuid::parse_str(&row.revision_id)?;
    let parent_revision_id = row
        .parent_revision_id
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()?;
    let operation = match row.operation.as_str() {
        "upsert" => EntryOperation::Upsert,
        "delete" => EntryOperation::Delete,
        "restore" => EntryOperation::Restore,
        other => return Err(anyhow!("unsupported revision operation: {other}")),
    };
    let values = if operation == EntryOperation::Delete {
        Default::default()
    } else {
        form_values_to_domain(&row.fields, form)?
    };
    let extra_attributes = row
        .extra_attributes
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mut entry = row
        .state
        .as_ref()
        .map(entry_metadata_from_row)
        .unwrap_or_default();
    entry.integrity = EntryIntegrity {
        checksum: row.integrity.checksum.clone(),
        signature: row.integrity.signature.clone(),
    };
    entry.restored_from = row
        .restored_from
        .as_deref()
        .map(Uuid::parse_str)
        .transpose()?
        .map(RevisionId::from);
    Ok(EntryRevision {
        form_id: form.id,
        entry_id: entry_id.into(),
        revision_id: revision_id.into(),
        parent_revision_id: parent_revision_id.map(RevisionId::from),
        entry_version: row.entry_version,
        expected_version: parent_revision_id.map(|_| row.entry_version.saturating_sub(1)),
        operation,
        committed_at_micros: to_timestamp_micros(row.timestamp),
        author_id: row.author.clone(),
        form_version: form.version,
        source_kind: row.source_kind.clone(),
        source_id: row.source_id.clone(),
        entry,
        values,
        extra_attributes,
        extension_metadata: row
            .extension_metadata
            .as_object()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .collect(),
    })
}

fn entry_metadata_from_row(row: &EntryRow) -> EntryMetadata {
    EntryMetadata {
        external_id: row.entry_id.clone(),
        title: row.title.clone(),
        tags: row.tags.clone(),
        created_at_micros: to_timestamp_micros(row.created_at),
        updated_at_micros: to_timestamp_micros(row.updated_at),
        integrity: EntryIntegrity {
            checksum: row.integrity.checksum.clone(),
            signature: row.integrity.signature.clone(),
        },
        deleted: row.deleted,
        deleted_at_micros: row.deleted_at.map(to_timestamp_micros),
        restored_from: None,
    }
}

fn form_values_to_domain(
    fields: &Value,
    form: &ugoite_domain::form::FormDefinition,
) -> Result<std::collections::BTreeMap<FieldId, FieldValue>> {
    let object = fields.as_object().cloned().unwrap_or_default();
    let mut values = std::collections::BTreeMap::new();
    for field in &form.fields {
        if let Some(value) = object.get(&field.name) {
            values.insert(
                field.id,
                json_to_field_value_for_field(value, field).map_err(|error| {
                    invalid_entry_input(format!("Field '{}': {error}", field.name))
                })?,
            );
        }
    }
    Ok(values)
}

fn json_to_field_value_for_field(value: &Value, field: &FormField) -> Result<FieldValue> {
    json_to_field_value_for_type(value, &field.field_type, field.list_item.as_ref())
}

/// Convert transport JSON to the canonical domain value exactly once.
///
/// The Form type is part of the conversion boundary: JSON integers remain
/// `FieldValue::Integer`, while floating fields become `FieldValue::Number`.
/// List items use the same canonicalization as scalar fields, so writers and
/// validators do not need a second transport coercion step.
fn json_to_field_value_for_type(
    value: &Value,
    field_type: &FieldType,
    list_item: Option<&ListItemDefinition>,
) -> Result<FieldValue> {
    if value.is_null() {
        return Ok(FieldValue::Null);
    }
    // Markdown lists arrive as strings because Markdown has no native JSON
    // scalar type. Treat the explicit null transport markers as null for
    // typed items, while preserving the literal string "null" for string
    // lists.
    if !matches!(
        field_type,
        FieldType::String | FieldType::Markdown | FieldType::Sql
    ) && value
        .as_str()
        .is_some_and(|value| matches!(value.trim(), "null" | "~"))
    {
        return Ok(FieldValue::Null);
    }
    match field_type {
        FieldType::String | FieldType::Markdown | FieldType::Sql | FieldType::RowReference => {
            Ok(FieldValue::String(
                value
                    .as_str()
                    .context("typed string field must be a string")?
                    .to_string(),
            ))
        }
        FieldType::Boolean => Ok(FieldValue::Boolean(
            value
                .as_bool()
                .or_else(|| {
                    value.as_str().and_then(|value| match value.trim() {
                        "true" | "True" | "TRUE" => Some(true),
                        "false" | "False" | "FALSE" => Some(false),
                        _ => None,
                    })
                })
                .context("boolean field must be a boolean")?,
        )),
        FieldType::Integer => Ok(FieldValue::Integer(i64::from(
            i32::try_from(
                value
                    .as_i64()
                    .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
                    .context("integer field must be an integer")?,
            )
            .context("integer field is outside the Int32 range")?,
        ))),
        FieldType::Long => Ok(FieldValue::Integer(
            value
                .as_i64()
                .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
                .context("long field must be an integer")?,
        )),
        FieldType::Float | FieldType::Double => {
            let value = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|value| value.trim().parse().ok()))
                .context("floating field must be a number")?;
            if !value.is_finite() {
                return Err(anyhow!("floating field must be finite"));
            }
            Ok(FieldValue::Number(value))
        }
        FieldType::Date => {
            let value = value.as_str().context("date field must be a string")?;
            let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
            Ok(FieldValue::String(date.format("%Y-%m-%d").to_string()))
        }
        FieldType::Time => Ok(FieldValue::String(
            index::normalize_time(value.as_str().context("time field must be a string")?)
                .context("invalid time field")?,
        )),
        FieldType::Timestamp => Ok(FieldValue::String(
            index::normalize_wall_timestamp(
                value.as_str().context("timestamp field must be a string")?,
                false,
            )
            .context("invalid timestamp field")?,
        )),
        FieldType::TimestampTz => Ok(FieldValue::String(
            index::normalize_zoned_timestamp(
                value
                    .as_str()
                    .context("timestamp_tz field must be a string")?,
                false,
            )
            .context("invalid timestamp_tz field")?,
        )),
        FieldType::TimestampNs => Ok(FieldValue::String(
            index::normalize_wall_timestamp(
                value
                    .as_str()
                    .context("timestamp_ns field must be a string")?,
                true,
            )
            .context("invalid timestamp_ns field")?,
        )),
        FieldType::TimestampTzNs => Ok(FieldValue::String(
            index::normalize_zoned_timestamp(
                value
                    .as_str()
                    .context("timestamp_tz_ns field must be a string")?,
                true,
            )
            .context("invalid timestamp_tz_ns field")?,
        )),
        FieldType::Uuid => Ok(FieldValue::String(
            Uuid::parse_str(value.as_str().context("UUID field must be a string")?)?.to_string(),
        )),
        FieldType::Binary => Ok(FieldValue::String(
            index::normalize_binary(value.as_str().context("binary field must be a string")?)
                .context("invalid binary field")?,
        )),
        FieldType::AssetReference => Ok(FieldValue::AssetReference(
            serde_json::from_value::<AssetReference>(match value {
                Value::String(raw) => serde_json::from_str(raw)
                    .context("asset reference list item must contain a JSON object")?,
                value => value.clone(),
            })
            .context("invalid asset reference value")?,
        )),
        FieldType::List => {
            let values = value
                .as_array()
                .context("typed list field must be an array")?
                .iter()
                .map(|item| {
                    let item_type = list_item
                        .map(|item| &item.field_type)
                        .unwrap_or(&FieldType::String);
                    json_to_field_value_for_type(item, item_type, None)
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(FieldValue::List(values))
        }
        FieldType::ObjectList => Ok(FieldValue::List(
            value
                .as_array()
                .context("object list field must be an array")?
                .iter()
                .map(json_to_untyped_field_value)
                .collect::<Result<Vec<_>>>()?,
        )),
    }
}

fn json_to_untyped_field_value(value: &Value) -> Result<FieldValue> {
    Ok(match value {
        Value::Null => FieldValue::Null,
        Value::Bool(value) => FieldValue::Boolean(*value),
        Value::String(value) => FieldValue::String(value.clone()),
        Value::Number(value) => FieldValue::Number(value.as_f64().context("invalid number")?),
        Value::Array(values) => FieldValue::List(
            values
                .iter()
                .map(json_to_untyped_field_value)
                .collect::<Result<Vec<_>>>()?,
        ),
        Value::Object(values) => FieldValue::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), json_to_untyped_field_value(value)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

fn revision_row_from_domain(
    revision: EntryRevision,
    form_name: &str,
    form: &ugoite_domain::form::FormDefinition,
) -> Result<RevisionRow> {
    let fields = form
        .fields
        .iter()
        .filter_map(|field| {
            revision
                .values
                .get(&field.id)
                .map(|value| Ok((field.name.clone(), serde_json::to_value(value)?)))
        })
        .collect::<Result<Map<String, Value>>>()?;
    let integrity = IntegrityPayload {
        checksum: revision.entry.integrity.checksum.clone(),
        signature: revision.entry.integrity.signature.clone(),
    };
    let state = EntryRow {
        entry_id: if revision.entry.external_id.is_empty() {
            revision.entry_id.to_string()
        } else {
            revision.entry.external_id.clone()
        },
        title: revision.entry.title.clone(),
        form: form_name.to_string(),
        tags: revision.entry.tags.clone(),
        created_at: from_timestamp_micros(revision.entry.created_at_micros),
        updated_at: from_timestamp_micros(revision.entry.updated_at_micros),
        fields: Value::Object(fields.clone()),
        extra_attributes: serde_json::to_value(&revision.extra_attributes)?,
        revision_id: revision.revision_id.to_string(),
        parent_revision_id: revision.parent_revision_id.map(|id| id.to_string()),
        integrity: integrity.clone(),
        deleted: revision.entry.deleted,
        deleted_at: revision.entry.deleted_at_micros.map(from_timestamp_micros),
        author: revision.author_id.clone(),
        entry_version: revision.entry_version,
    };
    Ok(RevisionRow {
        revision_id: revision.revision_id.to_string(),
        entry_id: if revision.entry.external_id.is_empty() {
            revision.entry_id.to_string()
        } else {
            revision.entry.external_id.clone()
        },
        parent_revision_id: revision.parent_revision_id.map(|id| id.to_string()),
        timestamp: from_timestamp_micros(revision.committed_at_micros),
        author: revision.author_id,
        fields: Value::Object(fields),
        extra_attributes: serde_json::to_value(&revision.extra_attributes)?,
        markdown_checksum: integrity.checksum.clone(),
        integrity,
        restored_from: revision.entry.restored_from.map(|id| id.to_string()),
        state: Some(state),
        entry_version: revision.entry_version,
        operation: match revision.operation {
            EntryOperation::Upsert => "upsert",
            EntryOperation::Delete => "delete",
            EntryOperation::Restore => "restore",
        }
        .to_string(),
        source_kind: revision.source_kind,
        source_id: revision.source_id,
        extension_metadata: serde_json::to_value(&revision.extension_metadata)?,
    })
}

async fn revision_rows_for_form(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
) -> Result<(Value, Vec<RevisionRow>)> {
    let (form, revisions) = iceberg_store::revisions_for_form(op, ws_path, form_name).await?;
    let form_def = form::from_domain_form(&form);
    let rows = revisions
        .into_iter()
        .map(|revision| revision_row_from_domain(revision, form_name, &form))
        .collect::<Result<Vec<_>>>()?;
    Ok((form_def, rows))
}

pub(crate) async fn list_form_names(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    form::list_form_names(op, ws_path).await
}

pub(crate) async fn find_entry_form(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
) -> Result<Option<String>> {
    find_entry_form_with_deleted(op, ws_path, entry_id, false).await
}

/// Resolves the owning Form from the latest head, including a tombstone. This
/// is deliberately separate from current reads: history and restore must keep
/// reaching an Entry after its latest revision is a delete.
pub(crate) async fn find_entry_form_with_deleted(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    include_deleted: bool,
) -> Result<Option<String>> {
    for form_name in list_form_names(op, ws_path).await? {
        let (_, revisions) =
            iceberg_store::latest_revisions_for_entry(op, ws_path, &form_name, entry_id).await?;
        if revisions.into_iter().any(|revision| {
            revision.entry.external_id == entry_id && (include_deleted || !revision.entry.deleted)
        }) {
            return Ok(Some(form_name));
        }
    }
    Ok(None)
}

pub(crate) async fn read_entry_row(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    entry_id: &str,
) -> Result<EntryRow> {
    let (form, revisions) =
        iceberg_store::latest_revisions_for_entry(op, ws_path, form_name, entry_id).await?;
    let selected = revisions
        .into_iter()
        .map(|revision| revision_row_from_domain(revision, form_name, &form))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .find(|revision| revision.entry_id == entry_id)
        .ok_or_else(|| entry_not_found(entry_id))?;
    selected
        .state
        .ok_or_else(|| entry_not_found(entry_id).into())
}

fn entry_scope_for_lookup(entry_scope: &EntryScope, entry_id: &str) -> EntryScope {
    let entry_id = Uuid::parse_str(entry_id)
        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, entry_id.as_bytes()))
        .into();
    match entry_scope {
        EntryScope::AllCurrent => EntryScope::Only(BTreeSet::from([entry_id])),
        EntryScope::Only(ids) if ids.contains(&entry_id) => {
            EntryScope::Only(BTreeSet::from([entry_id]))
        }
        EntryScope::Only(_) => EntryScope::Only(BTreeSet::new()),
        EntryScope::AllExcept(ids) if !ids.contains(&entry_id) => {
            EntryScope::Only(BTreeSet::from([entry_id]))
        }
        EntryScope::AllExcept(_) => EntryScope::Only(BTreeSet::new()),
    }
}

pub(crate) async fn read_entry_row_authorized(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    entry_id: &str,
    entry_scope: &EntryScope,
) -> Result<EntryRow> {
    let (form, revisions) = iceberg_store::latest_revisions_for_form_authorized(
        op,
        ws_path,
        form_name,
        entry_scope_for_lookup(entry_scope, entry_id),
    )
    .await?;
    revisions
        .into_iter()
        .map(|revision| revision_row_from_domain(revision, form_name, &form))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .find(|revision| revision.entry_id == entry_id)
        .and_then(|revision| revision.state)
        .ok_or_else(|| entry_not_found(entry_id).into())
}

pub(crate) async fn append_revision_row_for_form(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    row: &RevisionRow,
    form_def: &Value,
) -> Result<()> {
    append_revision_row_for_form_authorized(op, ws_path, form_name, row, form_def, None).await
}

pub(crate) async fn append_revision_row_for_form_authorized(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    row: &RevisionRow,
    form_def: &Value,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
) -> Result<()> {
    let _ = form_name;
    append_revision_rows_to_workspace_authorized(
        op,
        ws_path,
        std::slice::from_ref(row),
        form_def,
        relation_scopes,
    )
    .await
}

pub async fn append_revision_batch_for_form(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    rows: &[RevisionRow],
) -> Result<()> {
    let form_def = form::read_form_definition(op, ws_path, form_name).await?;
    append_revision_rows_to_workspace(op, ws_path, rows, &form_def).await
}

fn extract_tags(frontmatter: &Value) -> Vec<String> {
    match frontmatter.get("tags") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(tag)) => vec![tag.to_string()],
        _ => Vec::new(),
    }
}

fn extract_form(frontmatter: &Value) -> Option<String> {
    frontmatter
        .get("form")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

pub async fn create_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
    author: &str,
    integrity: &I,
) -> Result<EntryMeta> {
    create_entry_with_scopes(op, ws_path, entry_id, content, author, integrity, None).await
}

pub async fn create_entry_with_scopes<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
    author: &str,
    integrity: &I,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
) -> Result<EntryMeta> {
    let mut entries = create_entries_with_scopes(
        op,
        ws_path,
        vec![EntryCreateRequest::new(entry_id, content)],
        author,
        integrity,
        relation_scopes,
    )
    .await?;
    Ok(entries
        .pop()
        .expect("a one-entry create batch must return one entry"))
}

/// Creates one explicit batch. Each Form represented in the batch publishes
/// one upstream Iceberg snapshot after all entries have been validated.
pub async fn create_entries<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    requests: Vec<EntryCreateRequest>,
    author: &str,
    integrity: &I,
) -> Result<Vec<EntryMeta>> {
    create_entries_with_scopes(op, ws_path, requests, author, integrity, None).await
}

pub async fn create_entries_with_scopes<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    requests: Vec<EntryCreateRequest>,
    author: &str,
    integrity: &I,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
) -> Result<Vec<EntryMeta>> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    if requests.len() > MAX_ENTRY_CREATE_BATCH_SIZE {
        return Err(invalid_entry_input(format!(
            "entry create batches are limited to {MAX_ENTRY_CREATE_BATCH_SIZE} requests"
        )));
    }
    let mut requested_entry_ids = HashSet::new();
    for request in &requests {
        if !requested_entry_ids.insert(request.entry_id.clone()) {
            return Err(invalid_entry_input(format!(
                "Entry ID '{}' appears more than once in this create request",
                request.entry_id
            )));
        }
    }
    let mut batches = BTreeMap::<String, (Value, Vec<RevisionRow>)>::new();
    let mut entries = Vec::with_capacity(requests.len());
    for request in requests {
        let (entry, form_name, form_def, revision) = prepare_entry(
            op,
            ws_path,
            &request.entry_id,
            &request.content,
            author,
            integrity,
        )
        .await?;
        if let Some((_, revisions)) = batches.get_mut(&form_name) {
            revisions.push(revision);
        } else {
            batches.insert(form_name, (form_def, vec![revision]));
        }
        entries.push(entry);
    }
    reject_cross_form_forward_references(&batches)?;
    let workspace = iceberg_store::native_workspace(op, ws_path).await?;
    let domain_batches = batches
        .values()
        .map(|(form_def, revisions)| {
            let form = form::to_domain_form(form_def)?;
            let revisions = revisions
                .iter()
                .map(|revision| revision_row_to_domain(revision, &form))
                .collect::<Result<Vec<_>>>()
                .map_err(|error| {
                    invalid_entry_input(format!("Entry validation failed: {error:#}"))
                })?;
            Ok((form.id, revisions))
        })
        .collect::<Result<Vec<_>>>()?;
    for ((form_def, _), (_, revisions)) in batches.values().zip(&domain_batches) {
        let form = form::to_domain_form(form_def)?;
        validate_asset_references_exist(op, ws_path, &form, revisions).await?;
    }
    workspace
        .validate_revision_batches_authorized(&domain_batches, relation_scopes)
        .await?;
    for (_, (form_def, revisions)) in batches {
        append_revision_rows_to_workspace_authorized(
            op,
            ws_path,
            &revisions,
            &form_def,
            relation_scopes,
        )
        .await?;
    }
    Ok(entries)
}

async fn validate_asset_references_exist(
    op: &Operator,
    ws_path: &str,
    form: &ugoite_domain::form::FormDefinition,
    revisions: &[EntryRevision],
) -> Result<()> {
    let mut references = BTreeMap::<String, String>::new();
    for revision in revisions {
        if matches!(
            revision.operation,
            EntryOperation::Delete | EntryOperation::Restore
        ) {
            continue;
        }
        for field in &form.fields {
            let Some(value) = revision.values.get(&field.id) else {
                continue;
            };
            match (&field.field_type, value) {
                (FieldType::AssetReference, FieldValue::AssetReference(reference)) => {
                    validate_asset_id(&reference.asset_id).map_err(|error| {
                        invalid_entry_input(format!(
                            "Form validation failed: {}",
                            serde_json::json!([{
                                "field": field.name,
                                "message": error.to_string()
                            }])
                        ))
                    })?;
                    if i64::try_from(reference.size_bytes).is_err() {
                        return Err(invalid_entry_input(format!(
                            "Form validation failed: {}",
                            serde_json::json!([{
                                "field": field.name,
                                "message": "Asset size is too large"
                            }])
                        )));
                    }
                    references.insert(reference.asset_id.clone(), field.name.clone());
                }
                (FieldType::List, FieldValue::List(values))
                    if field
                        .list_item
                        .as_ref()
                        .is_some_and(|item| item.field_type == FieldType::AssetReference) =>
                {
                    for value in values {
                        let FieldValue::AssetReference(reference) = value else {
                            continue;
                        };
                        validate_asset_id(&reference.asset_id).map_err(|error| {
                            invalid_entry_input(format!(
                                "Form validation failed: {}",
                                serde_json::json!([{
                                    "field": field.name,
                                    "message": error.to_string()
                                }])
                            ))
                        })?;
                        if i64::try_from(reference.size_bytes).is_err() {
                            return Err(invalid_entry_input(format!(
                                "Form validation failed: {}",
                                serde_json::json!([{
                                    "field": field.name,
                                    "message": "Asset size is too large"
                                }])
                            )));
                        }
                        references.insert(reference.asset_id.clone(), field.name.clone());
                    }
                }
                _ => {}
            }
        }
    }
    for (asset_id, field) in references {
        if !crate::asset::asset_exists(op, ws_path, &asset_id).await? {
            return Err(invalid_entry_input(format!(
                "Form validation failed: {}",
                serde_json::json!([{
                    "field": field,
                    "message": format!("Asset '{asset_id}' does not exist")
                }])
            )));
        }
    }
    Ok(())
}

fn reject_cross_form_forward_references(
    batches: &BTreeMap<String, (Value, Vec<RevisionRow>)>,
) -> Result<()> {
    let pending = batches
        .values()
        .flat_map(|(form_def, revisions)| {
            let form_id = form::to_domain_form(form_def).ok().map(|form| form.id);
            revisions.iter().flat_map(move |revision| {
                form_id
                    .into_iter()
                    .map(move |form_id| (form_id, revision.entry_id.clone()))
            })
        })
        .collect::<BTreeSet<_>>();

    for (source_form_def, revisions) in batches.values() {
        let source_form = form::to_domain_form(source_form_def)?;
        for revision in revisions {
            for field in &source_form.fields {
                let Some(value) = revision
                    .state
                    .as_ref()
                    .and_then(|state| state.fields.get(&field.name))
                else {
                    continue;
                };
                let references = match (&field.field_type, value) {
                    (FieldType::RowReference, Value::String(entry_id)) => field
                        .reference_form
                        .map(|form| vec![(form, entry_id.clone())])
                        .unwrap_or_default(),
                    (FieldType::List, Value::Array(items))
                        if field
                            .list_item
                            .as_ref()
                            .is_some_and(|item| item.field_type == FieldType::RowReference) =>
                    {
                        field
                            .list_item
                            .as_ref()
                            .and_then(|item| {
                                item.reference_form.map(|form| {
                                    items
                                        .iter()
                                        .filter_map(Value::as_str)
                                        .map(|entry_id| (form, entry_id.to_string()))
                                        .collect()
                                })
                            })
                            .unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                for (target_form, entry_id) in references {
                    if target_form != source_form.id
                        && pending.contains(&(target_form, entry_id.clone()))
                    {
                        return Err(invalid_entry_input(format!(
                            "cross-Form forward references in one create batch are unsupported: '{}' -> '{}'",
                            source_form.name,
                            entry_id
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

async fn prepare_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
    author: &str,
    integrity: &I,
) -> Result<(EntryMeta, String, Value, RevisionRow)> {
    let (frontmatter, sections) = parse_markdown(content);
    let form_name = extract_form(&frontmatter)
        .ok_or_else(|| invalid_entry_input("Form is required for entry creation"))?;
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;

    let form_fields = form_field_names(&form_def);
    let form_set: HashSet<String> = form_fields.iter().cloned().collect();
    let policy = extra_attributes_policy(&form_def);
    let (extras, extra_attributes) = collect_extra_attributes(&sections, &form_set);
    if !extras.is_empty() && policy == ExtraAttributesPolicy::Deny {
        return Err(AppError::invalid_input_with_detail(
            ErrorCode::UnknownFormFields,
            "Entry contains unknown form fields",
            json!({"fields": extras}),
        )
        .into());
    }

    let properties = index::extract_properties(content);
    let (casted, warnings) = index::validate_properties(&properties, &form_def)?;
    if !warnings.is_empty() {
        return Err(AppError::invalid_input_with_detail(
            ErrorCode::FormValidationFailed,
            "Entry form validation failed",
            json!({"warnings": warnings}),
        )
        .into());
    }

    let mut fields = Map::new();
    if let Some(obj) = properties.as_object() {
        for (key, value) in obj {
            if form_set.contains(key) {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(obj) = casted.as_object() {
        for (key, value) in obj {
            if form_set.contains(key) {
                fields.insert(key.clone(), value.clone());
            }
        }
    }

    let title = extract_title(content, entry_id);
    let tags = extract_tags(&frontmatter);
    let timestamp = now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let checksum = integrity.checksum(content);
    let signature = integrity.signature(content);

    let entry_row = EntryRow {
        entry_id: entry_id.to_string(),
        title: title.clone(),
        form: form_name.clone(),
        tags,
        created_at: timestamp,
        updated_at: timestamp,
        fields: Value::Object(fields),
        extra_attributes: extra_attributes.clone(),
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        deleted: false,
        deleted_at: None,
        author: author.to_string(),
        entry_version: 1,
    };

    let revision = RevisionRow {
        revision_id: revision_id.clone(),
        entry_id: entry_id.to_string(),
        parent_revision_id: None,
        timestamp,
        author: author.to_string(),
        fields: entry_row.fields.clone(),
        extra_attributes: entry_row.extra_attributes.clone(),
        markdown_checksum: checksum.clone(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        restored_from: None,
        state: Some(entry_row.clone()),
        entry_version: entry_row.entry_version,
        operation: "upsert".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
        extension_metadata: Value::Object(Map::new()),
    };
    let ws_id = ws_path
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .unwrap_or(ws_path)
        .to_string();

    let entry = EntryMeta {
        id: entry_id.to_string(),
        space_id: ws_id,
        title,
        form: Some(form_name.clone()),
        tags: entry_row.tags.clone(),
        created_at: timestamp,
        updated_at: timestamp,
        integrity: IntegrityPayload {
            checksum,
            signature,
        },
        deleted: false,
        deleted_at: None,
        properties: Value::Object(Map::new()),
    };
    Ok((entry, form_name, form_def, revision))
}

pub async fn list_entries(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    let relation_scopes = list_form_names(op, ws_path)
        .await?
        .into_iter()
        .map(|form_name| (form_name.to_ascii_lowercase(), EntryScope::AllCurrent))
        .collect::<BTreeMap<_, _>>();
    list_entries_with_scopes(op, ws_path, &relation_scopes, crate::MAX_NORMAL_READ_ROWS).await
}

pub async fn list_entries_with_scopes(
    op: &Operator,
    ws_path: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
    limit: usize,
) -> Result<Vec<Value>> {
    let rows =
        index::query_entry_rows_authorized(op, ws_path, relation_scopes, None, None, limit).await?;
    list_entries_from_rows(rows)
}

fn list_entries_from_rows(rows: Vec<(String, EntryRow)>) -> Result<Vec<Value>> {
    let mut entries = Vec::new();
    for (form_name, row) in rows {
        if row.deleted {
            continue;
        }
        let merged_fields = merge_entry_fields(&row.fields, &row.extra_attributes);
        entries.push(serde_json::json!({
            "id": row.entry_id,
            "title": row.title,
            "form": form_name,
            "tags": row.tags,
            "properties": merged_fields,
            "created_at": row.created_at,
            "updated_at": row.updated_at,
        }));
    }
    Ok(entries)
}

pub async fn list_entry_summaries(
    op: &Operator,
    ws_path: &str,
    form_filter: Option<&str>,
    query: Option<&str>,
    limit: usize,
) -> Result<Vec<EntrySummary>> {
    let relation_scopes = list_form_names(op, ws_path)
        .await?
        .into_iter()
        .map(|form_name| (form_name.to_ascii_lowercase(), EntryScope::AllCurrent))
        .collect::<BTreeMap<_, _>>();
    list_entry_summaries_with_scopes(op, ws_path, form_filter, query, limit, &relation_scopes).await
}

pub async fn list_entry_summaries_with_scopes(
    op: &Operator,
    ws_path: &str,
    form_filter: Option<&str>,
    query: Option<&str>,
    limit: usize,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Vec<EntrySummary>> {
    let candidates = index::query_entry_candidates_authorized(
        op,
        ws_path,
        relation_scopes,
        form_filter,
        query,
        limit,
    )
    .await?;
    Ok(candidates
        .into_iter()
        .map(|candidate| EntrySummary {
            id: candidate.entry_id,
            title: candidate.title,
            form: candidate.form_name,
        })
        .collect())
}

pub async fn get_entry(op: &Operator, ws_path: &str, entry_id: &str) -> Result<Value> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
    let row = read_entry_row(op, ws_path, &form_name, entry_id).await?;
    if row.deleted {
        return Err(entry_not_found(entry_id).into());
    }

    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let field_order = form_field_names(&form_def);
    let merged_fields = merge_entry_fields(&row.fields, &row.extra_attributes);
    let markdown = render_markdown(
        &row.title,
        &form_name,
        &row.tags,
        &merged_fields,
        &field_order,
    );
    let frontmatter = serde_json::json!({
        "form": form_name,
        "tags": row.tags,
    });
    let sections = sections_from_fields(&merged_fields);

    Ok(serde_json::json!({
        "id": entry_id,
        "revision_id": row.revision_id,
        "content": markdown,
        "frontmatter": frontmatter,
        "sections": sections,
        "computed": Value::Object(Map::new()),
        "title": row.title,
        "form": row.form,
        "tags": row.tags,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "integrity": serde_json::to_value(row.integrity)?,
    }))
}

pub async fn get_entry_authorized(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    relation_scopes: &BTreeMap<String, EntryScope>,
) -> Result<Value> {
    let mut selected = None;
    for form_name in list_form_names(op, ws_path).await? {
        let Some(entry_scope) = relation_scopes.get(&form_name.to_ascii_lowercase()) else {
            continue;
        };
        match read_entry_row_authorized(op, ws_path, &form_name, entry_id, entry_scope).await {
            Ok(row) => {
                selected = Some((form_name, row));
                break;
            }
            Err(error)
                if error
                    .downcast_ref::<AppError>()
                    .is_some_and(|error| error.code() == ErrorCode::EntryNotFound) => {}
            Err(error) => return Err(error),
        }
    }
    let (form_name, row) = selected.ok_or_else(|| entry_not_found(entry_id))?;
    if row.deleted {
        return Err(entry_not_found(entry_id).into());
    }

    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let field_order = form_field_names(&form_def);
    let merged_fields = merge_entry_fields(&row.fields, &row.extra_attributes);
    let markdown = render_markdown(
        &row.title,
        &form_name,
        &row.tags,
        &merged_fields,
        &field_order,
    );
    let frontmatter = serde_json::json!({
        "form": form_name,
        "tags": row.tags,
    });
    let sections = sections_from_fields(&merged_fields);

    Ok(serde_json::json!({
        "id": entry_id,
        "revision_id": row.revision_id,
        "content": markdown,
        "frontmatter": frontmatter,
        "sections": sections,
        "computed": Value::Object(Map::new()),
        "title": row.title,
        "form": row.form,
        "tags": row.tags,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "integrity": serde_json::to_value(row.integrity)?,
    }))
}

pub async fn get_entry_content(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
) -> Result<EntryContent> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| entry_content_not_found(entry_id))?;
    let row = read_entry_row(op, ws_path, &form_name, entry_id).await?;
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let field_order = form_field_names(&form_def);
    let merged_fields = merge_entry_fields(&row.fields, &row.extra_attributes);
    let markdown = render_markdown(
        &row.title,
        &form_name,
        &row.tags,
        &merged_fields,
        &field_order,
    );
    Ok(EntryContent {
        revision_id: row.revision_id,
        parent_revision_id: row.parent_revision_id,
        timestamp: row.updated_at,
        author: row.author,
        markdown,
        frontmatter: serde_json::json!({
            "form": form_name,
            "tags": row.tags,
        }),
        sections: sections_from_fields(&merged_fields),
        computed: Value::Object(Map::new()),
    })
}

pub async fn get_entry_revision_content(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    revision_id: &str,
) -> Result<EntryContent> {
    let form_name = find_entry_form_with_deleted(op, ws_path, entry_id, true)
        .await?
        .ok_or_else(|| entry_content_not_found(entry_id))?;
    let row = read_entry_row(op, ws_path, &form_name, entry_id).await?;

    let (form_def, revisions) = revision_rows_for_form(op, ws_path, &form_name).await?;
    let revision = revisions
        .into_iter()
        .find(|rev| rev.entry_id == entry_id && rev.revision_id == revision_id)
        .ok_or_else(|| revision_not_found(entry_id, revision_id))?;

    let field_order = form_field_names(&form_def);
    let merged_fields = merge_entry_fields(&revision.fields, &revision.extra_attributes);
    let markdown = render_markdown(
        &row.title,
        &form_name,
        &row.tags,
        &merged_fields,
        &field_order,
    );
    Ok(EntryContent {
        revision_id: revision.revision_id,
        parent_revision_id: revision.parent_revision_id,
        timestamp: revision.timestamp,
        author: revision.author,
        markdown,
        frontmatter: serde_json::json!({
            "form": form_name,
            "tags": row.tags,
        }),
        sections: sections_from_fields(&merged_fields),
        computed: Value::Object(Map::new()),
    })
}

fn checkpoint_scope_for_form(
    form_id: FormId,
    form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
) -> EntryScope {
    form_scopes
        .and_then(|scopes| scopes.get(&form_id).cloned())
        .unwrap_or_else(|| {
            form_scopes
                .map(|_| EntryScope::Only(BTreeSet::new()))
                .unwrap_or(EntryScope::AllCurrent)
        })
}

async fn checkpoint_revisions_for_entry(
    workspace: &IcebergWorkspace,
    checkpoint: &SpaceCheckpoint,
    entry_id: &str,
    view: RevisionView,
    form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
) -> Result<Option<(ugoite_domain::form::FormDefinition, Vec<EntryRevision>)>> {
    let entry_uuid = Uuid::parse_str(entry_id)
        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, entry_id.as_bytes()))
        .into();
    for coordinate in &checkpoint.tables {
        let form = workspace
            .form_at_checkpoint(checkpoint, &sql_relation_name(coordinate.form_id))
            .await?;
        let scope =
            entry_scope_for_lookup(&checkpoint_scope_for_form(form.id, form_scopes), entry_id);
        if matches!(scope, EntryScope::Only(ref ids) if ids.is_empty()) {
            continue;
        }
        let revisions = workspace
            .read_revision_view_at_checkpoint_with_scope(checkpoint, form.id, scope, view)
            .await?
            .into_iter()
            .filter(|revision| {
                revision.entry_id == entry_uuid || revision.entry.external_id == entry_id
            })
            .collect::<Vec<_>>();
        if !revisions.is_empty() {
            return Ok(Some((form, revisions)));
        }
    }
    Ok(None)
}

fn entry_value_from_checkpoint_revision(
    entry_id: &str,
    form: &ugoite_domain::form::FormDefinition,
    revision: EntryRevision,
) -> Result<Value> {
    if revision.entry.deleted || revision.operation == EntryOperation::Delete {
        return Err(entry_not_found(entry_id).into());
    }
    let form_def = form::from_domain_form(form);
    let row = revision_row_from_domain(revision, &form.name, form)?.state;
    let row = row.ok_or_else(|| entry_not_found(entry_id))?;
    let merged_fields = merge_entry_fields(&row.fields, &row.extra_attributes);
    let markdown = render_markdown(
        &row.title,
        &form.name,
        &row.tags,
        &merged_fields,
        &form_field_names(&form_def),
    );
    Ok(json!({
        "id": entry_id,
        "revision_id": row.revision_id,
        "content": markdown,
        "frontmatter": {"form": form.name, "tags": row.tags},
        "sections": sections_from_fields(&merged_fields),
        "computed": Value::Object(Map::new()),
        "title": row.title,
        "form": row.form,
        "tags": row.tags,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "integrity": serde_json::to_value(row.integrity)?,
    }))
}

/// Reads the latest visible Entry state from a retained checkpoint. The
/// checkpoint itself supplies both the Form schema and the Iceberg snapshot.
pub async fn get_entry_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    checkpoint: &SpaceCheckpoint,
    form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
) -> Result<Value> {
    let workspace = iceberg_store::native_workspace(op, ws_path).await?;
    let Some((form, mut revisions)) = checkpoint_revisions_for_entry(
        &workspace,
        checkpoint,
        entry_id,
        RevisionView::LatestIncludingTombstones,
        form_scopes,
    )
    .await?
    else {
        return Err(entry_not_found(entry_id).into());
    };
    let revision = revisions.pop().ok_or_else(|| entry_not_found(entry_id))?;
    entry_value_from_checkpoint_revision(entry_id, &form, revision)
}

pub async fn get_entry_history_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    checkpoint: &SpaceCheckpoint,
    form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
) -> Result<Value> {
    let workspace = iceberg_store::native_workspace(op, ws_path).await?;
    let Some((_, mut revisions)) = checkpoint_revisions_for_entry(
        &workspace,
        checkpoint,
        entry_id,
        RevisionView::All,
        form_scopes,
    )
    .await?
    else {
        return Err(entry_not_found(entry_id).into());
    };
    revisions.sort_by_key(|revision| (revision.committed_at_micros, revision.revision_id));
    Ok(json!({
        "entry_id": entry_id,
        "revisions": revisions.into_iter().map(|revision| json!({
            "revision_id": revision.revision_id,
            "timestamp": from_timestamp_micros(revision.committed_at_micros),
            "checksum": revision.entry.integrity.checksum,
            "signature": revision.entry.integrity.signature,
            "entry_version": revision.entry_version,
            "operation": revision.operation,
            "source_kind": revision.source_kind,
            "source_id": revision.source_id,
            "restored_from": revision.entry.restored_from,
        })).collect::<Vec<_>>(),
    }))
}

pub async fn get_entry_revision_at_checkpoint(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    revision_id: &str,
    checkpoint: &SpaceCheckpoint,
    form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
) -> Result<Value> {
    let workspace = iceberg_store::native_workspace(op, ws_path).await?;
    let Some((form, revisions)) = checkpoint_revisions_for_entry(
        &workspace,
        checkpoint,
        entry_id,
        RevisionView::All,
        form_scopes,
    )
    .await?
    else {
        return Err(revision_not_found(entry_id, revision_id).into());
    };
    let revision = revisions
        .into_iter()
        .find(|revision| revision.revision_id.to_string() == revision_id)
        .ok_or_else(|| revision_not_found(entry_id, revision_id))?;
    let timestamp = from_timestamp_micros(revision.committed_at_micros);
    let row = revision_row_from_domain(revision, &form.name, &form)?
        .state
        .ok_or_else(|| revision_not_found(entry_id, revision_id))?;
    let form_def = form::from_domain_form(&form);
    let form_name = form.name;
    let merged_fields = merge_entry_fields(&row.fields, &row.extra_attributes);
    let markdown = render_markdown(
        &row.title,
        &form_name,
        &row.tags,
        &merged_fields,
        &form_field_names(&form_def),
    );
    Ok(serde_json::to_value(EntryContent {
        revision_id: row.revision_id,
        parent_revision_id: row.parent_revision_id,
        timestamp,
        author: row.author,
        markdown,
        frontmatter: json!({"form": form_name, "tags": row.tags}),
        sections: sections_from_fields(&merged_fields),
        computed: Value::Object(Map::new()),
    })?)
}

#[allow(clippy::too_many_arguments)]
pub async fn restore_entry_from_checkpoint_authorized<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    revision_id: &str,
    checkpoint: &SpaceCheckpoint,
    author: &str,
    integrity: &I,
    form_scopes: Option<&BTreeMap<FormId, EntryScope>>,
) -> Result<Value> {
    let workspace = iceberg_store::native_workspace(op, ws_path).await?;
    let form_name = find_entry_form_with_deleted(op, ws_path, entry_id, true)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let current_form = form::to_domain_form(&form_def)?;
    let scope = checkpoint_scope_for_form(current_form.id, form_scopes);
    if matches!(scope, EntryScope::Only(ref ids) if ids.is_empty()) {
        return Err(AppError::forbidden("Form is not readable").into());
    }
    let Some((_, source_revisions)) = checkpoint_revisions_for_entry(
        &workspace,
        checkpoint,
        entry_id,
        RevisionView::All,
        form_scopes,
    )
    .await?
    else {
        return Err(revision_not_found(entry_id, revision_id).into());
    };
    let source = source_revisions
        .into_iter()
        .find(|revision| revision.revision_id.to_string() == revision_id)
        .ok_or_else(|| revision_not_found(entry_id, revision_id))?;
    if source.form_id != current_form.id {
        return Err(revision_not_found(entry_id, revision_id).into());
    }

    let mut row = read_entry_row(op, ws_path, &form_name, entry_id).await?;
    let new_revision_id = Uuid::new_v4().to_string();
    let mut timestamp = now_ts();
    if timestamp <= row.updated_at {
        timestamp = row.updated_at + 0.001;
    }
    let values = current_form
        .fields
        .iter()
        .filter_map(|field| {
            source
                .values
                .get(&field.id)
                .map(|value| Ok((field.name.clone(), serde_json::to_value(value)?)))
        })
        .collect::<Result<Map<String, Value>>>()?;
    let merged_fields = merge_entry_fields(
        &Value::Object(values.clone()),
        &serde_json::to_value(&source.extra_attributes)?,
    );
    let markdown = render_markdown(
        &source.entry.title,
        &form_name,
        &source.entry.tags,
        &merged_fields,
        &form_field_names(&form_def),
    );
    let checksum = integrity.checksum(&markdown);
    let signature = integrity.signature(&markdown);
    row.title = source.entry.title.clone();
    row.tags = source.entry.tags.clone();
    row.updated_at = timestamp;
    row.fields = Value::Object(values);
    row.extra_attributes = serde_json::to_value(&source.extra_attributes)?;
    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = new_revision_id.clone();
    row.entry_version = row.entry_version.saturating_add(1);
    row.author = author.to_string();
    row.deleted = false;
    row.deleted_at = None;
    row.integrity = IntegrityPayload {
        checksum: checksum.clone(),
        signature: signature.clone(),
    };
    let entry_version = row.entry_version;
    let relation_scopes = form_scopes
        .and_then(|scopes| scopes.get(&current_form.id).cloned())
        .map(|scope| BTreeMap::from([(form_name.to_ascii_lowercase(), scope)]));
    let restore_revision = RevisionRow {
        revision_id: new_revision_id.clone(),
        entry_id: entry_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: checksum.clone(),
        integrity: row.integrity.clone(),
        restored_from: Some(revision_id.to_string()),
        state: Some(row),
        entry_version,
        operation: "restore".to_string(),
        source_kind: "checkpoint_restore".to_string(),
        source_id: Some(revision_id.to_string()),
        extension_metadata: json!({
            "restore_source_checkpoint": {
                "name": checkpoint.name,
                "coordinate_checksum": checkpoint.coordinate_checksum,
            },
            "restore_source_revision_id": revision_id,
            "restore_author": author,
        }),
    };
    append_revision_row_for_form_authorized(
        op,
        ws_path,
        &form_name,
        &restore_revision,
        &form_def,
        relation_scopes.as_ref(),
    )
    .await?;
    Ok(json!({
        "revision_id": new_revision_id,
        "restored_from": revision_id,
        "source_checkpoint": {
            "name": checkpoint.name,
            "coordinate_checksum": checkpoint.coordinate_checksum,
        },
        "source_revision_id": revision_id,
        "author": author,
        "timestamp": timestamp,
    }))
}

#[allow(clippy::too_many_arguments)]
pub async fn update_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
    parent_revision_id: Option<&str>,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    update_entry_authorized(
        op,
        ws_path,
        entry_id,
        content,
        parent_revision_id,
        author,
        integrity,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn update_entry_authorized<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
    parent_revision_id: Option<&str>,
    author: &str,
    integrity: &I,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
) -> Result<Value> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
    if let Some(scopes) = relation_scopes {
        if !scopes.contains_key(&form_name.to_ascii_lowercase()) {
            return Err(AppError::forbidden("Form is not readable").into());
        }
    }
    let mut row = read_entry_row(op, ws_path, &form_name, entry_id).await?;

    if let Some(expected_parent) = parent_revision_id {
        if row.revision_id != expected_parent {
            return Err(AppError::conflict(
                ErrorCode::RevisionConflict,
                format!(
                    "Revision conflict: expected {}, got {}",
                    expected_parent, row.revision_id
                ),
            )
            .into());
        }
    }

    let (frontmatter, sections) = parse_markdown(content);
    let updated_form = extract_form(&frontmatter)
        .ok_or_else(|| invalid_entry_input("Form is required for entry update"))?;
    if updated_form != form_name {
        return Err(invalid_entry_input("Form change is not supported"));
    }

    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let form_fields = form_field_names(&form_def);
    let form_set: HashSet<String> = form_fields.iter().cloned().collect();
    let policy = extra_attributes_policy(&form_def);
    let (extras, extra_attributes) = collect_extra_attributes(&sections, &form_set);
    if !extras.is_empty() && policy == ExtraAttributesPolicy::Deny {
        return Err(AppError::invalid_input_with_detail(
            ErrorCode::UnknownFormFields,
            "Entry contains unknown form fields",
            json!({"fields": extras}),
        )
        .into());
    }

    let properties = index::extract_properties(content);
    let (casted, warnings) = index::validate_properties(&properties, &form_def)?;
    if !warnings.is_empty() {
        return Err(AppError::invalid_input_with_detail(
            ErrorCode::FormValidationFailed,
            "Entry form validation failed",
            json!({"warnings": warnings}),
        )
        .into());
    }

    let mut fields = Map::new();
    if let Some(obj) = properties.as_object() {
        for (key, value) in obj {
            if form_set.contains(key) {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(obj) = casted.as_object() {
        for (key, value) in obj {
            if form_set.contains(key) {
                fields.insert(key.clone(), value.clone());
            }
        }
    }

    let mut timestamp = now_ts();
    if timestamp <= row.updated_at {
        timestamp = row.updated_at + 0.001;
    }
    let revision_id = Uuid::new_v4().to_string();
    let checksum = integrity.checksum(content);
    let signature = integrity.signature(content);

    row.title = extract_title(content, &row.title);
    row.updated_at = timestamp;
    if frontmatter.get("tags").is_some() {
        row.tags = extract_tags(&frontmatter);
    }
    row.fields = Value::Object(fields);
    row.extra_attributes = extra_attributes.clone();
    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = revision_id.clone();
    row.entry_version = row.entry_version.saturating_add(1);
    row.author = author.to_string();
    row.integrity = IntegrityPayload {
        checksum: checksum.clone(),
        signature: signature.clone(),
    };

    let revision = RevisionRow {
        revision_id: revision_id.clone(),
        entry_id: entry_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: checksum.clone(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        restored_from: None,
        state: Some(row.clone()),
        entry_version: row.entry_version,
        operation: "upsert".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
        extension_metadata: Value::Object(Map::new()),
    };
    append_revision_row_for_form_authorized(
        op,
        ws_path,
        &form_name,
        &revision,
        &form_def,
        relation_scopes,
    )
    .await?;

    get_entry(op, ws_path, entry_id).await
}

pub async fn delete_entry(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    hard_delete: bool,
) -> Result<()> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
    let mut row = read_entry_row(op, ws_path, &form_name, entry_id).await?;

    let mut delete_ts = now_ts();
    if delete_ts <= row.updated_at {
        delete_ts = row.updated_at + 0.001;
    }
    let _ = hard_delete;
    let previous_revision_id = row.revision_id.clone();
    row.deleted = true;
    row.deleted_at = Some(delete_ts);
    row.updated_at = delete_ts;
    row.parent_revision_id = Some(previous_revision_id);
    row.revision_id = Uuid::new_v4().to_string();
    row.entry_version = row.entry_version.saturating_add(1);
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let tombstone = RevisionRow {
        revision_id: row.revision_id.clone(),
        entry_id: entry_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp: delete_ts,
        author: row.author.clone(),
        fields: Value::Object(Map::new()),
        extra_attributes: Value::Object(Map::new()),
        markdown_checksum: row.integrity.checksum.clone(),
        integrity: row.integrity.clone(),
        restored_from: None,
        state: Some(row.clone()),
        entry_version: row.entry_version,
        operation: "delete".to_string(),
        source_kind: "api".to_string(),
        source_id: None,
        extension_metadata: Value::Object(Map::new()),
    };
    append_revision_row_for_form(op, ws_path, &form_name, &tombstone, &form_def).await?;
    Ok(())
}

pub async fn get_entry_history(op: &Operator, ws_path: &str, entry_id: &str) -> Result<Value> {
    let form_name = find_entry_form_with_deleted(op, ws_path, entry_id, true)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
    let (_, rows) = revision_rows_for_form(op, ws_path, &form_name).await?;

    let mut revisions = rows
        .into_iter()
        .filter(|rev| rev.entry_id == entry_id)
        .map(|rev| {
            serde_json::json!({
                "revision_id": rev.revision_id,
                "timestamp": rev.timestamp,
                "checksum": rev.integrity.checksum,
                "signature": rev.integrity.signature,
            })
        })
        .collect::<Vec<_>>();

    revisions.sort_by(|a, b| {
        let a_ts = a.get("timestamp").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b_ts = b.get("timestamp").and_then(|v| v.as_f64()).unwrap_or(0.0);
        a_ts.partial_cmp(&b_ts).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(serde_json::json!({
        "entry_id": entry_id,
        "revisions": revisions,
    }))
}

pub async fn get_entry_revision(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    revision_id: &str,
) -> Result<Value> {
    let form_name = find_entry_form_with_deleted(op, ws_path, entry_id, true)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
    let (_, rows) = revision_rows_for_form(op, ws_path, &form_name).await?;
    let revision = rows
        .into_iter()
        .find(|rev| rev.entry_id == entry_id && rev.revision_id == revision_id);

    let revision = revision.ok_or_else(|| revision_not_found(entry_id, revision_id))?;
    Ok(serde_json::to_value(revision)?)
}

pub async fn restore_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    revision_id: &str,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    restore_entry_authorized(op, ws_path, entry_id, revision_id, author, integrity, None).await
}

#[allow(clippy::too_many_arguments)]
pub async fn restore_entry_authorized<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    revision_id: &str,
    author: &str,
    integrity: &I,
    relation_scopes: Option<&BTreeMap<String, ugoite_core::query::EntryScope>>,
) -> Result<Value> {
    let form_name = find_entry_form_with_deleted(op, ws_path, entry_id, true)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
    if let Some(scopes) = relation_scopes {
        if !scopes.contains_key(&form_name.to_ascii_lowercase()) {
            return Err(AppError::forbidden("Form is not readable").into());
        }
    }
    let (form_def, revisions) = revision_rows_for_form(op, ws_path, &form_name).await?;
    let revision = revisions
        .into_iter()
        .find(|rev| rev.entry_id == entry_id && rev.revision_id == revision_id)
        .ok_or_else(|| revision_not_found(entry_id, revision_id))?;

    let mut row = read_entry_row(op, ws_path, &form_name, entry_id).await?;
    let new_rev_id = Uuid::new_v4().to_string();
    let mut timestamp = now_ts();
    if timestamp <= row.updated_at {
        timestamp = row.updated_at + 0.001;
    }

    let field_order = form_field_names(&form_def);
    let merged_fields = merge_entry_fields(&revision.fields, &revision.extra_attributes);
    let markdown = render_markdown(
        &row.title,
        &form_name,
        &row.tags,
        &merged_fields,
        &field_order,
    );
    let checksum = integrity.checksum(&markdown);
    let signature = integrity.signature(&markdown);

    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = new_rev_id.clone();
    row.entry_version = row.entry_version.saturating_add(1);
    row.updated_at = timestamp;
    row.fields = revision.fields.clone();
    row.extra_attributes = revision.extra_attributes.clone();
    row.deleted = false;
    row.deleted_at = None;
    row.integrity = IntegrityPayload {
        checksum: checksum.clone(),
        signature: signature.clone(),
    };
    row.author = author.to_string();
    let restore_revision = RevisionRow {
        revision_id: new_rev_id.clone(),
        entry_id: entry_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        extra_attributes: row.extra_attributes.clone(),
        markdown_checksum: checksum.clone(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        restored_from: Some(revision_id.to_string()),
        state: Some(row.clone()),
        entry_version: row.entry_version,
        operation: "restore".to_string(),
        source_kind: "api".to_string(),
        source_id: Some(revision_id.to_string()),
        extension_metadata: Value::Object(Map::new()),
    };
    append_revision_row_for_form_authorized(
        op,
        ws_path,
        &form_name,
        &restore_revision,
        &form_def,
        relation_scopes,
    )
    .await?;

    Ok(serde_json::json!({
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }))
}

#[cfg(test)]
mod input_conversion_tests {
    use super::*;
    use ugoite_domain::form::ListItemDefinition;

    fn field(field_type: FieldType, list_item: Option<FieldType>) -> FormField {
        FormField {
            id: ugoite_domain::id::FieldId::new(100).expect("valid test field id"),
            name: "value".into(),
            field_type,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: list_item.map(|field_type| ListItemDefinition {
                field_type,
                reference_form: None,
            }),
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }
    }

    #[test]
    fn transport_json_is_canonicalized_by_scalar_and_list_type() {
        assert_eq!(
            json_to_field_value_for_field(&serde_json::json!(7), &field(FieldType::Integer, None))
                .unwrap(),
            FieldValue::Integer(7)
        );
        assert_eq!(
            json_to_field_value_for_field(&serde_json::json!(7), &field(FieldType::Long, None))
                .unwrap(),
            FieldValue::Integer(7)
        );
        assert_eq!(
            json_to_field_value_for_field(
                &serde_json::json!("A7F9F5D2-8B7E-4DB1-9B0A-0E9A2B3F4C5D"),
                &field(FieldType::Uuid, None),
            )
            .unwrap(),
            FieldValue::String("a7f9f5d2-8b7e-4db1-9b0a-0e9a2b3f4c5d".into())
        );
        assert_eq!(
            json_to_field_value_for_field(
                &serde_json::json!("base64:ZGF0YQ=="),
                &field(FieldType::Binary, None),
            )
            .unwrap(),
            FieldValue::String("base64:ZGF0YQ==".into())
        );
        assert_eq!(
            json_to_field_value_for_field(
                &serde_json::json!([7, null, 8]),
                &field(FieldType::List, Some(FieldType::Integer)),
            )
            .unwrap(),
            FieldValue::List(vec![
                FieldValue::Integer(7),
                FieldValue::Null,
                FieldValue::Integer(8),
            ])
        );
        assert_eq!(
            json_to_field_value_for_field(
                &serde_json::json!(["base64:ZGF0YQ==", null]),
                &field(FieldType::List, Some(FieldType::Binary)),
            )
            .unwrap(),
            FieldValue::List(vec![
                FieldValue::String("base64:ZGF0YQ==".into()),
                FieldValue::Null,
            ])
        );
        assert_eq!(
            json_to_field_value_for_field(
                &serde_json::json!("12:34"),
                &field(FieldType::Time, None),
            )
            .unwrap(),
            FieldValue::String("12:34:00".into())
        );
        assert_eq!(
            json_to_field_value_for_field(
                &serde_json::json!("2025-01-02T03:04:05.123456789Z"),
                &field(FieldType::TimestampTzNs, None),
            )
            .unwrap(),
            FieldValue::String("2025-01-02T03:04:05.123456789+00:00".into())
        );
    }
}
