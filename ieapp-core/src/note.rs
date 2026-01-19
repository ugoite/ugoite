use crate::class;
use crate::index;
use crate::integrity::IntegrityProvider;
use crate::link::Link;
use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::TryStreamExt;
use opendal::{EntryMode, Operator};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct IntegrityPayload {
    #[serde(default)]
    pub checksum: String,
    #[serde(default)]
    pub signature: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NoteContent {
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    pub author: String,
    pub markdown: String,
    #[serde(default)]
    pub frontmatter: Value,
    #[serde(default)]
    pub sections: Value,
    #[serde(default)]
    pub attachments: Vec<Value>,
    #[serde(default)]
    pub computed: Value,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NoteMeta {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub workspace_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub canvas_position: Value,
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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NoteRow {
    pub note_id: String,
    pub title: String,
    pub class: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub links: Vec<Link>,
    #[serde(default)]
    pub canvas_position: Value,
    pub created_at: f64,
    pub updated_at: f64,
    #[serde(default)]
    pub fields: Value,
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    #[serde(default)]
    pub attachments: Vec<Value>,
    #[serde(default)]
    pub integrity: IntegrityPayload,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub deleted_at: Option<f64>,
    #[serde(default)]
    pub author: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RevisionRow {
    pub revision_id: String,
    pub note_id: String,
    pub parent_revision_id: Option<String>,
    pub timestamp: f64,
    pub author: String,
    #[serde(default)]
    pub fields: Value,
    pub markdown_checksum: String,
    #[serde(default)]
    pub integrity: IntegrityPayload,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_from: Option<String>,
}

fn now_ts() -> f64 {
    Utc::now().timestamp_millis() as f64 / 1000.0
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

fn class_dir(ws_path: &str, class_name: &str) -> String {
    format!("{}/classes/{}", ws_path, class_name)
}

fn class_notes_dir(ws_path: &str, class_name: &str) -> String {
    format!("{}/notes", class_dir(ws_path, class_name))
}

fn class_revisions_dir(ws_path: &str, class_name: &str) -> String {
    format!("{}/revisions", class_dir(ws_path, class_name))
}

fn note_row_path(ws_path: &str, class_name: &str, note_id: &str) -> String {
    format!("{}/{}.json", class_notes_dir(ws_path, class_name), note_id)
}

fn revision_row_path(ws_path: &str, class_name: &str, revision_id: &str) -> String {
    format!(
        "{}/{}.json",
        class_revisions_dir(ws_path, class_name),
        revision_id
    )
}

fn class_field_names(class_def: &Value) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(fields) = class_def.get("fields") {
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

fn render_frontmatter(class_name: &str, tags: &[String]) -> String {
    let mut frontmatter = String::from("---\n");
    frontmatter.push_str(&format!("class: {}\n", class_name));
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
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::String(s) => format!("- {}", s),
                Value::Number(n) => format!("- {}", n),
                Value::Bool(b) => format!("- {}", b),
                _ => "-".to_string(),
            })
            .collect::<Vec<String>>()
            .join("\n"),
        Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn render_markdown(
    title: &str,
    class_name: &str,
    tags: &[String],
    fields: &Value,
    field_order: &[String],
) -> String {
    let mut markdown = String::new();
    markdown.push_str(&render_frontmatter(class_name, tags));
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
        for (name, value) in map {
            if !seen.contains(name) {
                ordered_fields.push((name.clone(), value.clone()));
            }
        }
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

async fn ensure_class_storage(op: &Operator, ws_path: &str, class_name: &str) -> Result<()> {
    let base = class_dir(ws_path, class_name);
    op.create_dir(&format!("{}/", base)).await?;
    op.create_dir(&format!("{}/notes/", base)).await?;
    op.create_dir(&format!("{}/revisions/", base)).await?;
    Ok(())
}

pub(crate) async fn list_class_names(op: &Operator, ws_path: &str) -> Result<Vec<String>> {
    class::list_class_names(op, ws_path).await
}

pub(crate) async fn find_note_class(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
) -> Result<Option<String>> {
    for class_name in list_class_names(op, ws_path).await? {
        let path = note_row_path(ws_path, &class_name, note_id);
        if op.exists(&path).await? {
            return Ok(Some(class_name));
        }
    }
    Ok(None)
}

pub(crate) async fn read_note_row(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    note_id: &str,
) -> Result<NoteRow> {
    let path = note_row_path(ws_path, class_name, note_id);
    let bytes = op.read(&path).await?;
    Ok(serde_json::from_slice(&bytes.to_vec())?)
}

pub(crate) async fn write_note_row(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    note_id: &str,
    row: &NoteRow,
) -> Result<()> {
    let path = note_row_path(ws_path, class_name, note_id);
    let bytes = serde_json::to_vec_pretty(row)?;
    op.write(&path, bytes).await?;
    Ok(())
}

pub(crate) async fn list_note_rows(op: &Operator, ws_path: &str) -> Result<Vec<(String, NoteRow)>> {
    let mut rows = Vec::new();
    for class_name in list_class_names(op, ws_path).await? {
        let notes_dir = format!("{}/", class_notes_dir(ws_path, &class_name));
        if !op.exists(&notes_dir).await? {
            continue;
        }
        let mut lister = op.lister(&notes_dir).await?;
        while let Some(entry) = lister.try_next().await? {
            if entry.metadata().mode() != EntryMode::FILE {
                continue;
            }
            let note_id = entry
                .name()
                .trim_end_matches(".json")
                .split('/')
                .next_back()
                .unwrap_or("");
            if note_id.is_empty() {
                continue;
            }
            if let Ok(row) = read_note_row(op, ws_path, &class_name, note_id).await {
                rows.push((class_name.clone(), row));
            }
        }
    }
    Ok(rows)
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

fn extract_class(frontmatter: &Value) -> Option<String> {
    frontmatter
        .get("class")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

async fn read_json(op: &Operator, path: &str) -> Result<Value> {
    let bytes = op.read(path).await?;
    let value = serde_json::from_slice(&bytes.to_vec())?;
    Ok(value)
}

async fn write_json<T: Serialize>(op: &Operator, path: &str, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    op.write(path, bytes).await?;
    Ok(())
}

pub async fn create_note<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    content: &str,
    author: &str,
    integrity: &I,
) -> Result<NoteMeta> {
    if find_note_class(op, ws_path, note_id).await?.is_some() {
        return Err(anyhow!("Note already exists: {}", note_id));
    }

    let (frontmatter, sections) = parse_markdown(content);
    let class_name = extract_class(&frontmatter)
        .ok_or_else(|| anyhow!("Class is required for note creation"))?;
    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    ensure_class_storage(op, ws_path, &class_name).await?;

    let class_fields = class_field_names(&class_def);
    let class_set: HashSet<String> = class_fields.iter().cloned().collect();
    if let Some(section_map) = sections.as_object() {
        let extras: Vec<String> = section_map
            .keys()
            .filter(|key| !class_set.contains(*key))
            .cloned()
            .collect();
        if !extras.is_empty() {
            return Err(anyhow!("Unknown class fields: {}", extras.join(", ")));
        }
    }

    let properties = index::extract_properties(content);
    let (casted, warnings) = index::validate_properties(&properties, &class_def)?;
    if !warnings.is_empty() {
        return Err(anyhow!(
            "Class validation failed: {}",
            serde_json::to_string(&warnings)?
        ));
    }

    let mut fields = Map::new();
    for field in &class_fields {
        if let Some(value) = casted.get(field) {
            fields.insert(field.clone(), value.clone());
        }
    }

    let title = extract_title(content, note_id);
    let tags = extract_tags(&frontmatter);
    let timestamp = now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let checksum = integrity.checksum(content);
    let signature = integrity.signature(content);

    let note_row = NoteRow {
        note_id: note_id.to_string(),
        title: title.clone(),
        class: class_name.clone(),
        tags,
        links: Vec::new(),
        canvas_position: Value::Object(Map::new()),
        created_at: timestamp,
        updated_at: timestamp,
        fields: Value::Object(fields),
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        attachments: Vec::new(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        deleted: false,
        deleted_at: None,
        author: author.to_string(),
    };

    write_note_row(op, ws_path, &class_name, note_id, &note_row).await?;

    let revision = RevisionRow {
        revision_id: revision_id.clone(),
        note_id: note_id.to_string(),
        parent_revision_id: None,
        timestamp,
        author: author.to_string(),
        fields: note_row.fields.clone(),
        markdown_checksum: checksum.clone(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        restored_from: None,
    };
    let revision_path = revision_row_path(ws_path, &class_name, &revision_id);
    write_json(op, &revision_path, &revision).await?;

    let ws_id = ws_path
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .unwrap_or(ws_path)
        .to_string();

    Ok(NoteMeta {
        id: note_id.to_string(),
        workspace_id: ws_id,
        title,
        class: Some(class_name),
        tags: note_row.tags.clone(),
        links: note_row.links.clone(),
        canvas_position: note_row.canvas_position.clone(),
        created_at: timestamp,
        updated_at: timestamp,
        integrity: IntegrityPayload {
            checksum,
            signature,
        },
        deleted: false,
        deleted_at: None,
        properties: Value::Object(Map::new()),
    })
}

pub async fn list_notes(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    let mut notes = Vec::new();
    for (class_name, row) in list_note_rows(op, ws_path).await? {
        if row.deleted {
            continue;
        }
        notes.push(serde_json::json!({
            "id": row.note_id,
            "title": row.title,
            "class": class_name,
            "tags": row.tags,
            "properties": row.fields,
            "links": row.links,
            "canvas_position": row.canvas_position,
            "created_at": row.created_at,
            "updated_at": row.updated_at,
        }));
    }
    Ok(notes)
}

pub async fn get_note(op: &Operator, ws_path: &str, note_id: &str) -> Result<Value> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let row = read_note_row(op, ws_path, &class_name, note_id).await?;
    if row.deleted {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    let field_order = class_field_names(&class_def);
    let markdown = render_markdown(
        &row.title,
        &class_name,
        &row.tags,
        &row.fields,
        &field_order,
    );
    let frontmatter = serde_json::json!({
        "class": class_name,
        "tags": row.tags,
    });
    let sections = sections_from_fields(&row.fields);

    Ok(serde_json::json!({
        "id": note_id,
        "revision_id": row.revision_id,
        "content": markdown,
        "frontmatter": frontmatter,
        "sections": sections,
        "attachments": row.attachments,
        "computed": Value::Object(Map::new()),
        "title": row.title,
        "class": row.class,
        "tags": row.tags,
        "links": row.links,
        "canvas_position": row.canvas_position,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "integrity": serde_json::to_value(row.integrity)?,
    }))
}

pub async fn get_note_content(op: &Operator, ws_path: &str, note_id: &str) -> Result<NoteContent> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note content not found: {}", note_id))?;
    let row = read_note_row(op, ws_path, &class_name, note_id).await?;
    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    let field_order = class_field_names(&class_def);
    let markdown = render_markdown(
        &row.title,
        &class_name,
        &row.tags,
        &row.fields,
        &field_order,
    );
    Ok(NoteContent {
        revision_id: row.revision_id,
        parent_revision_id: row.parent_revision_id,
        author: row.author,
        markdown,
        frontmatter: serde_json::json!({
            "class": class_name,
            "tags": row.tags,
        }),
        sections: sections_from_fields(&row.fields),
        attachments: row.attachments,
        computed: Value::Object(Map::new()),
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn update_note<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    content: &str,
    parent_revision_id: Option<&str>,
    author: &str,
    attachments: Option<Vec<Value>>,
    integrity: &I,
) -> Result<Value> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let mut row = read_note_row(op, ws_path, &class_name, note_id).await?;

    if let Some(expected_parent) = parent_revision_id {
        if row.revision_id != expected_parent {
            return Err(anyhow!(
                "Revision conflict: expected {}, got {}",
                expected_parent,
                row.revision_id
            ));
        }
    }

    let (frontmatter, sections) = parse_markdown(content);
    let updated_class =
        extract_class(&frontmatter).ok_or_else(|| anyhow!("Class is required for note update"))?;
    if updated_class != class_name {
        return Err(anyhow!("Class change is not supported"));
    }

    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    let class_fields = class_field_names(&class_def);
    let class_set: HashSet<String> = class_fields.iter().cloned().collect();
    if let Some(section_map) = sections.as_object() {
        let extras: Vec<String> = section_map
            .keys()
            .filter(|key| !class_set.contains(*key))
            .cloned()
            .collect();
        if !extras.is_empty() {
            return Err(anyhow!("Unknown class fields: {}", extras.join(", ")));
        }
    }

    let properties = index::extract_properties(content);
    let (casted, warnings) = index::validate_properties(&properties, &class_def)?;
    if !warnings.is_empty() {
        return Err(anyhow!(
            "Class validation failed: {}",
            serde_json::to_string(&warnings)?
        ));
    }

    let mut fields = Map::new();
    for field in &class_fields {
        if let Some(value) = casted.get(field) {
            fields.insert(field.clone(), value.clone());
        }
    }

    let timestamp = now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let checksum = integrity.checksum(content);
    let signature = integrity.signature(content);

    row.title = extract_title(content, &row.title);
    row.updated_at = timestamp;
    if frontmatter.get("tags").is_some() {
        row.tags = extract_tags(&frontmatter);
    }
    row.fields = Value::Object(fields);
    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = revision_id.clone();
    row.author = author.to_string();
    row.integrity = IntegrityPayload {
        checksum: checksum.clone(),
        signature: signature.clone(),
    };
    row.attachments = attachments.unwrap_or_else(|| row.attachments.clone());

    write_note_row(op, ws_path, &class_name, note_id, &row).await?;

    let revision = RevisionRow {
        revision_id: revision_id.clone(),
        note_id: note_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        markdown_checksum: checksum.clone(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        restored_from: None,
    };
    let revision_path = revision_row_path(ws_path, &class_name, &revision_id);
    write_json(op, &revision_path, &revision).await?;

    get_note(op, ws_path, note_id).await
}

pub async fn delete_note(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    hard_delete: bool,
) -> Result<()> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let mut row = read_note_row(op, ws_path, &class_name, note_id).await?;

    if hard_delete {
        let path = note_row_path(ws_path, &class_name, note_id);
        op.delete(&path).await?;
        return Ok(());
    }

    row.deleted = true;
    row.deleted_at = Some(now_ts());
    write_note_row(op, ws_path, &class_name, note_id, &row).await?;
    Ok(())
}

pub async fn get_note_history(op: &Operator, ws_path: &str, note_id: &str) -> Result<Value> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let revisions_dir = format!("{}/", class_revisions_dir(ws_path, &class_name));
    if !op.exists(&revisions_dir).await? {
        return Ok(serde_json::json!({
            "note_id": note_id,
            "revisions": []
        }));
    }

    let mut revisions = Vec::new();
    let mut lister = op.lister(&revisions_dir).await?;
    while let Some(entry) = lister.try_next().await? {
        if entry.metadata().mode() != EntryMode::FILE {
            continue;
        }
        let revision_path = if entry.name().contains('/') {
            entry.name().to_string()
        } else {
            format!("{}{}", revisions_dir, entry.name())
        };
        let bytes = op.read(&revision_path).await?;
        if let Ok(revision) = serde_json::from_slice::<RevisionRow>(&bytes.to_vec()) {
            if revision.note_id == note_id {
                revisions.push(serde_json::json!({
                    "revision_id": revision.revision_id,
                    "timestamp": revision.timestamp,
                    "checksum": revision.integrity.checksum,
                    "signature": revision.integrity.signature,
                }));
            }
        }
    }

    revisions.sort_by(|a, b| {
        let a_ts = a.get("timestamp").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b_ts = b.get("timestamp").and_then(|v| v.as_f64()).unwrap_or(0.0);
        a_ts.partial_cmp(&b_ts).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(serde_json::json!({
        "note_id": note_id,
        "revisions": revisions,
    }))
}

pub async fn get_note_revision(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    revision_id: &str,
) -> Result<Value> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let revision_path = revision_row_path(ws_path, &class_name, revision_id);
    if !op.exists(&revision_path).await? {
        return Err(anyhow!(
            "Revision {} not found for note {}",
            revision_id,
            note_id
        ));
    }
    let revision = read_json(op, &revision_path).await?;
    if revision.get("note_id").and_then(|v| v.as_str()) != Some(note_id) {
        return Err(anyhow!(
            "Revision {} not found for note {}",
            revision_id,
            note_id
        ));
    }
    Ok(revision)
}

pub async fn restore_note<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    revision_id: &str,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let revision_path = revision_row_path(ws_path, &class_name, revision_id);
    if !op.exists(&revision_path).await? {
        return Err(anyhow!(
            "Revision {} not found for note {}",
            revision_id,
            note_id
        ));
    }

    let revision: RevisionRow = {
        let bytes = op.read(&revision_path).await?;
        serde_json::from_slice(&bytes.to_vec())?
    };
    if revision.note_id != note_id {
        return Err(anyhow!(
            "Revision {} not found for note {}",
            revision_id,
            note_id
        ));
    }

    let mut row = read_note_row(op, ws_path, &class_name, note_id).await?;
    let new_rev_id = Uuid::new_v4().to_string();
    let timestamp = now_ts();

    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    let field_order = class_field_names(&class_def);
    let markdown = render_markdown(
        &row.title,
        &class_name,
        &row.tags,
        &revision.fields,
        &field_order,
    );
    let checksum = integrity.checksum(&markdown);
    let signature = integrity.signature(&markdown);

    row.parent_revision_id = Some(row.revision_id.clone());
    row.revision_id = new_rev_id.clone();
    row.updated_at = timestamp;
    row.fields = revision.fields.clone();
    row.integrity = IntegrityPayload {
        checksum: checksum.clone(),
        signature: signature.clone(),
    };
    row.author = author.to_string();
    write_note_row(op, ws_path, &class_name, note_id, &row).await?;

    let restore_revision = RevisionRow {
        revision_id: new_rev_id.clone(),
        note_id: note_id.to_string(),
        parent_revision_id: row.parent_revision_id.clone(),
        timestamp,
        author: author.to_string(),
        fields: row.fields.clone(),
        markdown_checksum: checksum.clone(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        restored_from: Some(revision_id.to_string()),
    };
    let restore_path = revision_row_path(ws_path, &class_name, &new_rev_id);
    write_json(op, &restore_path, &restore_revision).await?;

    Ok(serde_json::json!({
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }))
}
