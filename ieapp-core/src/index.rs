use anyhow::{anyhow, Result};
use chrono::{NaiveDate, Utc};
use futures::TryStreamExt;
use opendal::Operator;
use regex::Regex;
use serde_json::{Map, Value};
use serde_yaml;
use std::collections::{HashMap, HashSet};

pub async fn query_index(op: &Operator, ws_path: &str, query: &str) -> Result<Vec<Value>> {
    let index_path = format!("{}/index/index.json", ws_path);
    if !op.exists(&index_path).await? {
        return Ok(vec![]);
    }

    let bytes = op.read(&index_path).await?;
    let index_data: Value = serde_json::from_slice(&bytes.to_vec()).unwrap_or(Value::Null);
    let notes_map = index_data
        .get("notes")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let filters: Option<Map<String, Value>> = if query.trim().is_empty() {
        None
    } else {
        let parsed: Value = serde_json::from_str(query).unwrap_or(Value::Null);
        parsed.as_object().cloned()
    };

    let mut results = Vec::new();
    for note in notes_map.values() {
        if let Some(filter_obj) = filters.as_ref() {
            if !matches_filters(note, filter_obj)? {
                continue;
            }
        }
        results.push(note.clone());
    }

    Ok(results)
}

fn matches_filters(note: &Value, filters: &Map<String, Value>) -> Result<bool> {
    for (key, expected) in filters {
        let mut note_value = note.get(key).cloned();
        if note_value.is_none() {
            note_value = note
                .get("properties")
                .and_then(|v| v.as_object())
                .and_then(|props| props.get(key))
                .cloned();
        }

        if expected.is_object() {
            return Err(anyhow!(
                "Structured operators (e.g., $gt) are not implemented for the local query helper yet."
            ));
        }

        if key == "tag" {
            if let Some(tags) = note.get("tags").and_then(|v| v.as_array()) {
                if !tags.iter().any(|v| v == expected) {
                    return Ok(false);
                }
                continue;
            }
        }

        match note_value {
            Some(Value::Array(list)) => {
                if !list.iter().any(|v| v == expected) {
                    return Ok(false);
                }
            }
            Some(value) => {
                if value != *expected {
                    return Ok(false);
                }
            }
            None => return Ok(false),
        }
    }
    Ok(true)
}

pub async fn reindex_all(op: &Operator, ws_path: &str) -> Result<()> {
    let index_dir = format!("{}/index/", ws_path);
    op.create_dir(&index_dir).await?;

    let classes = load_classes(op, ws_path).await?;
    let notes = collect_notes(op, ws_path, &classes).await?;
    let stats = aggregate_stats(&notes);
    let inverted = build_inverted_index(&notes);

    let index_path = format!("{}/index/index.json", ws_path);
    let stats_path = format!("{}/index/stats.json", ws_path);
    let inverted_path = format!("{}/index/inverted_index.json", ws_path);

    let index_payload = serde_json::json!({
        "notes": notes,
        "class_stats": stats.get("class_stats").cloned().unwrap_or(Value::Object(Map::new()))
    });
    op.write(&index_path, serde_json::to_vec_pretty(&index_payload)?)
        .await?;

    op.write(&inverted_path, serde_json::to_vec_pretty(&inverted)?)
        .await?;

    let stats_payload = serde_json::json!({
        "note_count": stats.get("note_count").cloned().unwrap_or(Value::Number(0.into())),
        "class_stats": stats.get("class_stats").cloned().unwrap_or(Value::Object(Map::new())),
        "tag_counts": stats.get("tag_counts").cloned().unwrap_or(Value::Object(Map::new())),
        "last_indexed": Utc::now().timestamp_millis() as f64 / 1000.0
    });
    op.write(&stats_path, serde_json::to_vec_pretty(&stats_payload)?)
        .await?;

    Ok(())
}

pub async fn update_note_index(op: &Operator, ws_path: &str, note_id: &str) -> Result<()> {
    let index_path = format!("{}/index/index.json", ws_path);
    let stats_path = format!("{}/index/stats.json", ws_path);
    let inverted_path = format!("{}/index/inverted_index.json", ws_path);

    if !op.exists(&index_path).await?
        || !op.exists(&stats_path).await?
        || !op.exists(&inverted_path).await?
    {
        return reindex_all(op, ws_path).await;
    }

    let classes = load_classes(op, ws_path).await?;
    let mut index_data: Value = {
        let bytes = op.read(&index_path).await?;
        serde_json::from_slice(&bytes.to_vec()).unwrap_or(Value::Null)
    };

    if index_data
        .get("notes")
        .and_then(|v| v.as_object())
        .is_none()
    {
        index_data["notes"] = Value::Object(Map::new());
    }

    let notes_obj = index_data
        .get_mut("notes")
        .and_then(|v| v.as_object_mut())
        .expect("notes should be an object after initialization");

    let record = build_record(op, ws_path, note_id, &classes).await?;
    if let Some(rec) = record {
        notes_obj.insert(note_id.to_string(), rec);
    } else {
        notes_obj.remove(note_id);
    }

    let notes_map = notes_obj.clone();
    let stats = aggregate_stats(&notes_map);
    index_data["class_stats"] = stats
        .get("class_stats")
        .cloned()
        .unwrap_or(Value::Object(Map::new()));

    op.write(&index_path, serde_json::to_vec_pretty(&index_data)?)
        .await?;

    let inverted = build_inverted_index(&notes_map);
    op.write(&inverted_path, serde_json::to_vec_pretty(&inverted)?)
        .await?;

    let stats_payload = serde_json::json!({
        "note_count": stats.get("note_count").cloned().unwrap_or(Value::Number(0.into())),
        "class_stats": stats.get("class_stats").cloned().unwrap_or(Value::Object(Map::new())),
        "tag_counts": stats.get("tag_counts").cloned().unwrap_or(Value::Object(Map::new())),
        "last_indexed": Utc::now().timestamp_millis() as f64 / 1000.0
    });
    op.write(&stats_path, serde_json::to_vec_pretty(&stats_payload)?)
        .await?;

    Ok(())
}

async fn read_json(op: &Operator, path: &str) -> Result<Value> {
    let bytes = op.read(path).await?;
    Ok(serde_json::from_slice(&bytes.to_vec())?)
}

pub fn extract_properties(markdown: &str) -> Value {
    let mut properties = Map::new();

    let (frontmatter, body) = extract_frontmatter(markdown);
    if let Some(fm) = frontmatter {
        if let Some(obj) = fm.as_object() {
            for (k, v) in obj {
                properties.insert(k.clone(), v.clone());
            }
        }
    }

    let sections = extract_sections(&body);
    for (k, v) in sections {
        if !v.is_empty() {
            properties.insert(k, Value::String(v));
        }
    }

    Value::Object(properties)
}

fn extract_frontmatter(content: &str) -> (Option<Value>, String) {
    let re = Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").unwrap();
    if let Some(caps) = re.captures(content) {
        let yaml_str = caps.get(1).unwrap().as_str();
        let fm_yaml: Option<serde_yaml::Value> = serde_yaml::from_str(yaml_str).ok();
        let fm_json = fm_yaml.and_then(|y| serde_json::to_value(y).ok());
        let end = caps.get(0).unwrap().end();
        return (fm_json, content[end..].to_string());
    }
    (None, content.to_string())
}

fn extract_sections(body: &str) -> Vec<(String, String)> {
    let mut sections = Vec::new();
    let mut current_key: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();

    let header_re = Regex::new(r"^##\s+(.+)$").unwrap();

    for line in body.lines() {
        if let Some(caps) = header_re.captures(line) {
            if let Some(key) = current_key.take() {
                sections.push((key, buffer.join("\n").trim().to_string()));
            }
            current_key = Some(caps.get(1).unwrap().as_str().trim().to_string());
            buffer.clear();
            continue;
        }

        if line.starts_with('#') {
            if let Some(key) = current_key.take() {
                sections.push((key, buffer.join("\n").trim().to_string()));
            }
            buffer.clear();
            continue;
        }

        if current_key.is_some() {
            buffer.push(line.to_string());
        }
    }

    if let Some(key) = current_key {
        sections.push((key, buffer.join("\n").trim().to_string()));
    }

    sections
}

pub fn compute_word_count(content: &str) -> usize {
    content.split_whitespace().count()
}

pub fn validate_properties(properties: &Value, note_class: &Value) -> Result<(Value, Vec<Value>)> {
    let mut warnings = Vec::new();
    let mut casted = properties.clone();

    let fields = note_class.get("fields");
    let mut field_defs: HashMap<String, Value> = HashMap::new();

    match fields {
        Some(Value::Object(obj)) => {
            for (k, v) in obj {
                field_defs.insert(k.clone(), v.clone());
            }
        }
        Some(Value::Array(arr)) => {
            for item in arr {
                if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                    field_defs.insert(name.to_string(), item.clone());
                }
            }
        }
        _ => {}
    }

    for (field_name, field_def) in field_defs {
        let value = properties.get(&field_name).cloned();
        let field_type = field_def
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("string");
        let required = field_def
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if required && (value.is_none() || value == Some(Value::String(String::new()))) {
            warnings.push(serde_json::json!({
                "code": "missing_field",
                "field": field_name,
                "message": format!("Missing required field: {}", field_name)
            }));
            continue;
        }

        let Some(raw_value) = value else { continue };

        let casted_value = match field_type {
            "number" => match raw_value {
                Value::Number(_) => Some(raw_value.clone()),
                Value::String(ref s) => s
                    .parse::<f64>()
                    .ok()
                    .map(|n| Value::Number(serde_json::Number::from_f64(n).unwrap())),
                _ => None,
            },
            "date" => match raw_value {
                Value::String(ref s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .ok()
                    .map(|d| Value::String(d.format("%Y-%m-%d").to_string())),
                _ => None,
            },
            "list" => match raw_value {
                Value::Array(_) => Some(raw_value.clone()),
                Value::String(ref s) => {
                    let items: Vec<Value> = s
                        .lines()
                        .filter_map(|line| {
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                None
                            } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
                                Some(Value::String(trimmed[2..].to_string()))
                            } else {
                                Some(Value::String(trimmed.to_string()))
                            }
                        })
                        .collect();
                    Some(Value::Array(items))
                }
                _ => None,
            },
            "boolean" => match raw_value {
                Value::Bool(_) => Some(raw_value.clone()),
                Value::String(ref s) => match s.to_lowercase().as_str() {
                    "true" => Some(Value::Bool(true)),
                    "false" => Some(Value::Bool(false)),
                    _ => None,
                },
                _ => None,
            },
            _ => Some(raw_value.clone()),
        };

        if let Some(value) = casted_value {
            if let Some(obj) = casted.as_object_mut() {
                obj.insert(field_name.clone(), value);
            }
        } else {
            warnings.push(serde_json::json!({
                "code": "invalid_type",
                "field": field_name,
                "message": format!("Field '{}' has invalid type", field_name)
            }));
        }
    }

    Ok((casted, warnings))
}

pub fn aggregate_stats(notes: &Map<String, Value>) -> Value {
    let mut class_stats: HashMap<String, Map<String, Value>> = HashMap::new();
    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    let mut uncategorized = 0usize;

    for record in notes.values() {
        let note_class = record
            .get("class")
            .or_else(|| record.get("properties").and_then(|v| v.get("class")));

        if let Some(class_name) = note_class.and_then(|v| v.as_str()) {
            let entry = class_stats.entry(class_name.to_string()).or_default();
            let count = entry.get("count").and_then(|v| v.as_u64()).unwrap_or(0) + 1;
            entry.insert("count".to_string(), Value::Number(count.into()));

            let fields = entry
                .entry("fields".to_string())
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(field_map) = fields.as_object_mut() {
                if let Some(props) = record.get("properties").and_then(|v| v.as_object()) {
                    for key in props.keys() {
                        let current = field_map.get(key).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
                        field_map.insert(key.to_string(), Value::Number(current.into()));
                    }
                }
            }
        } else {
            uncategorized += 1;
        }

        if let Some(tags) = record.get("tags").and_then(|v| v.as_array()) {
            for tag in tags {
                if let Some(tag_str) = tag.as_str() {
                    *tag_counts.entry(tag_str.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    let mut class_stats_json: Map<String, Value> = class_stats
        .into_iter()
        .map(|(k, v)| (k, Value::Object(v)))
        .collect();
    class_stats_json.insert(
        "_uncategorized".to_string(),
        Value::Object({
            let mut map = Map::new();
            map.insert("count".to_string(), Value::Number(uncategorized.into()));
            map
        }),
    );

    Value::Object(
        [
            (
                "note_count".to_string(),
                Value::Number((notes.len() as u64).into()),
            ),
            ("class_stats".to_string(), Value::Object(class_stats_json)),
            (
                "tag_counts".to_string(),
                Value::Object(
                    tag_counts
                        .into_iter()
                        .map(|(k, v)| (k, Value::Number((v as u64).into())))
                        .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

async fn load_classes(op: &Operator, ws_path: &str) -> Result<HashMap<String, Value>> {
    let classes_path = format!("{}/classes/", ws_path);
    if !op.exists(&classes_path).await? {
        return Ok(HashMap::new());
    }

    let mut classes = HashMap::new();
    let mut lister = op.lister(&classes_path).await?;
    while let Some(entry) = lister.try_next().await? {
        if entry.metadata().mode() != opendal::EntryMode::FILE {
            continue;
        }
        let name = entry
            .name()
            .trim_end_matches(".json")
            .split('/')
            .next_back()
            .unwrap_or("");
        if name.is_empty() {
            continue;
        }
        let entry_name = entry.name();
        let entry_path = if entry_name.contains('/') {
            entry_name.to_string()
        } else {
            format!("{}{}", classes_path, entry_name)
        };
        let bytes = op.read(&entry_path).await?;
        if let Ok(value) = serde_json::from_slice::<Value>(&bytes.to_vec()) {
            classes.insert(name.to_string(), value);
        }
    }
    Ok(classes)
}

async fn collect_notes(
    op: &Operator,
    ws_path: &str,
    classes: &HashMap<String, Value>,
) -> Result<Map<String, Value>> {
    let notes_path = format!("{}/notes/", ws_path);
    if !op.exists(&notes_path).await? {
        return Ok(Map::new());
    }

    let mut notes = Map::new();
    let mut lister = op.lister(&notes_path).await?;
    while let Some(entry) = lister.try_next().await? {
        if entry.metadata().mode() != opendal::EntryMode::DIR {
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
        if let Some(record) = build_record(op, ws_path, note_id, classes).await? {
            notes.insert(note_id.to_string(), record);
        }
    }
    Ok(notes)
}

async fn build_record(
    op: &Operator,
    ws_path: &str,
    note_id: &str,
    classes: &HashMap<String, Value>,
) -> Result<Option<Value>> {
    let note_dir = format!("{}/notes/{}", ws_path, note_id);
    let content_path = format!("{}/content.json", note_dir);
    let meta_path = format!("{}/meta.json", note_dir);

    if !op.exists(&content_path).await? {
        return Ok(None);
    }

    let content_bytes = op.read(&content_path).await?;
    let content_json: Value =
        serde_json::from_slice(&content_bytes.to_vec()).unwrap_or(Value::Null);
    let markdown = content_json
        .get("markdown")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut properties = extract_properties(markdown);

    let mut meta_json = Value::Object(Map::new());
    if op.exists(&meta_path).await? {
        if let Ok(value) = read_json(op, &meta_path).await {
            meta_json = value;
        }
    }

    if meta_json.get("deleted").and_then(|v| v.as_bool()) == Some(true) {
        return Ok(None);
    }

    let note_class = meta_json
        .get("class")
        .or_else(|| properties.get("class"))
        .or_else(|| content_json.get("frontmatter").and_then(|v| v.get("class")))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut warnings = Vec::new();
    if let Some(class_name) = note_class.as_ref() {
        if let Some(class_def) = classes.get(class_name) {
            if let Ok((casted, warns)) = validate_properties(&properties, class_def) {
                properties = casted;
                warnings = warns;
            }
        }
    }

    let word_count = compute_word_count(markdown);
    let record = serde_json::json!({
        "id": note_id,
        "title": meta_json.get("title").cloned().unwrap_or(Value::String(note_id.to_string())),
        "class": note_class,
        "updated_at": meta_json.get("updated_at").cloned().unwrap_or(Value::Null),
        "workspace_id": meta_json
            .get("workspace_id")
            .cloned()
            .unwrap_or(Value::String(
                ws_path.split('/').next_back().unwrap_or("").to_string(),
            )),
        "properties": properties,
        "word_count": word_count,
        "tags": meta_json.get("tags").cloned().unwrap_or(Value::Array(Vec::new())),
        "links": meta_json.get("links").cloned().unwrap_or(Value::Array(Vec::new())),
        "canvas_position": meta_json
            .get("canvas_position")
            .cloned()
            .unwrap_or(Value::Object(Map::new())),
        "checksum": meta_json
            .get("integrity")
            .and_then(|v| v.get("checksum"))
            .cloned()
            .unwrap_or(Value::Null),
        "validation_warnings": Value::Array(warnings),
    });

    Ok(Some(record))
}

fn build_inverted_index(notes: &Map<String, Value>) -> Value {
    let mut inverted: HashMap<String, HashSet<String>> = HashMap::new();

    for (note_id, record) in notes {
        let tokens = tokenize_record(record);
        for token in tokens {
            inverted
                .entry(token)
                .or_default()
                .insert(note_id.to_string());
        }
    }

    let mut inverted_json = Map::new();
    for (token, ids) in inverted {
        let ids_arr = ids.into_iter().map(Value::String).collect();
        inverted_json.insert(token, Value::Array(ids_arr));
    }

    Value::Object(inverted_json)
}

fn tokenize_record(record: &Value) -> HashSet<String> {
    let mut tokens = HashSet::new();

    if let Some(title) = record.get("title").and_then(|v| v.as_str()) {
        tokens.extend(tokenize_text(title));
    }

    if let Some(tags) = record.get("tags").and_then(|v| v.as_array()) {
        for tag in tags {
            if let Some(tag_str) = tag.as_str() {
                tokens.extend(tokenize_text(tag_str));
            }
        }
    }

    if let Some(class) = record.get("class").and_then(|v| v.as_str()) {
        tokens.extend(tokenize_text(class));
    }

    if let Some(props) = record.get("properties").and_then(|v| v.as_object()) {
        for (key, value) in props {
            tokens.extend(tokenize_text(key));
            match value {
                Value::String(text) => tokens.extend(tokenize_text(text)),
                Value::Array(items) => {
                    for item in items {
                        if let Some(item_str) = item.as_str() {
                            tokens.extend(tokenize_text(item_str));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    tokens
}

fn tokenize_text(text: &str) -> HashSet<String> {
    Regex::new(r"\w+")
        .unwrap()
        .find_iter(text)
        .map(|m| m.as_str().to_lowercase())
        .filter(|token| token.len() > 1 && !token.chars().all(|c| c.is_ascii_digit()))
        .collect()
}
