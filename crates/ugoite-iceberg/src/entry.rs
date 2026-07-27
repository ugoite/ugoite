use crate::form;
use crate::iceberg_store;
use crate::index;
use crate::integrity::IntegrityProvider;
use crate::link::Link;
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use opendal::Operator;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::entry::{
    EntryAsset, EntryIntegrity, EntryLink, EntryMetadata, EntryOperation, EntryRevision, FieldValue,
};
use ugoite_domain::id::{FieldId, RevisionId};
use url::Url;
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
    pub author: String,
    pub markdown: String,
    #[serde(default)]
    pub frontmatter: Value,
    #[serde(default)]
    pub sections: Value,
    #[serde(default)]
    pub assets: Vec<Value>,
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
    pub links: Vec<Link>,
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
    #[serde(default)]
    pub links: Vec<Link>,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(default)]
    pub fields: Value,
    #[serde(default)]
    pub extra_attributes: Value,
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    #[serde(default)]
    pub assets: Vec<Value>,
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

fn normalize_ugoite_links(content: &str) -> String {
    let re = Regex::new(r#"ugoite://[^\s)]+"#).unwrap();
    re.replace_all(content, |caps: &regex::Captures| {
        normalize_ugoite_link(caps.get(0).map(|m| m.as_str()).unwrap_or(""))
    })
    .to_string()
}

fn normalize_ugoite_link(raw: &str) -> String {
    let Ok(url) = Url::parse(raw) else {
        return raw.to_string();
    };
    let kind = url.host_str().unwrap_or("").to_lowercase();
    let canonical_kind = match kind.as_str() {
        "entries" | "entry" => "entry",
        "assets" | "asset" => "asset",
        _ => kind.as_str(),
    };
    let mut path = url.path().trim_start_matches('/').to_string();
    if path.is_empty() {
        for (key, value) in url.query_pairs() {
            if key.eq_ignore_ascii_case("id") && !value.is_empty() {
                path = value.to_string();
                break;
            }
        }
    }
    if path.is_empty() || canonical_kind.is_empty() {
        return raw.to_string();
    }
    format!("ugoite://{}/{}", canonical_kind, path)
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

async fn append_revision_row_to_table(
    op: &Operator,
    ws_path: &str,
    row: &RevisionRow,
    form_def: &Value,
) -> Result<()> {
    append_revision_rows_to_workspace(op, ws_path, std::slice::from_ref(row), form_def).await
}

async fn append_revision_rows_to_workspace(
    op: &Operator,
    ws_path: &str,
    rows: &[RevisionRow],
    form_def: &Value,
) -> Result<()> {
    if rows.is_empty() {
        return Err(anyhow!("revision batch must not be empty"));
    }
    let domain_form = form::to_domain_form(form_def)?;
    let revisions = rows
        .iter()
        .map(|row| revision_row_to_domain(row, &domain_form))
        .collect::<Result<Vec<_>>>()?;
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
        .append_revisions(domain_form.id, revisions)
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
        extension_metadata: Default::default(),
    })
}

fn entry_metadata_from_row(row: &EntryRow) -> EntryMetadata {
    EntryMetadata {
        external_id: row.entry_id.clone(),
        title: row.title.clone(),
        tags: row.tags.clone(),
        links: row
            .links
            .iter()
            .map(|link| EntryLink {
                id: link.id.clone(),
                target: link.target.clone(),
                kind: link.kind.clone(),
            })
            .collect(),
        created_at_micros: to_timestamp_micros(row.created_at),
        updated_at_micros: to_timestamp_micros(row.updated_at),
        assets: row
            .assets
            .iter()
            .filter_map(|asset| {
                let object = asset.as_object()?;
                Some(EntryAsset {
                    id: object.get("id")?.as_str()?.to_string(),
                    name: object.get("name")?.as_str()?.to_string(),
                    path: object.get("path")?.as_str()?.to_string(),
                })
            })
            .collect(),
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
            values.insert(field.id, json_to_field_value(value)?);
        }
    }
    Ok(values)
}

fn json_to_field_value(value: &Value) -> Result<FieldValue> {
    Ok(match value {
        Value::Null => FieldValue::Null,
        Value::Bool(value) => FieldValue::Boolean(*value),
        Value::String(value) => FieldValue::String(value.clone()),
        Value::Number(value) => FieldValue::Number(value.as_f64().context("invalid number")?),
        Value::Array(values) => FieldValue::List(
            values
                .iter()
                .map(json_to_field_value)
                .collect::<Result<Vec<_>>>()?,
        ),
        Value::Object(values) => FieldValue::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), json_to_field_value(value)?)))
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
    let links = revision
        .entry
        .links
        .iter()
        .map(|link| Link {
            id: link.id.clone(),
            source: if revision.entry.external_id.is_empty() {
                revision.entry_id.to_string()
            } else {
                revision.entry.external_id.clone()
            },
            target: link.target.clone(),
            kind: link.kind.clone(),
        })
        .collect::<Vec<_>>();
    let assets = revision
        .entry
        .assets
        .iter()
        .map(serde_json::to_value)
        .collect::<Result<Vec<_>, _>>()?;
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
        links,
        created_at: from_timestamp_micros(revision.entry.created_at_micros),
        updated_at: from_timestamp_micros(revision.entry.updated_at_micros),
        fields: Value::Object(fields.clone()),
        extra_attributes: serde_json::to_value(&revision.extra_attributes)?,
        revision_id: revision.revision_id.to_string(),
        parent_revision_id: revision.parent_revision_id.map(|id| id.to_string()),
        assets,
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
    let rows = list_entry_rows(op, ws_path).await?;
    Ok(rows
        .into_iter()
        .find(|(_, row)| row.entry_id == entry_id)
        .map(|(form_name, _)| form_name))
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

pub(crate) async fn write_entry_row(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    entry_id: &str,
    row: &EntryRow,
) -> Result<()> {
    let mut state = row.clone();
    state.parent_revision_id = Some(row.revision_id.clone());
    state.revision_id = Uuid::new_v4().to_string();
    state.entry_version = row.entry_version.saturating_add(1);
    let form_def = form::read_form_definition(op, ws_path, form_name).await?;
    let revision = RevisionRow {
        revision_id: state.revision_id.clone(),
        entry_id: entry_id.to_string(),
        parent_revision_id: state.parent_revision_id.clone(),
        timestamp: state.updated_at,
        author: state.author.clone(),
        fields: state.fields.clone(),
        extra_attributes: state.extra_attributes.clone(),
        markdown_checksum: state.integrity.checksum.clone(),
        integrity: state.integrity.clone(),
        restored_from: None,
        state: Some(state.clone()),
        entry_version: state.entry_version,
        operation: if state.deleted { "delete" } else { "upsert" }.to_string(),
        source_kind: "application".to_string(),
        source_id: None,
    };
    append_revision_row_for_form(op, ws_path, form_name, &revision, &form_def).await
}

pub(crate) async fn list_entry_rows(
    op: &Operator,
    ws_path: &str,
) -> Result<Vec<(String, EntryRow)>> {
    let mut latest: std::collections::HashMap<String, (String, RevisionRow)> =
        std::collections::HashMap::new();
    for form_name in list_form_names(op, ws_path).await? {
        let (form, revisions) =
            iceberg_store::latest_revisions_for_form(op, ws_path, &form_name).await?;
        let rows = revisions
            .into_iter()
            .map(|revision| revision_row_from_domain(revision, &form_name, &form))
            .collect::<Result<Vec<_>>>()?;
        for revision in rows {
            let Some(row) = revision.state.as_ref() else {
                continue;
            };
            let entry = latest.get(&row.entry_id);
            if let Some((_, existing)) = entry {
                if revision.entry_version == existing.entry_version
                    && revision.revision_id != existing.revision_id
                {
                    return Err(anyhow!(
                        "multiple revisions exist for entry {} at version {}",
                        row.entry_id,
                        revision.entry_version
                    ));
                }
            }
            let should_replace = match entry {
                Some((_, existing)) => revision.entry_version > existing.entry_version,
                None => true,
            };
            if should_replace {
                latest.insert(row.entry_id.clone(), (form_name.clone(), revision));
            }
        }
    }
    Ok(latest
        .into_values()
        .filter_map(|(form_name, revision)| revision.state.map(|row| (form_name, row)))
        .collect())
}

pub(crate) async fn list_form_entry_rows(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    _form_def: &Value,
) -> Result<Vec<EntryRow>> {
    let (form, revisions) =
        iceberg_store::latest_revisions_for_form(op, ws_path, form_name).await?;
    Ok(revisions
        .into_iter()
        .map(|revision| revision_row_from_domain(revision, form_name, &form))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|revision| revision.state)
        .collect())
}

#[allow(dead_code)] // used by migration verification tooling
pub(crate) async fn list_form_revision_rows(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    _form_def: &Value,
) -> Result<Vec<RevisionRow>> {
    let (_, rows) = revision_rows_for_form(op, ws_path, form_name).await?;
    Ok(rows)
}

pub(crate) async fn append_revision_row_for_form(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    row: &RevisionRow,
    form_def: &Value,
) -> Result<()> {
    let _ = form_name;
    append_revision_row_to_table(op, ws_path, row, form_def).await
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
    let mut entries = create_entries(
        op,
        ws_path,
        vec![EntryCreateRequest::new(entry_id, content)],
        author,
        integrity,
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
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    if requests.len() > MAX_ENTRY_CREATE_BATCH_SIZE {
        return Err(anyhow!(
            "entry create batches are limited to {MAX_ENTRY_CREATE_BATCH_SIZE} requests"
        ));
    }
    let mut known_entry_ids = list_entry_rows(op, ws_path)
        .await?
        .into_iter()
        .map(|(_, row)| row.entry_id)
        .collect::<HashSet<_>>();
    let mut batches = BTreeMap::<String, (Value, Vec<RevisionRow>)>::new();
    let mut entries = Vec::with_capacity(requests.len());
    for request in requests {
        if !known_entry_ids.insert(request.entry_id.clone()) {
            return Err(anyhow!("Entry already exists: {}", request.entry_id));
        }
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
    for (_, (form_def, revisions)) in batches {
        append_revision_rows_to_workspace(op, ws_path, &revisions, &form_def).await?;
    }
    Ok(entries)
}

async fn prepare_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
    author: &str,
    integrity: &I,
) -> Result<(EntryMeta, String, Value, RevisionRow)> {
    let normalized_content = normalize_ugoite_links(content);
    let (frontmatter, sections) = parse_markdown(&normalized_content);
    let form_name =
        extract_form(&frontmatter).ok_or_else(|| anyhow!("Form is required for entry creation"))?;
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;

    let form_fields = form_field_names(&form_def);
    let form_set: HashSet<String> = form_fields.iter().cloned().collect();
    let policy = extra_attributes_policy(&form_def);
    let (extras, extra_attributes) = collect_extra_attributes(&sections, &form_set);
    if !extras.is_empty() && policy == ExtraAttributesPolicy::Deny {
        return Err(anyhow!("Unknown form fields: {}", extras.join(", ")));
    }

    let properties = index::extract_properties(&normalized_content);
    let (casted, warnings) = index::validate_properties(&properties, &form_def)?;
    if !warnings.is_empty() {
        return Err(anyhow!(
            "Form validation failed: {}",
            serde_json::to_string(&warnings)?
        ));
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

    let title = extract_title(&normalized_content, entry_id);
    let tags = extract_tags(&frontmatter);
    let timestamp = now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let checksum = integrity.checksum(&normalized_content);
    let signature = integrity.signature(&normalized_content);

    let entry_row = EntryRow {
        entry_id: entry_id.to_string(),
        title: title.clone(),
        form: form_name.clone(),
        tags,
        links: Vec::new(),
        created_at: timestamp,
        updated_at: timestamp,
        fields: Value::Object(fields),
        extra_attributes: extra_attributes.clone(),
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        assets: Vec::new(),
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
        links: entry_row.links.clone(),
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
    let mut entries = Vec::new();
    for (form_name, row) in list_entry_rows(op, ws_path).await? {
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
            "links": row.links,
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
    list_entry_summaries_authorized(op, ws_path, form_filter, query, limit, None).await
}

pub async fn list_entry_summaries_authorized(
    op: &Operator,
    ws_path: &str,
    form_filter: Option<&str>,
    query: Option<&str>,
    limit: usize,
    readable_entry_ids: Option<&std::collections::HashSet<String>>,
) -> Result<Vec<EntrySummary>> {
    let normalized_form = form_filter.map(str::trim).filter(|value| !value.is_empty());
    let normalized_query = query
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_lowercase);
    let mut entries = Vec::new();
    for (form_name, row) in list_entry_rows(op, ws_path).await? {
        if row.deleted || readable_entry_ids.is_some_and(|allowed| !allowed.contains(&row.entry_id))
        {
            continue;
        }
        if let Some(expected_form) = normalized_form {
            if form_name != expected_form {
                continue;
            }
        }
        if let Some(expected_query) = normalized_query.as_deref() {
            let search_text = format!("{}\n{}", row.title, row.entry_id).to_lowercase();
            if !search_text.contains(expected_query) {
                continue;
            }
        }
        entries.push(EntrySummary {
            id: row.entry_id,
            title: row.title,
            form: form_name,
        });
    }
    entries.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
    });
    entries.truncate(limit);
    Ok(entries)
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
        "assets": row.assets,
        "computed": Value::Object(Map::new()),
        "title": row.title,
        "form": row.form,
        "tags": row.tags,
        "links": row.links,
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
        author: row.author,
        markdown,
        frontmatter: serde_json::json!({
            "form": form_name,
            "tags": row.tags,
        }),
        sections: sections_from_fields(&merged_fields),
        assets: row.assets,
        computed: Value::Object(Map::new()),
    })
}

pub async fn get_entry_revision_content(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    revision_id: &str,
) -> Result<EntryContent> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| entry_content_not_found(entry_id))?;
    let row = read_entry_row(op, ws_path, &form_name, entry_id).await?;
    if row.deleted {
        return Err(entry_content_not_found(entry_id).into());
    }

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
        author: revision.author,
        markdown,
        frontmatter: serde_json::json!({
            "form": form_name,
            "tags": row.tags,
        }),
        sections: sections_from_fields(&merged_fields),
        assets: Vec::new(),
        computed: Value::Object(Map::new()),
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn update_entry<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    entry_id: &str,
    content: &str,
    parent_revision_id: Option<&str>,
    author: &str,
    assets: Option<Vec<Value>>,
    integrity: &I,
) -> Result<Value> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
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

    let normalized_content = normalize_ugoite_links(content);
    let (frontmatter, sections) = parse_markdown(&normalized_content);
    let updated_form =
        extract_form(&frontmatter).ok_or_else(|| anyhow!("Form is required for entry update"))?;
    if updated_form != form_name {
        return Err(anyhow!("Form change is not supported"));
    }

    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let form_fields = form_field_names(&form_def);
    let form_set: HashSet<String> = form_fields.iter().cloned().collect();
    let policy = extra_attributes_policy(&form_def);
    let (extras, extra_attributes) = collect_extra_attributes(&sections, &form_set);
    if !extras.is_empty() && policy == ExtraAttributesPolicy::Deny {
        return Err(anyhow!("Unknown form fields: {}", extras.join(", ")));
    }

    let properties = index::extract_properties(&normalized_content);
    let (casted, warnings) = index::validate_properties(&properties, &form_def)?;
    if !warnings.is_empty() {
        return Err(anyhow!(
            "Form validation failed: {}",
            serde_json::to_string(&warnings)?
        ));
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
    let checksum = integrity.checksum(&normalized_content);
    let signature = integrity.signature(&normalized_content);

    row.title = extract_title(&normalized_content, &row.title);
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
    row.assets = assets.unwrap_or_else(|| row.assets.clone());

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
    };
    append_revision_row_for_form(op, ws_path, &form_name, &revision, &form_def).await?;

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
    };
    append_revision_row_for_form(op, ws_path, &form_name, &tombstone, &form_def).await?;
    Ok(())
}

pub async fn get_entry_history(op: &Operator, ws_path: &str, entry_id: &str) -> Result<Value> {
    let form_name = find_entry_form(op, ws_path, entry_id)
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
    let form_name = find_entry_form(op, ws_path, entry_id)
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
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| entry_not_found(entry_id))?;
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
    };
    append_revision_row_for_form(op, ws_path, &form_name, &restore_revision, &form_def).await?;

    Ok(serde_json::json!({
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }))
}
