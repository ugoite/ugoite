use crate::form;
use crate::iceberg_store;
use crate::index;
use crate::integrity::IntegrityProvider;
use crate::link::Link;
use anyhow::{anyhow, Result};
use arrow_array::builder::{FixedSizeBinaryBuilder, ListBuilder, StringBuilder, StructBuilder};
use arrow_array::{
    Array, ArrayRef, BooleanArray, Date32Array, FixedSizeBinaryArray, Float32Array, Float64Array,
    Int32Array, Int64Array, LargeBinaryArray, ListArray, RecordBatch, StringArray, StructArray,
    Time64MicrosecondArray, TimestampMicrosecondArray, TimestampNanosecondArray,
};
use arrow_schema::{DataType, Fields};
use base64::Engine as _;
use chrono::{DateTime, NaiveTime, SecondsFormat, Timelike, Utc};
use futures::TryStreamExt;
use iceberg::arrow::schema_to_arrow_schema;
use iceberg::arrow::ArrowReaderBuilder;
use iceberg::spec::DataFile;
use iceberg::transaction::ApplyTransactionAction;
use iceberg::transaction::Transaction;
use iceberg::writer::file_writer::{FileWriter, FileWriterBuilder, ParquetWriterBuilder};
use iceberg::MemoryCatalog;
use opendal::Operator;
use parquet::file::properties::WriterProperties;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::sync::Arc;
use url::Url;
use uuid::Uuid;

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
pub struct EntryRow {
    pub entry_id: String,
    pub title: String,
    pub form: String,
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

fn form_field_defs(form_def: &Value) -> Vec<(String, String)> {
    let mut defs = Vec::new();
    if let Some(fields) = form_def.get("fields") {
        match fields {
            Value::Object(map) => {
                for (name, def) in map {
                    let field_type = def
                        .get("type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("string")
                        .to_string();
                    defs.push((name.clone(), field_type));
                }
            }
            Value::Array(items) => {
                for item in items {
                    if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                        let field_type = item
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("string")
                            .to_string();
                        defs.push((name.to_string(), field_type));
                    }
                }
            }
            _ => {}
        }
    }
    defs
}

fn form_field_type_map(form_def: &Value) -> std::collections::HashMap<String, String> {
    form_field_defs(form_def)
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>()
}

fn date_to_days(value: &str) -> Option<i32> {
    let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)?;
    let days = date.signed_duration_since(epoch).num_days();
    i32::try_from(days).ok()
}

fn days_to_date(days: i32) -> Option<String> {
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)?;
    let date = epoch.checked_add_signed(chrono::Duration::days(days as i64))?;
    Some(date.format("%Y-%m-%d").to_string())
}

fn parse_timestamp_to_micros(value: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp_micros())
}

fn timestamp_micros_to_string(micros: i64) -> Option<String> {
    DateTime::<Utc>::from_timestamp_micros(micros).map(|dt| dt.to_rfc3339())
}

fn parse_time_to_micros(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    let formats = ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"];
    for format in formats {
        if let Ok(time) = NaiveTime::parse_from_str(trimmed, format) {
            let secs = i64::from(time.num_seconds_from_midnight());
            let micros = i64::from(time.nanosecond() / 1_000);
            return Some(secs * 1_000_000 + micros);
        }
    }
    None
}

fn time_micros_to_string(micros: i64) -> Option<String> {
    if micros < 0 {
        return None;
    }
    let secs = (micros / 1_000_000) as u32;
    let sub_micros = (micros % 1_000_000) as u32;
    let nanos = sub_micros * 1_000;
    let time = NaiveTime::from_num_seconds_from_midnight_opt(secs, nanos)?;
    if sub_micros == 0 {
        Some(time.format("%H:%M:%S").to_string())
    } else {
        Some(format!("{}.{:06}", time.format("%H:%M:%S"), sub_micros))
    }
}

fn parse_timestamp_to_nanos(value: &str) -> Option<i64> {
    let dt = DateTime::parse_from_rfc3339(value).ok()?;
    let secs = dt.timestamp();
    let nanos = i64::from(dt.timestamp_subsec_nanos());
    Some(secs.saturating_mul(1_000_000_000) + nanos)
}

fn timestamp_nanos_to_string(nanos: i64) -> Option<String> {
    let secs = nanos.div_euclid(1_000_000_000);
    let sub_nanos = nanos.rem_euclid(1_000_000_000) as u32;
    let dt = DateTime::<Utc>::from_timestamp(secs, sub_nanos)?;
    Some(dt.to_rfc3339_opts(SecondsFormat::Nanos, false))
}

fn parse_binary_string(value: &str) -> Option<Vec<u8>> {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix("base64:") {
        return base64::engine::general_purpose::STANDARD
            .decode(rest.trim())
            .ok();
    }
    if let Some(rest) = trimmed.strip_prefix("hex:") {
        return hex::decode(rest.trim()).ok();
    }
    if let Some(rest) = trimmed.strip_prefix("0x") {
        return hex::decode(rest.trim()).ok();
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .ok()
}

fn binary_to_base64(value: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(value);
    format!("base64:{}", encoded)
}

fn list_element_field(list_field: &arrow_schema::Field) -> Result<arrow_schema::FieldRef> {
    match list_field.data_type() {
        DataType::List(inner) => Ok(inner.clone()),
        _ => Err(anyhow!("Expected list field: {}", list_field.name())),
    }
}

fn list_array_from_strings(
    values: &[String],
    list_field: &arrow_schema::Field,
) -> Result<ArrayRef> {
    let element_field = list_element_field(list_field)?;
    let mut builder = ListBuilder::new(StringBuilder::new()).with_field(element_field);
    for value in values {
        builder.values().append_value(value);
    }
    builder.append(true);
    Ok(Arc::new(builder.finish()))
}

fn list_array_from_values(
    values: Option<&Value>,
    list_field: &arrow_schema::Field,
) -> Result<ArrayRef> {
    let element_field = list_element_field(list_field)?;
    let mut builder = ListBuilder::new(StringBuilder::new()).with_field(element_field);
    if let Some(Value::Array(items)) = values {
        for item in items {
            let rendered = match item {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => item.to_string(),
            };
            builder.values().append_value(rendered);
        }
        builder.append(true);
    } else {
        builder.append(false);
    }
    Ok(Arc::new(builder.finish()))
}

fn object_list_array_from_values(
    values: Option<&Value>,
    list_field: &arrow_schema::Field,
) -> Result<ArrayRef> {
    let element_field = list_element_field(list_field)?;
    let struct_fields = list_struct_fields_from_field(list_field)?;
    let count = values
        .and_then(|value| value.as_array().map(|items| items.len()))
        .unwrap_or(0);
    let struct_builder = StructBuilder::from_fields(struct_fields.clone(), count);
    let mut list_builder = ListBuilder::new(struct_builder).with_field(element_field);

    if let Some(Value::Array(items)) = values {
        for item in items {
            let builder = list_builder.values();
            let obj = item.as_object();
            for (idx, field) in struct_fields.iter().enumerate() {
                let value = obj
                    .and_then(|map| map.get(field.name()))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let field_builder =
                    builder.field_builder::<StringBuilder>(idx).ok_or_else(|| {
                        anyhow!("Invalid object_list field builder: {}", field.name())
                    })?;
                field_builder.append_value(value);
            }
            builder.append(true);
        }
        list_builder.append(true);
    } else {
        list_builder.append(false);
    }

    Ok(Arc::new(list_builder.finish()))
}

fn list_struct_fields_from_field(list_field: &arrow_schema::Field) -> Result<Fields> {
    let element_field = list_element_field(list_field)?;
    match element_field.data_type() {
        DataType::Struct(fields) => Ok(fields.clone()),
        _ => Err(anyhow!(
            "Expected list<struct> field: {}",
            list_field.name()
        )),
    }
}

fn struct_fields_from_field(field: &arrow_schema::Field) -> Result<Fields> {
    match field.data_type() {
        DataType::Struct(fields) => Ok(fields.clone()),
        _ => Err(anyhow!("Expected struct field: {}", field.name())),
    }
}

fn list_links_array_from_links(
    links: &[Link],
    list_field: &arrow_schema::Field,
) -> Result<ArrayRef> {
    let element_field = list_element_field(list_field)?;
    let struct_fields = list_struct_fields_from_field(list_field)?;
    let struct_builder = StructBuilder::from_fields(struct_fields.clone(), links.len());
    let mut list_builder = ListBuilder::new(struct_builder).with_field(element_field);

    for link in links {
        let builder = list_builder.values();
        for (idx, field) in struct_fields.iter().enumerate() {
            let value = match field.name().as_str() {
                "id" => link.id.as_str(),
                "target" => link.target.as_str(),
                "kind" => link.kind.as_str(),
                other => {
                    return Err(anyhow!("Unexpected link field: {}", other));
                }
            };
            let field_builder = builder
                .field_builder::<StringBuilder>(idx)
                .ok_or_else(|| anyhow!("Invalid link field builder: {}", field.name()))?;
            field_builder.append_value(value);
        }
        builder.append(true);
    }
    list_builder.append(true);
    Ok(Arc::new(list_builder.finish()))
}

fn list_assets_array_from_values(
    assets: &[Value],
    list_field: &arrow_schema::Field,
) -> Result<ArrayRef> {
    let element_field = list_element_field(list_field)?;
    let struct_fields = list_struct_fields_from_field(list_field)?;
    let struct_builder = StructBuilder::from_fields(struct_fields.clone(), assets.len());
    let mut list_builder = ListBuilder::new(struct_builder).with_field(element_field);

    for asset in assets {
        let builder = list_builder.values();
        let asset_obj = asset.as_object();
        for (idx, field) in struct_fields.iter().enumerate() {
            let value = asset_obj
                .and_then(|obj| obj.get(field.name()))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let field_builder = builder
                .field_builder::<StringBuilder>(idx)
                .ok_or_else(|| anyhow!("Invalid asset field builder: {}", field.name()))?;
            field_builder.append_value(value);
        }
        builder.append(true);
    }
    list_builder.append(true);
    Ok(Arc::new(list_builder.finish()))
}

fn struct_array_from_canvas(canvas: &Value, struct_fields: &Fields) -> Result<ArrayRef> {
    let mut arrays = Vec::new();
    for field in struct_fields {
        let value = canvas.get(field.name()).and_then(|v| v.as_f64());
        let array: ArrayRef = Arc::new(Float64Array::from(vec![value]));
        arrays.push(array);
    }
    let struct_array = StructArray::try_new(struct_fields.clone(), arrays, None)
        .map_err(|e| anyhow!("Failed to build canvas struct array: {}", e))?;
    Ok(Arc::new(struct_array))
}

fn struct_array_from_integrity(
    integrity: &IntegrityPayload,
    struct_fields: &Fields,
) -> Result<ArrayRef> {
    let mut arrays = Vec::new();
    for field in struct_fields {
        let value = match field.name().as_str() {
            "checksum" => integrity.checksum.as_str(),
            "signature" => integrity.signature.as_str(),
            other => return Err(anyhow!("Unexpected integrity field: {}", other)),
        };
        let array: ArrayRef = Arc::new(StringArray::from(vec![Some(value)]));
        arrays.push(array);
    }
    let struct_array = StructArray::try_new(struct_fields.clone(), arrays, None)
        .map_err(|e| anyhow!("Failed to build integrity struct array: {}", e))?;
    Ok(Arc::new(struct_array))
}

fn normalize_extra_attributes(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> =
                map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            let mut normalized = Map::new();
            for (key, value) in entries {
                normalized.insert(key, value);
            }
            Value::Object(normalized)
        }
        _ => value.clone(),
    }
}

fn extra_attributes_to_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Object(map) if map.is_empty() => None,
        _ => serde_json::to_string(&normalize_extra_attributes(value)).ok(),
    }
}

fn extra_attributes_from_string(raw: Option<&str>) -> Value {
    match raw {
        Some(value) if !value.trim().is_empty() => {
            serde_json::from_str(value).unwrap_or_else(|_| Value::Object(Map::new()))
        }
        _ => Value::Object(Map::new()),
    }
}

fn list_strings_from_array(list_array: &ListArray, row: usize) -> Vec<String> {
    if list_array.is_null(row) {
        return Vec::new();
    }
    let values = list_array.value(row);
    let values = values.as_any().downcast_ref::<StringArray>().map(|array| {
        let mut items = Vec::new();
        for i in 0..array.len() {
            if !array.is_null(i) {
                items.push(array.value(i).to_string());
            }
        }
        items
    });
    values.unwrap_or_default()
}

fn list_links_from_array(list_array: &ListArray, row: usize, source: &str) -> Result<Vec<Link>> {
    if list_array.is_null(row) {
        return Ok(Vec::new());
    }
    let values = list_array.value(row);
    let struct_array = values
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or_else(|| anyhow!("Invalid links struct array"))?;

    let id_col = struct_array
        .column_by_name("id")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
    let target_col = struct_array
        .column_by_name("target")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
    let kind_col = struct_array
        .column_by_name("kind")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());

    let mut links = Vec::new();
    for idx in 0..struct_array.len() {
        let id = id_col
            .and_then(|col| {
                if col.is_null(idx) {
                    None
                } else {
                    Some(col.value(idx))
                }
            })
            .unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let target = target_col
            .and_then(|col| {
                if col.is_null(idx) {
                    None
                } else {
                    Some(col.value(idx))
                }
            })
            .unwrap_or("");
        let kind = kind_col
            .and_then(|col| {
                if col.is_null(idx) {
                    None
                } else {
                    Some(col.value(idx))
                }
            })
            .unwrap_or("");
        links.push(Link {
            id: id.to_string(),
            source: source.to_string(),
            target: target.to_string(),
            kind: kind.to_string(),
        });
    }

    Ok(links)
}

fn list_assets_from_array(list_array: &ListArray, row: usize) -> Result<Vec<Value>> {
    if list_array.is_null(row) {
        return Ok(Vec::new());
    }
    let values = list_array.value(row);
    let struct_array = values
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or_else(|| anyhow!("Invalid assets struct array"))?;

    let id_col = struct_array
        .column_by_name("id")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
    let name_col = struct_array
        .column_by_name("name")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
    let path_col = struct_array
        .column_by_name("path")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());

    let mut assets = Vec::new();
    for idx in 0..struct_array.len() {
        let id = id_col
            .and_then(|col| {
                if col.is_null(idx) {
                    None
                } else {
                    Some(col.value(idx))
                }
            })
            .unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let name = name_col
            .and_then(|col| {
                if col.is_null(idx) {
                    None
                } else {
                    Some(col.value(idx))
                }
            })
            .unwrap_or("");
        let path = path_col
            .and_then(|col| {
                if col.is_null(idx) {
                    None
                } else {
                    Some(col.value(idx))
                }
            })
            .unwrap_or("");
        assets.push(serde_json::json!({
            "id": id,
            "name": name,
            "path": path,
        }));
    }

    Ok(assets)
}

fn canvas_from_struct_array(struct_array: &StructArray, row: usize) -> Value {
    if struct_array.is_null(row) {
        return Value::Object(Map::new());
    }
    let x_col = struct_array
        .column_by_name("x")
        .and_then(|col| col.as_any().downcast_ref::<Float64Array>());
    let y_col = struct_array
        .column_by_name("y")
        .and_then(|col| col.as_any().downcast_ref::<Float64Array>());
    let mut map = Map::new();
    if let Some(col) = x_col {
        if !col.is_null(row) {
            map.insert(
                "x".to_string(),
                Value::Number(serde_json::Number::from_f64(col.value(row)).unwrap()),
            );
        }
    }
    if let Some(col) = y_col {
        if !col.is_null(row) {
            map.insert(
                "y".to_string(),
                Value::Number(serde_json::Number::from_f64(col.value(row)).unwrap()),
            );
        }
    }
    Value::Object(map)
}

fn integrity_from_struct_array(struct_array: &StructArray, row: usize) -> IntegrityPayload {
    if struct_array.is_null(row) {
        return IntegrityPayload::default();
    }
    let checksum = struct_array
        .column_by_name("checksum")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .and_then(|col| {
            if col.is_null(row) {
                None
            } else {
                Some(col.value(row))
            }
        })
        .unwrap_or("");
    let signature = struct_array
        .column_by_name("signature")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .and_then(|col| {
            if col.is_null(row) {
                None
            } else {
                Some(col.value(row))
            }
        })
        .unwrap_or("");
    IntegrityPayload {
        checksum: checksum.to_string(),
        signature: signature.to_string(),
    }
}

fn struct_array_from_fields(
    form_def: &Value,
    fields_value: &Value,
    struct_fields: &Fields,
) -> Result<ArrayRef> {
    let type_map = form_field_type_map(form_def);
    let mut arrays = Vec::new();

    for field in struct_fields {
        let name = field.name();
        let field_type = type_map.get(name).map(String::as_str).unwrap_or("string");
        let value = fields_value.get(name);

        let array: ArrayRef = match field_type {
            "number" | "double" => {
                let number = value.and_then(|v| v.as_f64());
                Arc::new(Float64Array::from(vec![number]))
            }
            "float" => {
                let number = value.and_then(|v| v.as_f64()).map(|n| n as f32);
                Arc::new(Float32Array::from(vec![number]))
            }
            "integer" => {
                let number = value
                    .and_then(|v| v.as_i64())
                    .and_then(|v| i32::try_from(v).ok());
                Arc::new(Int32Array::from(vec![number]))
            }
            "long" => {
                let number = value.and_then(|v| v.as_i64());
                Arc::new(Int64Array::from(vec![number]))
            }
            "boolean" => {
                let bool_value = value.and_then(|v| v.as_bool());
                Arc::new(BooleanArray::from(vec![bool_value]))
            }
            "date" => {
                let days = value.and_then(|v| v.as_str()).and_then(date_to_days);
                Arc::new(Date32Array::from(vec![days]))
            }
            "time" => {
                let micros = value
                    .and_then(|v| v.as_str())
                    .and_then(parse_time_to_micros);
                Arc::new(Time64MicrosecondArray::from(vec![micros]))
            }
            "timestamp" => {
                let micros = value
                    .and_then(|v| v.as_str())
                    .and_then(parse_timestamp_to_micros);
                Arc::new(TimestampMicrosecondArray::from(vec![micros]))
            }
            "timestamp_tz" => {
                let micros = value
                    .and_then(|v| v.as_str())
                    .and_then(parse_timestamp_to_micros);
                Arc::new(TimestampMicrosecondArray::from(vec![micros]))
            }
            "timestamp_ns" => {
                let nanos = value
                    .and_then(|v| v.as_str())
                    .and_then(parse_timestamp_to_nanos);
                Arc::new(TimestampNanosecondArray::from(vec![nanos]))
            }
            "timestamp_tz_ns" => {
                let nanos = value
                    .and_then(|v| v.as_str())
                    .and_then(parse_timestamp_to_nanos);
                Arc::new(TimestampNanosecondArray::from(vec![nanos]))
            }
            "uuid" => {
                let bytes = value
                    .and_then(|v| v.as_str())
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .map(|uuid| uuid.into_bytes());
                let mut builder = FixedSizeBinaryBuilder::with_capacity(1, 16);
                if let Some(bytes) = bytes {
                    builder
                        .append_value(bytes)
                        .map_err(|e| anyhow!("Failed to build uuid array: {}", e))?;
                } else {
                    builder.append_null();
                }
                Arc::new(builder.finish())
            }
            "binary" => {
                let bytes = value.and_then(|v| v.as_str()).and_then(parse_binary_string);
                Arc::new(LargeBinaryArray::from_opt_vec(vec![bytes.as_deref()]))
            }
            "list" => list_array_from_values(value, field.as_ref())?,
            "object_list" => object_list_array_from_values(value, field.as_ref())?,
            "markdown" | "string" | "row_reference" => {
                let string_value = value.and_then(|v| v.as_str()).map(|s| s.to_string());
                Arc::new(StringArray::from(vec![string_value]))
            }
            _ => {
                let string_value = value.and_then(|v| v.as_str()).map(|s| s.to_string());
                Arc::new(StringArray::from(vec![string_value]))
            }
        };

        arrays.push(array);
    }

    let struct_array = StructArray::try_new(struct_fields.clone(), arrays, None)
        .map_err(|e| anyhow!("Failed to build fields struct array: {}", e))?;
    Ok(Arc::new(struct_array))
}

async fn scan_table_batches(table: &iceberg::table::Table) -> Result<Vec<RecordBatch>> {
    let scan = table.scan().build()?;
    let tasks = scan.plan_files().await?;
    let reader = ArrowReaderBuilder::new(table.file_io().clone()).build();
    let mut stream = reader.read(tasks)?;
    let mut batches = Vec::new();
    while let Some(batch) = stream.try_next().await? {
        batches.push(batch);
    }
    Ok(batches)
}

async fn latest_revision_for_entry(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    form_def: &Value,
    entry_id: &str,
) -> Result<Option<RevisionRow>> {
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, form_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches, form_def)?;
    let mut selected: Option<RevisionRow> = None;
    for row in rows {
        if row.entry_id != entry_id {
            continue;
        }
        let replace = match &selected {
            Some(existing) => row.timestamp >= existing.timestamp,
            None => true,
        };
        if replace {
            selected = Some(row);
        }
    }
    Ok(selected)
}

fn column_as<'a, T: 'static>(batch: &'a RecordBatch, name: &str) -> Result<&'a T> {
    let column = batch
        .column_by_name(name)
        .ok_or_else(|| anyhow!("Missing column: {}", name))?;
    column
        .as_any()
        .downcast_ref::<T>()
        .ok_or_else(|| anyhow!("Invalid column type for {}", name))
}

fn value_from_struct_array(struct_array: &StructArray, row: usize, form_def: &Value) -> Value {
    let type_map = form_field_type_map(form_def);
    let mut map = Map::new();

    for (idx, field) in struct_array.fields().iter().enumerate() {
        let name = field.name();
        let field_type = type_map.get(name).map(String::as_str).unwrap_or("string");
        let column = struct_array.column(idx);

        let value = match field_type {
            "number" | "double" => {
                column
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .and_then(|array| {
                        if array.is_null(row) {
                            None
                        } else {
                            serde_json::Number::from_f64(array.value(row)).map(Value::Number)
                        }
                    })
            }
            "float" => column
                .as_any()
                .downcast_ref::<Float32Array>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        serde_json::Number::from_f64(f64::from(array.value(row))).map(Value::Number)
                    }
                }),
            "integer" => column
                .as_any()
                .downcast_ref::<Int32Array>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        Some(Value::Number(array.value(row).into()))
                    }
                }),
            "long" => column
                .as_any()
                .downcast_ref::<Int64Array>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        Some(Value::Number(array.value(row).into()))
                    }
                }),
            "boolean" => column
                .as_any()
                .downcast_ref::<BooleanArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        Some(Value::Bool(array.value(row)))
                    }
                }),
            "date" => column
                .as_any()
                .downcast_ref::<Date32Array>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        days_to_date(array.value(row)).map(Value::String)
                    }
                }),
            "time" => column
                .as_any()
                .downcast_ref::<Time64MicrosecondArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        time_micros_to_string(array.value(row)).map(Value::String)
                    }
                }),
            "timestamp" => column
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        timestamp_micros_to_string(array.value(row)).map(Value::String)
                    }
                }),
            "timestamp_tz" => column
                .as_any()
                .downcast_ref::<TimestampMicrosecondArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        timestamp_micros_to_string(array.value(row)).map(Value::String)
                    }
                }),
            "timestamp_ns" => column
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        timestamp_nanos_to_string(array.value(row)).map(Value::String)
                    }
                }),
            "timestamp_tz_ns" => column
                .as_any()
                .downcast_ref::<TimestampNanosecondArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        timestamp_nanos_to_string(array.value(row)).map(Value::String)
                    }
                }),
            "uuid" => column
                .as_any()
                .downcast_ref::<FixedSizeBinaryArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        Uuid::from_slice(array.value(row))
                            .ok()
                            .map(|uuid| Value::String(uuid.to_string()))
                    }
                }),
            "binary" => column
                .as_any()
                .downcast_ref::<LargeBinaryArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        Some(Value::String(binary_to_base64(array.value(row))))
                    }
                }),
            "list" => column
                .as_any()
                .downcast_ref::<ListArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        let values = array.value(row);
                        let values = values.as_any().downcast_ref::<StringArray>()?;
                        let mut items = Vec::new();
                        for i in 0..values.len() {
                            if !values.is_null(i) {
                                items.push(Value::String(values.value(i).to_string()));
                            }
                        }
                        Some(Value::Array(items))
                    }
                }),
            "object_list" => column
                .as_any()
                .downcast_ref::<ListArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        return None;
                    }
                    let values = array.value(row);
                    let struct_array = values.as_any().downcast_ref::<StructArray>()?;
                    let type_col = struct_array
                        .column_by_name("type")
                        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
                    let name_col = struct_array
                        .column_by_name("name")
                        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
                    let desc_col = struct_array
                        .column_by_name("description")
                        .and_then(|col| col.as_any().downcast_ref::<StringArray>());

                    let mut items = Vec::new();
                    for idx in 0..struct_array.len() {
                        let var_type = type_col
                            .and_then(|col| {
                                if col.is_null(idx) {
                                    None
                                } else {
                                    Some(col.value(idx))
                                }
                            })
                            .unwrap_or("");
                        let name = name_col
                            .and_then(|col| {
                                if col.is_null(idx) {
                                    None
                                } else {
                                    Some(col.value(idx))
                                }
                            })
                            .unwrap_or("");
                        let description = desc_col
                            .and_then(|col| {
                                if col.is_null(idx) {
                                    None
                                } else {
                                    Some(col.value(idx))
                                }
                            })
                            .unwrap_or("");
                        items.push(serde_json::json!({
                            "type": var_type,
                            "name": name,
                            "description": description,
                        }));
                    }
                    Some(Value::Array(items))
                }),
            "markdown" | "string" | "row_reference" => column
                .as_any()
                .downcast_ref::<StringArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        Some(Value::String(array.value(row).to_string()))
                    }
                }),
            _ => column
                .as_any()
                .downcast_ref::<StringArray>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        Some(Value::String(array.value(row).to_string()))
                    }
                }),
        };

        if let Some(value) = value {
            map.insert(name.to_string(), value);
        }
    }

    Value::Object(map)
}

fn entry_rows_from_batches(
    batches: &[RecordBatch],
    form_def: &Value,
    form_name: &str,
) -> Result<Vec<EntryRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        let entry_ids = column_as::<StringArray>(batch, "entry_id")?;
        let titles = column_as::<StringArray>(batch, "title")?;
        let tags = column_as::<ListArray>(batch, "tags")?;
        let links = column_as::<ListArray>(batch, "links")?;
        let canvas_positions = column_as::<StructArray>(batch, "canvas_position")?;
        let created_at = column_as::<TimestampMicrosecondArray>(batch, "created_at")?;
        let updated_at = column_as::<TimestampMicrosecondArray>(batch, "updated_at")?;
        let fields = column_as::<StructArray>(batch, "fields")?;
        let extra_attributes = batch
            .column_by_name("extra_attributes")
            .and_then(|col| col.as_any().downcast_ref::<StringArray>());
        let assets = column_as::<ListArray>(batch, "assets")?;
        let integrity = column_as::<StructArray>(batch, "integrity")?;
        let deleted = column_as::<BooleanArray>(batch, "deleted")?;
        let deleted_at = column_as::<TimestampMicrosecondArray>(batch, "deleted_at")?;

        for row_idx in 0..batch.num_rows() {
            if entry_ids.is_null(row_idx) {
                continue;
            }

            let tags_value = list_strings_from_array(tags, row_idx);
            let links_value = list_links_from_array(links, row_idx, entry_ids.value(row_idx))?;
            let canvas_value = canvas_from_struct_array(canvas_positions, row_idx);
            let assets_value = list_assets_from_array(assets, row_idx)?;
            let integrity_value = integrity_from_struct_array(integrity, row_idx);

            let fields_value = if fields.is_null(row_idx) {
                Value::Object(Map::new())
            } else {
                value_from_struct_array(fields, row_idx, form_def)
            };
            let extra_attributes_value = match extra_attributes {
                Some(array) => {
                    if array.is_null(row_idx) {
                        Value::Object(Map::new())
                    } else {
                        extra_attributes_from_string(Some(array.value(row_idx)))
                    }
                }
                None => Value::Object(Map::new()),
            };

            let deleted_at_value = if deleted_at.is_null(row_idx) {
                None
            } else {
                Some(from_timestamp_micros(deleted_at.value(row_idx)))
            };

            rows.push(EntryRow {
                entry_id: entry_ids.value(row_idx).to_string(),
                title: if titles.is_null(row_idx) {
                    "".to_string()
                } else {
                    titles.value(row_idx).to_string()
                },
                form: form_name.to_string(),
                tags: tags_value,
                links: links_value,
                canvas_position: canvas_value,
                created_at: if created_at.is_null(row_idx) {
                    0.0
                } else {
                    from_timestamp_micros(created_at.value(row_idx))
                },
                updated_at: if updated_at.is_null(row_idx) {
                    0.0
                } else {
                    from_timestamp_micros(updated_at.value(row_idx))
                },
                fields: fields_value,
                extra_attributes: extra_attributes_value,
                revision_id: "".to_string(),
                parent_revision_id: None,
                assets: assets_value,
                integrity: integrity_value,
                deleted: !deleted.is_null(row_idx) && deleted.value(row_idx),
                deleted_at: deleted_at_value,
                author: "".to_string(),
            });
        }
    }
    Ok(rows)
}

fn revision_rows_from_batches(
    batches: &[RecordBatch],
    form_def: &Value,
) -> Result<Vec<RevisionRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        let revision_ids = column_as::<StringArray>(batch, "revision_id")?;
        let entry_ids = column_as::<StringArray>(batch, "entry_id")?;
        let parent_revision_ids = column_as::<StringArray>(batch, "parent_revision_id")?;
        let timestamps = column_as::<TimestampMicrosecondArray>(batch, "timestamp")?;
        let authors = column_as::<StringArray>(batch, "author")?;
        let fields = column_as::<StructArray>(batch, "fields")?;
        let extra_attributes = batch
            .column_by_name("extra_attributes")
            .and_then(|col| col.as_any().downcast_ref::<StringArray>());
        let checksums = column_as::<StringArray>(batch, "markdown_checksum")?;
        let integrity = column_as::<StructArray>(batch, "integrity")?;
        let restored_from = column_as::<StringArray>(batch, "restored_from")?;

        for row_idx in 0..batch.num_rows() {
            if revision_ids.is_null(row_idx) {
                continue;
            }

            let integrity_value = integrity_from_struct_array(integrity, row_idx);

            let fields_value = if fields.is_null(row_idx) {
                Value::Object(Map::new())
            } else {
                value_from_struct_array(fields, row_idx, form_def)
            };
            let extra_attributes_value = match extra_attributes {
                Some(array) => {
                    if array.is_null(row_idx) {
                        Value::Object(Map::new())
                    } else {
                        extra_attributes_from_string(Some(array.value(row_idx)))
                    }
                }
                None => Value::Object(Map::new()),
            };

            rows.push(RevisionRow {
                revision_id: revision_ids.value(row_idx).to_string(),
                entry_id: if entry_ids.is_null(row_idx) {
                    "".to_string()
                } else {
                    entry_ids.value(row_idx).to_string()
                },
                parent_revision_id: if parent_revision_ids.is_null(row_idx) {
                    None
                } else {
                    Some(parent_revision_ids.value(row_idx).to_string())
                },
                timestamp: if timestamps.is_null(row_idx) {
                    0.0
                } else {
                    from_timestamp_micros(timestamps.value(row_idx))
                },
                author: if authors.is_null(row_idx) {
                    "".to_string()
                } else {
                    authors.value(row_idx).to_string()
                },
                fields: fields_value,
                extra_attributes: extra_attributes_value,
                markdown_checksum: if checksums.is_null(row_idx) {
                    "".to_string()
                } else {
                    checksums.value(row_idx).to_string()
                },
                integrity: integrity_value,
                restored_from: if restored_from.is_null(row_idx) {
                    None
                } else {
                    Some(restored_from.value(row_idx).to_string())
                },
            });
        }
    }
    Ok(rows)
}

fn entry_row_to_record_batch(
    row: &EntryRow,
    form_def: &Value,
    table_schema: &iceberg::spec::Schema,
) -> Result<RecordBatch> {
    let arrow_schema = Arc::new(schema_to_arrow_schema(table_schema)?);

    let mut arrays = Vec::new();
    for field in arrow_schema.fields() {
        let array: ArrayRef = match field.name().as_str() {
            "entry_id" => Arc::new(StringArray::from(vec![Some(row.entry_id.clone())])),
            "title" => Arc::new(StringArray::from(vec![Some(row.title.clone())])),
            "tags" => list_array_from_strings(&row.tags, field.as_ref())?,
            "links" => list_links_array_from_links(&row.links, field.as_ref())?,
            "canvas_position" => {
                let struct_fields = struct_fields_from_field(field.as_ref())?;
                struct_array_from_canvas(&row.canvas_position, &struct_fields)?
            }
            "created_at" => Arc::new(TimestampMicrosecondArray::from(vec![Some(
                to_timestamp_micros(row.created_at),
            )])),
            "updated_at" => Arc::new(TimestampMicrosecondArray::from(vec![Some(
                to_timestamp_micros(row.updated_at),
            )])),
            "fields" => {
                let struct_fields = struct_fields_from_field(field.as_ref())?;
                struct_array_from_fields(form_def, &row.fields, &struct_fields)?
            }
            "extra_attributes" => {
                let json_value = extra_attributes_to_string(&row.extra_attributes);
                Arc::new(StringArray::from(vec![json_value]))
            }
            "assets" => list_assets_array_from_values(&row.assets, field.as_ref())?,
            "integrity" => {
                let struct_fields = struct_fields_from_field(field.as_ref())?;
                struct_array_from_integrity(&row.integrity, &struct_fields)?
            }
            "deleted" => Arc::new(BooleanArray::from(vec![Some(row.deleted)])),
            "deleted_at" => Arc::new(TimestampMicrosecondArray::from(vec![row
                .deleted_at
                .map(to_timestamp_micros)])),
            other => {
                return Err(anyhow!("Unexpected column in entries schema: {}", other));
            }
        };
        arrays.push(array);
    }

    RecordBatch::try_new(arrow_schema, arrays).map_err(|e| anyhow!("Record batch error: {}", e))
}

fn revision_row_to_record_batch(
    row: &RevisionRow,
    form_def: &Value,
    table_schema: &iceberg::spec::Schema,
) -> Result<RecordBatch> {
    let arrow_schema = Arc::new(schema_to_arrow_schema(table_schema)?);

    let mut arrays = Vec::new();
    for field in arrow_schema.fields() {
        let array: ArrayRef = match field.name().as_str() {
            "revision_id" => Arc::new(StringArray::from(vec![Some(row.revision_id.clone())])),
            "entry_id" => Arc::new(StringArray::from(vec![Some(row.entry_id.clone())])),
            "parent_revision_id" => {
                Arc::new(StringArray::from(vec![row.parent_revision_id.clone()]))
            }
            "timestamp" => Arc::new(TimestampMicrosecondArray::from(vec![Some(
                to_timestamp_micros(row.timestamp),
            )])),
            "author" => Arc::new(StringArray::from(vec![Some(row.author.clone())])),
            "fields" => {
                let struct_fields = struct_fields_from_field(field.as_ref())?;
                struct_array_from_fields(form_def, &row.fields, &struct_fields)?
            }
            "extra_attributes" => {
                let json_value = extra_attributes_to_string(&row.extra_attributes);
                Arc::new(StringArray::from(vec![json_value]))
            }
            "markdown_checksum" => {
                Arc::new(StringArray::from(vec![Some(row.markdown_checksum.clone())]))
            }
            "integrity" => {
                let struct_fields = struct_fields_from_field(field.as_ref())?;
                struct_array_from_integrity(&row.integrity, &struct_fields)?
            }
            "restored_from" => Arc::new(StringArray::from(vec![row.restored_from.clone()])),
            other => {
                return Err(anyhow!("Unexpected column in revisions schema: {}", other));
            }
        };
        arrays.push(array);
    }

    RecordBatch::try_new(arrow_schema, arrays).map_err(|e| anyhow!("Record batch error: {}", e))
}

async fn write_record_batch(table: &iceberg::table::Table, batch: RecordBatch) -> Result<DataFile> {
    let schema = table.metadata().current_schema();
    let props = WriterProperties::builder().build();
    let output_path = format!(
        "{}/data/{}.parquet",
        table.metadata().location(),
        Uuid::new_v4()
    );
    let output_file = table.file_io().new_output(&output_path)?;
    let mut writer = ParquetWriterBuilder::new(props, schema.clone())
        .build(output_file)
        .await?;
    writer.write(&batch).await?;
    let builders = writer.close().await?;
    let mut data_files = Vec::new();
    for builder in builders {
        data_files.push(
            builder
                .build()
                .map_err(|e| anyhow!("Data file build error: {}", e))?,
        );
    }
    data_files
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("No data files produced by writer"))
}

async fn append_entry_row_to_table(
    catalog: &MemoryCatalog,
    table: &iceberg::table::Table,
    row: &EntryRow,
    form_def: &Value,
) -> Result<()> {
    let batch = entry_row_to_record_batch(row, form_def, table.metadata().current_schema())?;
    let data_file = write_record_batch(table, batch).await?;
    let tx = Transaction::new(table);
    let action = tx.fast_append().add_data_files(vec![data_file]);
    let tx = action.apply(tx)?;
    tx.commit(catalog).await?;
    Ok(())
}

async fn append_revision_row_to_table(
    catalog: &MemoryCatalog,
    table: &iceberg::table::Table,
    row: &RevisionRow,
    form_def: &Value,
) -> Result<()> {
    let batch = revision_row_to_record_batch(row, form_def, table.metadata().current_schema())?;
    let data_file = write_record_batch(table, batch).await?;
    let tx = Transaction::new(table);
    let action = tx.fast_append().add_data_files(vec![data_file]);
    let tx = action.apply(tx)?;
    tx.commit(catalog).await?;
    Ok(())
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
    let form_def = form::read_form_definition(op, ws_path, form_name).await?;
    let (_, table) = iceberg_store::load_entries_table(op, ws_path, form_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = entry_rows_from_batches(&batches, &form_def, form_name)?;
    let mut selected: Option<EntryRow> = None;
    for row in rows {
        if row.entry_id != entry_id {
            continue;
        }
        let replace = match &selected {
            Some(existing) => row.updated_at >= existing.updated_at,
            None => true,
        };
        if replace {
            selected = Some(row);
        }
    }
    let mut selected = selected.ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    if let Some(latest) =
        latest_revision_for_entry(op, ws_path, form_name, &form_def, entry_id).await?
    {
        selected.revision_id = latest.revision_id;
        selected.parent_revision_id = latest.parent_revision_id;
        selected.author = latest.author;
        if selected.integrity.checksum.is_empty() {
            selected.integrity = latest.integrity;
        }
    }

    Ok(selected)
}

pub(crate) async fn write_entry_row(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    entry_id: &str,
    row: &EntryRow,
) -> Result<()> {
    let _ = entry_id;
    let (catalog, table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_entries_table(op, ws_path, form_name).await?;
    let form_def = form::read_form_definition(op, ws_path, form_name).await?;
    append_entry_row_to_table(catalog.as_ref(), &table, row, &form_def).await
}

pub(crate) async fn list_entry_rows(
    op: &Operator,
    ws_path: &str,
) -> Result<Vec<(String, EntryRow)>> {
    let mut latest: std::collections::HashMap<String, (String, EntryRow)> =
        std::collections::HashMap::new();
    for form_name in list_form_names(op, ws_path).await? {
        let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
        let (_, table) = iceberg_store::load_entries_table(op, ws_path, &form_name).await?;
        let batches = scan_table_batches(&table).await?;
        let rows = entry_rows_from_batches(&batches, &form_def, &form_name)?;
        for row in rows {
            let entry = latest.get(&row.entry_id);
            let should_replace = match entry {
                Some((_, existing)) => row.updated_at >= existing.updated_at,
                None => true,
            };
            if should_replace {
                latest.insert(row.entry_id.clone(), (form_name.clone(), row));
            }
        }
    }
    Ok(latest.into_values().collect())
}

pub(crate) async fn list_form_entry_rows(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    form_def: &Value,
) -> Result<Vec<EntryRow>> {
    let (_, table) = iceberg_store::load_entries_table(op, ws_path, form_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = entry_rows_from_batches(&batches, form_def, form_name)?;
    let mut latest: std::collections::HashMap<String, EntryRow> = std::collections::HashMap::new();
    for row in rows {
        let entry = latest.get(&row.entry_id);
        let should_replace = match entry {
            Some(existing) => row.updated_at >= existing.updated_at,
            None => true,
        };
        if should_replace {
            latest.insert(row.entry_id.clone(), row);
        }
    }
    Ok(latest.into_values().collect())
}

pub(crate) async fn list_form_revision_rows(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    form_def: &Value,
) -> Result<Vec<RevisionRow>> {
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, form_name).await?;
    let batches = scan_table_batches(&table).await?;
    revision_rows_from_batches(&batches, form_def)
}

pub(crate) async fn append_revision_row_for_form(
    op: &Operator,
    ws_path: &str,
    form_name: &str,
    row: &RevisionRow,
    form_def: &Value,
) -> Result<()> {
    let (catalog, table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, form_name).await?;
    append_revision_row_to_table(catalog.as_ref(), &table, row, form_def).await
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
    if find_entry_form(op, ws_path, entry_id).await?.is_some() {
        return Err(anyhow!("Entry already exists: {}", entry_id));
    }

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
        canvas_position: Value::Object(Map::new()),
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
    };

    write_entry_row(op, ws_path, &form_name, entry_id, &entry_row).await?;

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
    };
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &form_name).await?;
    append_revision_row_to_table(rev_catalog.as_ref(), &rev_table, &revision, &form_def).await?;

    let ws_id = ws_path
        .trim_end_matches('/')
        .split('/')
        .next_back()
        .unwrap_or(ws_path)
        .to_string();

    Ok(EntryMeta {
        id: entry_id.to_string(),
        space_id: ws_id,
        title,
        form: Some(form_name),
        tags: entry_row.tags.clone(),
        links: entry_row.links.clone(),
        canvas_position: entry_row.canvas_position.clone(),
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
            "canvas_position": row.canvas_position,
            "created_at": row.created_at,
            "updated_at": row.updated_at,
        }));
    }
    Ok(entries)
}

pub async fn get_entry(op: &Operator, ws_path: &str, entry_id: &str) -> Result<Value> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    let row = read_entry_row(op, ws_path, &form_name, entry_id).await?;
    if row.deleted {
        return Err(anyhow!("Entry not found: {}", entry_id));
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
        "canvas_position": row.canvas_position,
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
        .ok_or_else(|| anyhow!("Entry content not found: {}", entry_id))?;
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
        .ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    let mut row = read_entry_row(op, ws_path, &form_name, entry_id).await?;

    if let Some(expected_parent) = parent_revision_id {
        if row.revision_id != expected_parent {
            return Err(anyhow!(
                "Revision conflict: expected {}, got {}",
                expected_parent,
                row.revision_id
            ));
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
    row.author = author.to_string();
    row.integrity = IntegrityPayload {
        checksum: checksum.clone(),
        signature: signature.clone(),
    };
    row.assets = assets.unwrap_or_else(|| row.assets.clone());

    write_entry_row(op, ws_path, &form_name, entry_id, &row).await?;

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
    };
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &form_name).await?;
    append_revision_row_to_table(rev_catalog.as_ref(), &rev_table, &revision, &form_def).await?;

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
        .ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    let mut row = read_entry_row(op, ws_path, &form_name, entry_id).await?;

    let mut delete_ts = now_ts();
    if delete_ts <= row.updated_at {
        delete_ts = row.updated_at + 0.001;
    }
    if hard_delete {
        row.deleted = true;
        row.deleted_at = Some(delete_ts);
        row.updated_at = delete_ts;
        write_entry_row(op, ws_path, &form_name, entry_id, &row).await?;
        return Ok(());
    }

    row.deleted = true;
    row.deleted_at = Some(delete_ts);
    row.updated_at = delete_ts;
    write_entry_row(op, ws_path, &form_name, entry_id, &row).await?;
    Ok(())
}

pub async fn get_entry_history(op: &Operator, ws_path: &str, entry_id: &str) -> Result<Value> {
    let form_name = find_entry_form(op, ws_path, entry_id)
        .await?
        .ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, &form_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches, &form_def)?;

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
        .ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, &form_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches, &form_def)?;
    let revision = rows
        .into_iter()
        .find(|rev| rev.entry_id == entry_id && rev.revision_id == revision_id);

    let revision = revision
        .ok_or_else(|| anyhow!("Revision {} not found for entry {}", revision_id, entry_id))?;
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
        .ok_or_else(|| anyhow!("Entry not found: {}", entry_id))?;
    let form_def = form::read_form_definition(op, ws_path, &form_name).await?;
    let (_, revisions_table) = iceberg_store::load_revisions_table(op, ws_path, &form_name).await?;
    let batches = scan_table_batches(&revisions_table).await?;
    let revisions = revision_rows_from_batches(&batches, &form_def)?;
    let revision = revisions
        .into_iter()
        .find(|rev| rev.entry_id == entry_id && rev.revision_id == revision_id)
        .ok_or_else(|| anyhow!("Revision {} not found for entry {}", revision_id, entry_id))?;

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
    row.updated_at = timestamp;
    row.fields = revision.fields.clone();
    row.extra_attributes = revision.extra_attributes.clone();
    row.integrity = IntegrityPayload {
        checksum: checksum.clone(),
        signature: signature.clone(),
    };
    row.author = author.to_string();
    write_entry_row(op, ws_path, &form_name, entry_id, &row).await?;

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
    };
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &form_name).await?;
    append_revision_row_to_table(
        rev_catalog.as_ref(),
        &rev_table,
        &restore_revision,
        &form_def,
    )
    .await?;

    Ok(serde_json::json!({
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }))
}
