use anyhow::{anyhow, Result};
use base64::Engine as _;
use chrono::{DateTime, NaiveDate, NaiveTime, SecondsFormat, Timelike, Utc};
use opendal::Operator;
use regex::Regex;
use serde_json::{Map, Value};
use serde_yaml;
use std::collections::HashMap;
use uuid::Uuid;

use crate::entry;
use crate::sql;

pub async fn query_index(op: &Operator, ws_path: &str, query: &str) -> Result<Vec<Value>> {
    let forms = load_forms(op, ws_path).await?;
    let entries_map = collect_entries(op, ws_path, &forms).await?;

    let query_value = if query.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(query).unwrap_or(Value::Null)
    };

    if let Some(sql_query) = extract_sql_query(&query_value) {
        let parsed = sql::parse_sql(&sql_query)?;
        let tables = build_sql_tables(op, ws_path, &forms, &entries_map).await?;
        return sql::filter_entries_by_sql(&tables, &parsed);
    }

    let filters: Option<Map<String, Value>> = query_value.as_object().cloned();

    let mut results = Vec::new();
    for entry in entries_map.values() {
        if let Some(filter_obj) = filters.as_ref() {
            if !matches_filters(entry, filter_obj)? {
                continue;
            }
        }
        results.push(entry.clone());
    }

    Ok(results)
}

pub async fn execute_sql_query(
    op: &Operator,
    ws_path: &str,
    sql_query: &str,
) -> Result<Vec<Value>> {
    let forms = load_forms(op, ws_path).await?;
    let entries_map = collect_entries(op, ws_path, &forms).await?;
    let parsed = sql::parse_sql(sql_query)?;
    let tables = build_sql_tables(op, ws_path, &forms, &entries_map).await?;
    sql::filter_entries_by_sql(&tables, &parsed)
}

fn extract_sql_query(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.to_string()),
        Value::Object(map) => map
            .get("$sql")
            .or_else(|| map.get("sql"))
            .and_then(|v| v.as_str())
            .map(|text| text.to_string()),
        _ => None,
    }
}

fn matches_filters(entry: &Value, filters: &Map<String, Value>) -> Result<bool> {
    for (key, expected) in filters {
        let mut entry_value = entry.get(key).cloned();
        if entry_value.is_none() {
            entry_value = entry
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
            if let Some(tags) = entry.get("tags").and_then(|v| v.as_array()) {
                if !tags.iter().any(|v| v == expected) {
                    return Ok(false);
                }
                continue;
            }
        }

        match entry_value {
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
    let _ = op;
    let _ = ws_path;
    Ok(())
}

pub async fn update_entry_index(op: &Operator, ws_path: &str, entry_id: &str) -> Result<()> {
    let _ = op;
    let _ = ws_path;
    let _ = entry_id;
    Ok(())
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

fn parse_boolean(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn normalize_timestamp(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc).to_rfc3339())
}

fn normalize_timestamp_ns(value: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(value).ok().map(|dt| {
        dt.with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Nanos, false)
    })
}

fn normalize_time(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let formats = ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"];
    for format in formats {
        if let Ok(time) = NaiveTime::parse_from_str(trimmed, format) {
            let micros = time.nanosecond() / 1_000;
            if micros == 0 {
                return Some(time.format("%H:%M:%S").to_string());
            }
            return Some(format!("{}.{:06}", time.format("%H:%M:%S"), micros));
        }
    }
    None
}

fn normalize_binary(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let bytes = if let Some(rest) = trimmed.strip_prefix("base64:") {
        base64::engine::general_purpose::STANDARD
            .decode(rest.trim())
            .ok()?
    } else if let Some(rest) = trimmed.strip_prefix("hex:") {
        hex::decode(rest.trim()).ok()?
    } else if let Some(rest) = trimmed.strip_prefix("0x") {
        hex::decode(rest.trim()).ok()?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .ok()?
    };

    Some(format!(
        "base64:{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

fn parse_markdown_list(value: &str) -> Vec<Value> {
    let mut items = Vec::new();
    for line in value.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let item = if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("- [X] ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("- ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            rest
        } else if let Some(rest) = trimmed.strip_prefix("+ ") {
            rest
        } else {
            trimmed
        };
        if !item.is_empty() {
            items.push(Value::String(item.to_string()));
        }
    }
    items
}

fn parse_object_list(value: &Value) -> Option<Value> {
    let items = match value {
        Value::Array(items) => items.clone(),
        Value::String(raw) => serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|parsed| parsed.as_array().cloned())?,
        _ => return None,
    };

    let mut normalized = Vec::new();
    for item in items {
        let obj = item.as_object()?;
        let var_type = obj.get("type").and_then(|v| v.as_str())?;
        let name = obj.get("name").and_then(|v| v.as_str())?;
        let description = obj.get("description").and_then(|v| v.as_str())?;
        normalized.push(serde_json::json!({
            "type": var_type,
            "name": name,
            "description": description,
        }));
    }
    Some(Value::Array(normalized))
}

pub fn validate_properties(properties: &Value, entry_form: &Value) -> Result<(Value, Vec<Value>)> {
    let mut warnings = Vec::new();
    let mut casted = properties.clone();

    let fields = entry_form.get("fields");
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
            "number" | "double" => match raw_value {
                Value::Number(_) => Some(raw_value.clone()),
                Value::String(ref s) => s
                    .parse::<f64>()
                    .ok()
                    .map(|n| Value::Number(serde_json::Number::from_f64(n).unwrap())),
                _ => None,
            },
            "float" => match raw_value {
                Value::Number(_) => Some(raw_value.clone()),
                Value::String(ref s) => s
                    .parse::<f32>()
                    .ok()
                    .and_then(|n| serde_json::Number::from_f64(f64::from(n)))
                    .map(Value::Number),
                _ => None,
            },
            "integer" => match raw_value {
                Value::Number(num) => num
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .map(serde_json::Number::from),
                Value::String(ref s) => s.parse::<i32>().ok().map(serde_json::Number::from),
                _ => None,
            }
            .map(Value::Number),
            "long" => match raw_value {
                Value::Number(num) => num.as_i64().map(serde_json::Number::from),
                Value::String(ref s) => s.parse::<i64>().ok().map(serde_json::Number::from),
                _ => None,
            }
            .map(Value::Number),
            "date" => match raw_value {
                Value::String(ref s) => NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .ok()
                    .map(|d| Value::String(d.format("%Y-%m-%d").to_string())),
                _ => None,
            },
            "time" => match raw_value {
                Value::String(ref s) => normalize_time(s).map(Value::String),
                _ => None,
            },
            "timestamp" => match raw_value {
                Value::String(ref s) => normalize_timestamp(s).map(Value::String),
                _ => None,
            },
            "timestamp_tz" => match raw_value {
                Value::String(ref s) => normalize_timestamp(s).map(Value::String),
                _ => None,
            },
            "timestamp_ns" => match raw_value {
                Value::String(ref s) => normalize_timestamp_ns(s).map(Value::String),
                _ => None,
            },
            "timestamp_tz_ns" => match raw_value {
                Value::String(ref s) => normalize_timestamp_ns(s).map(Value::String),
                _ => None,
            },
            "uuid" => match raw_value {
                Value::String(ref s) => Uuid::parse_str(s)
                    .ok()
                    .map(|u| Value::String(u.to_string())),
                _ => None,
            },
            "binary" => match raw_value {
                Value::String(ref s) => normalize_binary(s).map(Value::String),
                _ => None,
            },
            "list" => match raw_value {
                Value::Array(_) => Some(raw_value.clone()),
                Value::String(ref s) => Some(Value::Array(parse_markdown_list(s))),
                _ => None,
            },
            "object_list" => parse_object_list(&raw_value),
            "boolean" => match raw_value {
                Value::Bool(_) => Some(raw_value.clone()),
                Value::String(ref s) => parse_boolean(s).map(Value::Bool),
                _ => None,
            },
            "markdown" | "string" | "row_reference" => Some(raw_value.clone()),
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

pub fn aggregate_stats(entries: &Map<String, Value>) -> Value {
    let mut form_stats: HashMap<String, Map<String, Value>> = HashMap::new();
    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    let mut uncategorized = 0usize;

    for record in entries.values() {
        let entry_form = record
            .get("form")
            .or_else(|| record.get("properties").and_then(|v| v.get("form")));

        if let Some(form_name) = entry_form.and_then(|v| v.as_str()) {
            let entry = form_stats.entry(form_name.to_string()).or_default();
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

    let mut form_stats_json: Map<String, Value> = form_stats
        .into_iter()
        .map(|(k, v)| (k, Value::Object(v)))
        .collect();
    form_stats_json.insert(
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
                "entry_count".to_string(),
                Value::Number((entries.len() as u64).into()),
            ),
            ("form_stats".to_string(), Value::Object(form_stats_json)),
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

async fn load_forms(op: &Operator, ws_path: &str) -> Result<HashMap<String, Value>> {
    let mut forms = HashMap::new();
    for form_name in crate::form::list_form_names(op, ws_path).await? {
        if let Ok(value) = crate::form::get_form(op, ws_path, &form_name).await {
            forms.insert(form_name, value);
        }
    }
    Ok(forms)
}

async fn collect_entries(
    op: &Operator,
    ws_path: &str,
    forms: &HashMap<String, Value>,
) -> Result<Map<String, Value>> {
    let mut entries = Map::new();
    let rows = entry::list_entry_rows(op, ws_path).await?;
    for (form_name, row) in rows {
        if let Some(record) = build_record(ws_path, &form_name, &row, forms).await? {
            entries.insert(row.entry_id.clone(), record);
        }
    }
    Ok(entries)
}

async fn build_record(
    ws_path: &str,
    form_name: &str,
    row: &entry::EntryRow,
    forms: &HashMap<String, Value>,
) -> Result<Option<Value>> {
    if row.deleted {
        return Ok(None);
    }

    let mut warnings = Vec::new();
    let mut properties = entry::merge_entry_fields(&row.fields, &row.extra_attributes);
    if let Some(form_def) = forms.get(form_name) {
        if let Ok((casted, warns)) = validate_properties(&properties, form_def) {
            properties = casted;
            warnings = warns;
        }
    }

    let word_count = compute_word_count(&serde_json::to_string(&properties)?);
    let record = serde_json::json!({
        "id": row.entry_id,
        "title": row.title,
        "form": form_name,
        "updated_at": row.updated_at,
        "space_id": ws_path.split('/').next_back().unwrap_or("").to_string(),
        "properties": properties,
        "word_count": word_count,
        "tags": row.tags,
        "links": row.links,
        "assets": row.assets,
        "checksum": row.integrity.checksum,
        "validation_warnings": Value::Array(warnings),
    });

    Ok(Some(record))
}

async fn build_sql_tables(
    op: &Operator,
    ws_path: &str,
    forms: &HashMap<String, Value>,
    entries_map: &Map<String, Value>,
) -> Result<HashMap<String, Vec<Value>>> {
    let mut tables: HashMap<String, Vec<Value>> = HashMap::new();
    tables.insert(
        "entries".to_string(),
        entries_map.values().cloned().collect::<Vec<_>>(),
    );

    for form_name in forms.keys() {
        let rows: Vec<Value> = entries_map
            .values()
            .filter(|entry| {
                entry
                    .get("form")
                    .and_then(|v| v.as_str())
                    .map(|form| form.eq_ignore_ascii_case(form_name))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        tables.insert(form_name.to_lowercase(), rows);
    }

    let mut entry_form_map: HashMap<String, String> = HashMap::new();
    for (entry_id, entry_value) in entries_map {
        if let Some(form) = entry_value.get("form").and_then(|v| v.as_str()) {
            entry_form_map.insert(entry_id.to_string(), form.to_string());
        }
    }

    let mut asset_rows = Vec::new();
    let entry_rows = entry::list_entry_rows(op, ws_path).await?;
    let mut link_rows = Vec::new();
    for (_form_name, row) in entry_rows {
        if row.deleted {
            continue;
        }
        for link_item in row.links {
            let source_form = entry_form_map.get(&link_item.source).cloned();
            let target_form = entry_form_map.get(&link_item.target).cloned();
            link_rows.push(serde_json::json!({
                "id": link_item.id,
                "source": link_item.source,
                "target": link_item.target,
                "kind": link_item.kind,
                "source_form": source_form,
                "target_form": target_form,
            }));
        }
        for asset in row.assets {
            if let Some(obj) = asset.as_object() {
                asset_rows.push(serde_json::json!({
                    "id": obj.get("id").cloned().unwrap_or(Value::Null),
                    "entry_id": row.entry_id,
                    "name": obj.get("name").cloned().unwrap_or(Value::Null),
                    "path": obj.get("path").cloned().unwrap_or(Value::Null),
                }));
            }
        }
    }
    tables.insert("links".to_string(), link_rows);
    tables.insert("assets".to_string(), asset_rows);

    Ok(tables)
}
