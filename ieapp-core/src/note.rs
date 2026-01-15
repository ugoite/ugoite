use crate::index;
use crate::integrity::IntegrityProvider;
use crate::link::Link;
use anyhow::{anyhow, Result};
use chrono::Utc;
use futures::TryStreamExt;
use opendal::Operator;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

const DEFAULT_INITIAL_MESSAGE: &str = "Initial creation";
const DEFAULT_UPDATE_MESSAGE: &str = "Update";

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
struct HistoryIndex {
    note_id: String,
    revisions: Vec<RevisionEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RevisionEntry {
    revision_id: String,
    timestamp: f64,
    checksum: String,
    signature: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RevisionRecord {
    revision_id: String,
    parent_revision_id: Option<String>,
    timestamp: f64,
    author: String,
    diff: String,
    integrity: IntegrityPayload,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    restored_from: Option<String>,
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

fn build_diff(old_content: &str, new_content: &str) -> String {
    if old_content == new_content {
        return String::new();
    }

    let mut diff = String::from("--- previous\n+++ current\n");
    for line in old_content.lines() {
        diff.push_str(&format!("-{}\n", line));
    }
    for line in new_content.lines() {
        diff.push_str(&format!("+{}\n", line));
    }
    diff
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
    let note_path = format!("{}/notes/{}", ws_path, note_id);

    if op.exists(&format!("{}/", note_path)).await?
        || op.exists(&format!("{}/meta.json", note_path)).await?
    {
        return Err(anyhow!("Note already exists: {}", note_id));
    }

    op.create_dir(&format!("{}/", note_path)).await?;
    op.create_dir(&format!("{}/history/", note_path)).await?;

    let (frontmatter, sections) = parse_markdown(content);
    let title = extract_title(content, note_id);
    let timestamp = now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let checksum = integrity.checksum(content);
    let signature = integrity.signature(content);

    let note_content = NoteContent {
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        author: author.to_string(),
        markdown: content.to_string(),
        frontmatter: frontmatter.clone(),
        sections,
        attachments: Vec::new(),
        computed: Value::Object(Map::new()),
    };

    write_json(op, &format!("{}/content.json", note_path), &note_content).await?;

    let history_record = RevisionRecord {
        revision_id: revision_id.clone(),
        parent_revision_id: None,
        timestamp,
        author: author.to_string(),
        diff: String::new(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        message: DEFAULT_INITIAL_MESSAGE.to_string(),
        restored_from: None,
    };

    write_json(
        op,
        &format!("{}/history/{}.json", note_path, revision_id),
        &history_record,
    )
    .await?;

    let history_index = HistoryIndex {
        note_id: note_id.to_string(),
        revisions: vec![RevisionEntry {
            revision_id: revision_id.clone(),
            timestamp,
            checksum: checksum.clone(),
            signature: signature.clone(),
        }],
    };
    write_json(
        op,
        &format!("{}/history/index.json", note_path),
        &history_index,
    )
    .await?;

    let ws_id = ws_path
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .unwrap_or(ws_path)
        .to_string();

    let meta = NoteMeta {
        id: note_id.to_string(),
        workspace_id: ws_id,
        title,
        class: extract_class(&frontmatter),
        tags: extract_tags(&frontmatter),
        links: Vec::new(),
        canvas_position: Value::Object(Map::new()),
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

    write_json(op, &format!("{}/meta.json", note_path), &meta).await?;

    index::update_note_index(op, ws_path, note_id).await?;

    Ok(meta)
}

pub async fn list_notes(op: &Operator, ws_path: &str) -> Result<Vec<Value>> {
    let index_path = format!("{}/index/index.json", ws_path);
    if op.exists(&index_path).await? {
        if let Ok(index_json) = read_json(op, &index_path).await {
            if let Some(notes) = index_json.get("notes").and_then(|v| v.as_object()) {
                return Ok(notes.values().cloned().collect());
            }
        }
    }

    let notes_dir = format!("{}/notes/", ws_path);
    if !op.exists(&notes_dir).await? {
        return Ok(Vec::new());
    }

    let mut lister = op.lister(&notes_dir).await?;
    let mut notes = Vec::new();

    while let Some(entry) = lister.try_next().await? {
        let meta = entry.metadata();
        if meta.mode() != opendal::EntryMode::DIR {
            continue;
        }
        let note_id = entry
            .name()
            .trim_end_matches('/')
            .split('/')
            .next_back()
            .unwrap_or("");
        if note_id.is_empty() {
            continue;
        }
        let note_dir = format!("{}/notes/{}", ws_path, note_id);
        let meta_path = format!("{}/meta.json", note_dir);
        if !op.exists(&meta_path).await? {
            continue;
        }
        let meta_json = match read_json(op, &meta_path).await {
            Ok(value) => value,
            Err(_) => continue,
        };
        if meta_json.get("deleted").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let summary = serde_json::json!({
            "id": meta_json.get("id").cloned().unwrap_or(Value::Null),
            "title": meta_json.get("title").cloned().unwrap_or(Value::Null),
            "class": meta_json.get("class").cloned().unwrap_or(Value::Null),
            "tags": meta_json.get("tags").cloned().unwrap_or(Value::Array(Vec::new())),
            "properties": meta_json.get("properties").cloned().unwrap_or(Value::Object(Map::new())),
            "links": meta_json.get("links").cloned().unwrap_or(Value::Array(Vec::new())),
            "canvas_position": meta_json
                .get("canvas_position")
                .cloned()
                .unwrap_or(Value::Object(Map::new())),
            "created_at": meta_json.get("created_at").cloned().unwrap_or(Value::Null),
            "updated_at": meta_json.get("updated_at").cloned().unwrap_or(Value::Null),
        });
        notes.push(summary);
    }

    Ok(notes)
}

pub async fn get_note(op: &Operator, ws_path: &str, note_id: &str) -> Result<Value> {
    let note_path = format!("{}/notes/{}", ws_path, note_id);
    let meta_path = format!("{}/meta.json", note_path);
    let content_path = format!("{}/content.json", note_path);

    if !op.exists(&meta_path).await? || !op.exists(&content_path).await? {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    let meta = read_json(op, &meta_path).await?;
    if meta.get("deleted").and_then(|v| v.as_bool()) == Some(true) {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    let content = read_json(op, &content_path).await?;

    Ok(serde_json::json!({
        "id": note_id,
        "revision_id": content.get("revision_id").cloned().unwrap_or(Value::Null),
        "content": content.get("markdown").cloned().unwrap_or(Value::Null),
        "frontmatter": content.get("frontmatter").cloned().unwrap_or(Value::Object(Map::new())),
        "sections": content.get("sections").cloned().unwrap_or(Value::Object(Map::new())),
        "attachments": content.get("attachments").cloned().unwrap_or(Value::Array(Vec::new())),
        "computed": content.get("computed").cloned().unwrap_or(Value::Object(Map::new())),
        "title": meta.get("title").cloned().unwrap_or(Value::Null),
        "class": meta.get("class").cloned().unwrap_or(Value::Null),
        "tags": meta.get("tags").cloned().unwrap_or(Value::Array(Vec::new())),
        "links": meta.get("links").cloned().unwrap_or(Value::Array(Vec::new())),
        "canvas_position": meta.get("canvas_position").cloned().unwrap_or(Value::Object(Map::new())),
        "created_at": meta.get("created_at").cloned().unwrap_or(Value::Null),
        "updated_at": meta.get("updated_at").cloned().unwrap_or(Value::Null),
        "integrity": meta.get("integrity").cloned().unwrap_or(Value::Object(Map::new())),
    }))
}

pub async fn get_note_content(op: &Operator, ws_path: &str, note_id: &str) -> Result<NoteContent> {
    let path = format!("{}/notes/{}/content.json", ws_path, note_id);
    if !op.exists(&path).await? {
        return Err(anyhow!("Note content not found: {}", note_id));
    }
    let bytes = op.read(&path).await?;
    let content: NoteContent = serde_json::from_slice(&bytes.to_vec())?;
    Ok(content)
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
    let note_path = format!("{}/notes/{}", ws_path, note_id);

    if !op.exists(&format!("{}/meta.json", note_path)).await? {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    let current_content_bytes = op.read(&format!("{}/content.json", note_path)).await?;
    let current_content: NoteContent = serde_json::from_slice(&current_content_bytes.to_vec())?;

    if let Some(expected_parent) = parent_revision_id {
        if current_content.revision_id != expected_parent {
            return Err(anyhow!(
                "Revision conflict: expected {}, got {}",
                expected_parent,
                current_content.revision_id
            ));
        }
    }

    let (frontmatter, sections) = parse_markdown(content);
    let timestamp = now_ts();
    let revision_id = Uuid::new_v4().to_string();
    let checksum = integrity.checksum(content);
    let signature = integrity.signature(content);

    let diff = build_diff(&current_content.markdown, content);

    let new_attachments = attachments.unwrap_or_else(|| current_content.attachments.clone());

    let new_note_content = NoteContent {
        revision_id: revision_id.clone(),
        parent_revision_id: Some(current_content.revision_id.clone()),
        author: author.to_string(),
        markdown: content.to_string(),
        frontmatter: frontmatter.clone(),
        sections,
        attachments: new_attachments,
        computed: current_content.computed.clone(),
    };

    write_json(
        op,
        &format!("{}/content.json", note_path),
        &new_note_content,
    )
    .await?;

    let history_record = RevisionRecord {
        revision_id: revision_id.clone(),
        parent_revision_id: Some(current_content.revision_id.clone()),
        timestamp,
        author: author.to_string(),
        diff,
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        message: DEFAULT_UPDATE_MESSAGE.to_string(),
        restored_from: None,
    };

    write_json(
        op,
        &format!("{}/history/{}.json", note_path, revision_id),
        &history_record,
    )
    .await?;

    let history_path = format!("{}/history/index.json", note_path);
    let mut history: HistoryIndex = if op.exists(&history_path).await? {
        let h_bytes = op.read(&history_path).await?;
        serde_json::from_slice(&h_bytes.to_vec())?
    } else {
        HistoryIndex {
            note_id: note_id.to_string(),
            revisions: vec![],
        }
    };
    history.revisions.push(RevisionEntry {
        revision_id: revision_id.clone(),
        timestamp,
        checksum: checksum.clone(),
        signature: signature.clone(),
    });
    write_json(op, &history_path, &history).await?;

    let meta_path = format!("{}/meta.json", note_path);
    let mut meta: NoteMeta = {
        let meta_bytes = op.read(&meta_path).await?;
        serde_json::from_slice(&meta_bytes.to_vec())?
    };
    meta.title = extract_title(content, &meta.title);
    meta.updated_at = timestamp;
    if frontmatter.get("class").is_some() {
        meta.class = extract_class(&frontmatter);
    }
    if frontmatter.get("tags").is_some() {
        meta.tags = extract_tags(&frontmatter);
    }
    meta.integrity = IntegrityPayload {
        checksum,
        signature,
    };

    write_json(op, &meta_path, &meta).await?;

    index::update_note_index(op, ws_path, note_id).await?;

    get_note(op, ws_path, note_id).await
}

pub async fn delete_note(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    hard_delete: bool,
) -> Result<()> {
    let note_path = format!("{}/notes/{}", ws_path, note_id);
    if !op.exists(&format!("{}/", note_path)).await? {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    if hard_delete {
        op.remove_all(&format!("{}/", note_path)).await?;
        index::update_note_index(op, ws_path, note_id).await?;
        return Ok(());
    }

    let meta_path = format!("{}/meta.json", note_path);
    let mut meta: NoteMeta = {
        let bytes = op.read(&meta_path).await?;
        serde_json::from_slice(&bytes.to_vec())?
    };
    meta.deleted = true;
    meta.deleted_at = Some(now_ts());
    write_json(op, &meta_path, &meta).await?;

    index::update_note_index(op, ws_path, note_id).await?;
    Ok(())
}

pub async fn get_note_history(op: &Operator, ws_path: &str, note_id: &str) -> Result<Value> {
    let note_path = format!("{}/notes/{}", ws_path, note_id);
    if !op.exists(&format!("{}/", note_path)).await? {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    let history_path = format!("{}/history/index.json", note_path);
    if !op.exists(&history_path).await? {
        return Ok(serde_json::json!({
            "note_id": note_id,
            "revisions": []
        }));
    }

    read_json(op, &history_path).await
}

pub async fn get_note_revision(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    revision_id: &str,
) -> Result<Value> {
    let revision_path = format!("{}/notes/{}/history/{}.json", ws_path, note_id, revision_id);
    if !op.exists(&revision_path).await? {
        return Err(anyhow!(
            "Revision {} not found for note {}",
            revision_id,
            note_id
        ));
    }
    read_json(op, &revision_path).await
}

pub async fn restore_note<I: IntegrityProvider>(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    revision_id: &str,
    author: &str,
    integrity: &I,
) -> Result<Value> {
    let note_path = format!("{}/notes/{}", ws_path, note_id);
    if !op.exists(&format!("{}/", note_path)).await? {
        return Err(anyhow!("Note not found: {}", note_id));
    }

    let revision_path = format!("{}/history/{}.json", note_path, revision_id);
    if !op.exists(&revision_path).await? {
        return Err(anyhow!(
            "Revision {} not found for note {}",
            revision_id,
            note_id
        ));
    }

    let content_path = format!("{}/content.json", note_path);
    if !op.exists(&content_path).await? {
        return Err(anyhow!("Note content not found: {}", note_id));
    }

    let current_content: NoteContent = {
        let bytes = op.read(&content_path).await?;
        serde_json::from_slice(&bytes.to_vec())?
    };

    let history_path = format!("{}/history/index.json", note_path);
    let mut history: HistoryIndex = if op.exists(&history_path).await? {
        let bytes = op.read(&history_path).await?;
        serde_json::from_slice(&bytes.to_vec())?
    } else {
        return Err(anyhow!("History index missing for note {}", note_id));
    };

    let revision_order: Vec<String> = history
        .revisions
        .iter()
        .map(|r| r.revision_id.clone())
        .collect();
    if !revision_order.contains(&revision_id.to_string()) {
        return Err(anyhow!(
            "Revision {} not found in history index",
            revision_id
        ));
    }

    let new_rev_id = Uuid::new_v4().to_string();
    let timestamp = now_ts();
    let parent_revision = current_content.revision_id.clone();
    let checksum = integrity.checksum(&current_content.markdown);
    let signature = integrity.signature(&current_content.markdown);

    let revision_record = RevisionRecord {
        revision_id: new_rev_id.clone(),
        parent_revision_id: Some(parent_revision.clone()),
        timestamp,
        author: author.to_string(),
        diff: String::new(),
        integrity: IntegrityPayload {
            checksum: checksum.clone(),
            signature: signature.clone(),
        },
        message: format!("Restored from revision {}", revision_id),
        restored_from: Some(revision_id.to_string()),
    };

    write_json(
        op,
        &format!("{}/history/{}.json", note_path, new_rev_id),
        &revision_record,
    )
    .await?;

    history.revisions.push(RevisionEntry {
        revision_id: new_rev_id.clone(),
        timestamp,
        checksum: checksum.clone(),
        signature: signature.clone(),
    });
    write_json(op, &history_path, &history).await?;

    let mut new_content = current_content;
    new_content.revision_id = new_rev_id.clone();
    new_content.parent_revision_id = Some(parent_revision);
    write_json(op, &content_path, &new_content).await?;

    let meta_path = format!("{}/meta.json", note_path);
    let mut meta: NoteMeta = {
        let bytes = op.read(&meta_path).await?;
        serde_json::from_slice(&bytes.to_vec())?
    };
    meta.updated_at = timestamp;
    meta.integrity = IntegrityPayload {
        checksum,
        signature,
    };
    write_json(op, &meta_path, &meta).await?;

    index::update_note_index(op, ws_path, note_id).await?;

    Ok(serde_json::json!({
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }))
}
