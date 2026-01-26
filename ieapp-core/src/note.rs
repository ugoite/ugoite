use crate::class;
use crate::iceberg_store;
use crate::index;
use crate::integrity::IntegrityProvider;
use crate::link::Link;
use anyhow::{anyhow, Result};
use arrow_array::builder::{ListBuilder, StringBuilder, StructBuilder};
use arrow_array::{
    Array, ArrayRef, BooleanArray, Date32Array, Float64Array, LargeListArray, LargeStringArray,
    ListArray, RecordBatch, StringArray, StructArray, TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Fields};
use chrono::Utc;
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

pub(crate) fn render_markdown(
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

pub(crate) fn render_markdown_for_class(
    title: &str,
    class_name: &str,
    tags: &[String],
    fields: &Value,
    class_def: &Value,
) -> String {
    let field_order = class_field_names(class_def);
    render_markdown(title, class_name, tags, fields, &field_order)
}

fn class_field_defs(class_def: &Value) -> Vec<(String, String)> {
    let mut defs = Vec::new();
    if let Some(fields) = class_def.get("fields") {
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

fn class_field_type_map(class_def: &Value) -> std::collections::HashMap<String, String> {
    class_field_defs(class_def)
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

fn list_attachments_array_from_values(
    attachments: &[Value],
    list_field: &arrow_schema::Field,
) -> Result<ArrayRef> {
    let element_field = list_element_field(list_field)?;
    let struct_fields = list_struct_fields_from_field(list_field)?;
    let struct_builder = StructBuilder::from_fields(struct_fields.clone(), attachments.len());
    let mut list_builder = ListBuilder::new(struct_builder).with_field(element_field);

    for attachment in attachments {
        let builder = list_builder.values();
        let attachment_obj = attachment.as_object();
        for (idx, field) in struct_fields.iter().enumerate() {
            let value = attachment_obj
                .and_then(|obj| obj.get(field.name()))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let field_builder = builder
                .field_builder::<StringBuilder>(idx)
                .ok_or_else(|| anyhow!("Invalid attachment field builder: {}", field.name()))?;
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

fn list_values_from_array(list_array: &dyn Array, row: usize) -> Result<Option<ArrayRef>> {
    match list_array.data_type() {
        DataType::List(_) => {
            let list_array = list_array
                .as_any()
                .downcast_ref::<ListArray>()
                .ok_or_else(|| anyhow!("Invalid list array"))?;
            if list_array.is_null(row) {
                Ok(None)
            } else {
                Ok(Some(list_array.value(row)))
            }
        }
        DataType::LargeList(_) => {
            let list_array = list_array
                .as_any()
                .downcast_ref::<LargeListArray>()
                .ok_or_else(|| anyhow!("Invalid large list array"))?;
            if list_array.is_null(row) {
                Ok(None)
            } else {
                Ok(Some(list_array.value(row)))
            }
        }
        other => Err(anyhow!("Invalid list type: {:?}", other)),
    }
}

fn list_strings_from_array(list_array: &dyn Array, row: usize) -> Result<Vec<String>> {
    let values = match list_values_from_array(list_array, row)? {
        Some(values) => values,
        None => return Ok(Vec::new()),
    };
    if let Some(array) = values.as_any().downcast_ref::<StringArray>() {
        let mut items = Vec::new();
        for i in 0..array.len() {
            if !array.is_null(i) {
                items.push(array.value(i).to_string());
            }
        }
        Ok(items)
    } else if let Some(array) = values.as_any().downcast_ref::<LargeStringArray>() {
        let mut items = Vec::new();
        for i in 0..array.len() {
            if !array.is_null(i) {
                items.push(array.value(i).to_string());
            }
        }
        Ok(items)
    } else {
        Err(anyhow!("Invalid list string array"))
    }
}

fn list_links_from_array(list_array: &dyn Array, row: usize, source: &str) -> Result<Vec<Link>> {
    let values = match list_values_from_array(list_array, row)? {
        Some(values) => values,
        None => return Ok(Vec::new()),
    };
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

fn list_attachments_from_array(list_array: &dyn Array, row: usize) -> Result<Vec<Value>> {
    let values = match list_values_from_array(list_array, row)? {
        Some(values) => values,
        None => return Ok(Vec::new()),
    };
    let struct_array = values
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or_else(|| anyhow!("Invalid attachments struct array"))?;

    let id_col = struct_array
        .column_by_name("id")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
    let name_col = struct_array
        .column_by_name("name")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());
    let path_col = struct_array
        .column_by_name("path")
        .and_then(|col| col.as_any().downcast_ref::<StringArray>());

    let mut attachments = Vec::new();
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
        attachments.push(serde_json::json!({
            "id": id,
            "name": name,
            "path": path,
        }));
    }

    Ok(attachments)
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

fn canvas_from_array(array: &dyn Array, row: usize) -> Result<Value> {
    match array.data_type() {
        DataType::Struct(_) => {
            let struct_array = array
                .as_any()
                .downcast_ref::<StructArray>()
                .ok_or_else(|| anyhow!("Invalid canvas struct array"))?;
            Ok(canvas_from_struct_array(struct_array, row))
        }
        DataType::Null => Ok(Value::Object(Map::new())),
        other => Err(anyhow!("Invalid canvas_position type: {:?}", other)),
    }
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

fn integrity_from_array(array: &dyn Array, row: usize) -> Result<IntegrityPayload> {
    match array.data_type() {
        DataType::Struct(_) => {
            let struct_array = array
                .as_any()
                .downcast_ref::<StructArray>()
                .ok_or_else(|| anyhow!("Invalid integrity struct array"))?;
            Ok(integrity_from_struct_array(struct_array, row))
        }
        DataType::Null => Ok(IntegrityPayload::default()),
        other => Err(anyhow!("Invalid integrity type: {:?}", other)),
    }
}

fn struct_array_from_fields(
    class_def: &Value,
    fields_value: &Value,
    struct_fields: &Fields,
) -> Result<ArrayRef> {
    let type_map = class_field_type_map(class_def);
    let mut arrays = Vec::new();

    for field in struct_fields {
        let name = field.name();
        let field_type = type_map.get(name).map(String::as_str).unwrap_or("string");
        let value = fields_value.get(name);

        let array: ArrayRef = match field_type {
            "number" => {
                let number = value.and_then(|v| v.as_f64());
                Arc::new(Float64Array::from(vec![number]))
            }
            "date" => {
                let days = value.and_then(|v| v.as_str()).and_then(date_to_days);
                Arc::new(Date32Array::from(vec![days]))
            }
            "list" => list_array_from_values(value, field.as_ref())?,
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

async fn latest_revision_for_note(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    class_def: &Value,
    note_id: &str,
) -> Result<Option<RevisionRow>> {
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches, class_def)?;
    let mut selected: Option<RevisionRow> = None;
    for row in rows {
        if row.note_id != note_id {
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

fn value_from_struct_array(struct_array: &StructArray, row: usize, class_def: &Value) -> Value {
    let type_map = class_field_type_map(class_def);
    let mut map = Map::new();

    for (idx, field) in struct_array.fields().iter().enumerate() {
        let name = field.name();
        let field_type = type_map.get(name).map(String::as_str).unwrap_or("string");
        let column = struct_array.column(idx);

        let value = match field_type {
            "number" => column
                .as_any()
                .downcast_ref::<Float64Array>()
                .and_then(|array| {
                    if array.is_null(row) {
                        None
                    } else {
                        serde_json::Number::from_f64(array.value(row)).map(Value::Number)
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

fn value_from_array(array: &dyn Array, row: usize, class_def: &Value) -> Result<Value> {
    match array.data_type() {
        DataType::Struct(_) => {
            let struct_array = array
                .as_any()
                .downcast_ref::<StructArray>()
                .ok_or_else(|| anyhow!("Invalid fields struct array"))?;
            if struct_array.is_null(row) {
                Ok(Value::Object(Map::new()))
            } else {
                Ok(value_from_struct_array(struct_array, row, class_def))
            }
        }
        DataType::Null => Ok(Value::Object(Map::new())),
        other => Err(anyhow!("Invalid fields type: {:?}", other)),
    }
}

fn note_rows_from_batches(
    batches: &[RecordBatch],
    class_def: &Value,
    class_name: &str,
) -> Result<Vec<NoteRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        let note_ids = column_as::<StringArray>(batch, "note_id")?;
        let titles = column_as::<StringArray>(batch, "title")?;
        let tags = batch
            .column_by_name("tags")
            .ok_or_else(|| anyhow!("Missing column: tags"))?;
        let links = batch
            .column_by_name("links")
            .ok_or_else(|| anyhow!("Missing column: links"))?;
        let canvas_positions = batch
            .column_by_name("canvas_position")
            .ok_or_else(|| anyhow!("Missing column: canvas_position"))?;
        let created_at = column_as::<TimestampMicrosecondArray>(batch, "created_at")?;
        let updated_at = column_as::<TimestampMicrosecondArray>(batch, "updated_at")?;
        let fields = batch
            .column_by_name("fields")
            .ok_or_else(|| anyhow!("Missing column: fields"))?;
        let attachments = batch
            .column_by_name("attachments")
            .ok_or_else(|| anyhow!("Missing column: attachments"))?;
        let integrity = batch
            .column_by_name("integrity")
            .ok_or_else(|| anyhow!("Missing column: integrity"))?;
        let deleted = column_as::<BooleanArray>(batch, "deleted")?;
        let deleted_at = column_as::<TimestampMicrosecondArray>(batch, "deleted_at")?;

        for row_idx in 0..batch.num_rows() {
            if note_ids.is_null(row_idx) {
                continue;
            }

            let tags_value = list_strings_from_array(tags.as_ref(), row_idx)?;
            let links_value =
                list_links_from_array(links.as_ref(), row_idx, note_ids.value(row_idx))?;
            let canvas_value = canvas_from_array(canvas_positions.as_ref(), row_idx)?;
            let attachments_value = list_attachments_from_array(attachments.as_ref(), row_idx)?;
            let integrity_value = integrity_from_array(integrity.as_ref(), row_idx)?;
            let fields_value = value_from_array(fields.as_ref(), row_idx, class_def)?;

            let deleted_at_value = if deleted_at.is_null(row_idx) {
                None
            } else {
                Some(from_timestamp_micros(deleted_at.value(row_idx)))
            };

            rows.push(NoteRow {
                note_id: note_ids.value(row_idx).to_string(),
                title: if titles.is_null(row_idx) {
                    "".to_string()
                } else {
                    titles.value(row_idx).to_string()
                },
                class: class_name.to_string(),
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
                revision_id: "".to_string(),
                parent_revision_id: None,
                attachments: attachments_value,
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
    class_def: &Value,
) -> Result<Vec<RevisionRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        let revision_ids = column_as::<StringArray>(batch, "revision_id")?;
        let note_ids = column_as::<StringArray>(batch, "note_id")?;
        let parent_revision_ids = column_as::<StringArray>(batch, "parent_revision_id")?;
        let timestamps = column_as::<TimestampMicrosecondArray>(batch, "timestamp")?;
        let authors = column_as::<StringArray>(batch, "author")?;
        let fields = batch
            .column_by_name("fields")
            .ok_or_else(|| anyhow!("Missing column: fields"))?;
        let checksums = column_as::<StringArray>(batch, "markdown_checksum")?;
        let integrity = batch
            .column_by_name("integrity")
            .ok_or_else(|| anyhow!("Missing column: integrity"))?;
        let restored_from = column_as::<StringArray>(batch, "restored_from")?;

        for row_idx in 0..batch.num_rows() {
            if revision_ids.is_null(row_idx) {
                continue;
            }

            let integrity_value = integrity_from_array(integrity.as_ref(), row_idx)?;
            let fields_value = value_from_array(fields.as_ref(), row_idx, class_def)?;

            rows.push(RevisionRow {
                revision_id: revision_ids.value(row_idx).to_string(),
                note_id: if note_ids.is_null(row_idx) {
                    "".to_string()
                } else {
                    note_ids.value(row_idx).to_string()
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

fn note_row_to_record_batch(
    row: &NoteRow,
    class_def: &Value,
    table_schema: &iceberg::spec::Schema,
) -> Result<RecordBatch> {
    let arrow_schema = Arc::new(schema_to_arrow_schema(table_schema)?);

    let mut arrays = Vec::new();
    for field in arrow_schema.fields() {
        let array: ArrayRef = match field.name().as_str() {
            "note_id" => Arc::new(StringArray::from(vec![Some(row.note_id.clone())])),
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
                struct_array_from_fields(class_def, &row.fields, &struct_fields)?
            }
            "attachments" => list_attachments_array_from_values(&row.attachments, field.as_ref())?,
            "integrity" => {
                let struct_fields = struct_fields_from_field(field.as_ref())?;
                struct_array_from_integrity(&row.integrity, &struct_fields)?
            }
            "deleted" => Arc::new(BooleanArray::from(vec![Some(row.deleted)])),
            "deleted_at" => Arc::new(TimestampMicrosecondArray::from(vec![row
                .deleted_at
                .map(to_timestamp_micros)])),
            other => {
                return Err(anyhow!("Unexpected column in notes schema: {}", other));
            }
        };
        arrays.push(array);
    }

    RecordBatch::try_new(arrow_schema, arrays).map_err(|e| anyhow!("Record batch error: {}", e))
}

fn revision_row_to_record_batch(
    row: &RevisionRow,
    class_def: &Value,
    table_schema: &iceberg::spec::Schema,
) -> Result<RecordBatch> {
    let arrow_schema = Arc::new(schema_to_arrow_schema(table_schema)?);

    let mut arrays = Vec::new();
    for field in arrow_schema.fields() {
        let array: ArrayRef = match field.name().as_str() {
            "revision_id" => Arc::new(StringArray::from(vec![Some(row.revision_id.clone())])),
            "note_id" => Arc::new(StringArray::from(vec![Some(row.note_id.clone())])),
            "parent_revision_id" => {
                Arc::new(StringArray::from(vec![row.parent_revision_id.clone()]))
            }
            "timestamp" => Arc::new(TimestampMicrosecondArray::from(vec![Some(
                to_timestamp_micros(row.timestamp),
            )])),
            "author" => Arc::new(StringArray::from(vec![Some(row.author.clone())])),
            "fields" => {
                let struct_fields = struct_fields_from_field(field.as_ref())?;
                struct_array_from_fields(class_def, &row.fields, &struct_fields)?
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

async fn append_note_row_to_table(
    catalog: &MemoryCatalog,
    table: &iceberg::table::Table,
    row: &NoteRow,
    class_def: &Value,
) -> Result<()> {
    let batch = note_row_to_record_batch(row, class_def, table.metadata().current_schema())?;
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
    class_def: &Value,
) -> Result<()> {
    let batch = revision_row_to_record_batch(row, class_def, table.metadata().current_schema())?;
    let data_file = write_record_batch(table, batch).await?;
    let tx = Transaction::new(table);
    let action = tx.fast_append().add_data_files(vec![data_file]);
    let tx = action.apply(tx)?;
    tx.commit(catalog).await?;
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
    let rows = list_note_rows(op, ws_path).await?;
    Ok(rows
        .into_iter()
        .find(|(_, row)| row.note_id == note_id)
        .map(|(class_name, _)| class_name))
}

pub(crate) async fn read_note_row(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    note_id: &str,
) -> Result<NoteRow> {
    let class_def = class::read_class_definition(op, ws_path, class_name).await?;
    let (_, table) = iceberg_store::load_notes_table(op, ws_path, class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = note_rows_from_batches(&batches, &class_def, class_name)?;
    let mut selected: Option<NoteRow> = None;
    for row in rows {
        if row.note_id != note_id {
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
    let mut selected = selected.ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    if let Some(latest) =
        latest_revision_for_note(op, ws_path, class_name, &class_def, note_id).await?
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

pub(crate) async fn write_note_row(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    note_id: &str,
    row: &NoteRow,
) -> Result<()> {
    let _ = note_id;
    let (catalog, table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_notes_table(op, ws_path, class_name).await?;
    let class_def = class::read_class_definition(op, ws_path, class_name).await?;
    append_note_row_to_table(catalog.as_ref(), &table, row, &class_def).await
}

pub(crate) async fn list_note_rows(op: &Operator, ws_path: &str) -> Result<Vec<(String, NoteRow)>> {
    let mut latest: std::collections::HashMap<String, (String, NoteRow)> =
        std::collections::HashMap::new();
    for class_name in list_class_names(op, ws_path).await? {
        let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
        let (_, table) = iceberg_store::load_notes_table(op, ws_path, &class_name).await?;
        let batches = scan_table_batches(&table).await?;
        let rows = note_rows_from_batches(&batches, &class_def, &class_name)?;
        for row in rows {
            let entry = latest.get(&row.note_id);
            let should_replace = match entry {
                Some((_, existing)) => row.updated_at >= existing.updated_at,
                None => true,
            };
            if should_replace {
                latest.insert(row.note_id.clone(), (class_name.clone(), row));
            }
        }
    }
    Ok(latest.into_values().collect())
}

pub(crate) async fn list_class_note_rows(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    class_def: &Value,
) -> Result<Vec<NoteRow>> {
    let (_, table) = iceberg_store::load_notes_table(op, ws_path, class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = note_rows_from_batches(&batches, class_def, class_name)?;
    let mut latest: std::collections::HashMap<String, NoteRow> = std::collections::HashMap::new();
    for row in rows {
        let entry = latest.get(&row.note_id);
        let should_replace = match entry {
            Some(existing) => row.updated_at >= existing.updated_at,
            None => true,
        };
        if should_replace {
            latest.insert(row.note_id.clone(), row);
        }
    }
    Ok(latest.into_values().collect())
}

pub(crate) async fn list_class_revision_rows(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    class_def: &Value,
) -> Result<Vec<RevisionRow>> {
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, class_name).await?;
    let batches = scan_table_batches(&table).await?;
    revision_rows_from_batches(&batches, class_def)
}

pub(crate) async fn append_revision_row_for_class(
    op: &Operator,
    ws_path: &str,
    class_name: &str,
    row: &RevisionRow,
    class_def: &Value,
) -> Result<()> {
    let (catalog, table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, class_name).await?;
    append_revision_row_to_table(catalog.as_ref(), &table, row, class_def).await
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
    if let Some(obj) = properties.as_object() {
        for (key, value) in obj {
            if class_set.contains(key) {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(obj) = casted.as_object() {
        for (key, value) in obj {
            if class_set.contains(key) {
                fields.insert(key.clone(), value.clone());
            }
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
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    append_revision_row_to_table(rev_catalog.as_ref(), &rev_table, &revision, &class_def).await?;

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
    if let Some(obj) = properties.as_object() {
        for (key, value) in obj {
            if class_set.contains(key) {
                fields.insert(key.clone(), value.clone());
            }
        }
    }
    if let Some(obj) = casted.as_object() {
        for (key, value) in obj {
            if class_set.contains(key) {
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
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    append_revision_row_to_table(rev_catalog.as_ref(), &rev_table, &revision, &class_def).await?;

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

    let mut delete_ts = now_ts();
    if delete_ts <= row.updated_at {
        delete_ts = row.updated_at + 0.001;
    }
    if hard_delete {
        row.deleted = true;
        row.deleted_at = Some(delete_ts);
        row.updated_at = delete_ts;
        write_note_row(op, ws_path, &class_name, note_id, &row).await?;
        return Ok(());
    }

    row.deleted = true;
    row.deleted_at = Some(delete_ts);
    row.updated_at = delete_ts;
    write_note_row(op, ws_path, &class_name, note_id, &row).await?;
    Ok(())
}

pub async fn get_note_history(op: &Operator, ws_path: &str, note_id: &str) -> Result<Value> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches, &class_def)?;

    let mut revisions = rows
        .into_iter()
        .filter(|rev| rev.note_id == note_id)
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
    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches, &class_def)?;
    let revision = rows
        .into_iter()
        .find(|rev| rev.note_id == note_id && rev.revision_id == revision_id);

    let revision = revision
        .ok_or_else(|| anyhow!("Revision {} not found for note {}", revision_id, note_id))?;
    Ok(serde_json::to_value(revision)?)
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
    let class_def = class::read_class_definition(op, ws_path, &class_name).await?;
    let (_, revisions_table) =
        iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    let batches = scan_table_batches(&revisions_table).await?;
    let revisions = revision_rows_from_batches(&batches, &class_def)?;
    let revision = revisions
        .into_iter()
        .find(|rev| rev.note_id == note_id && rev.revision_id == revision_id)
        .ok_or_else(|| anyhow!("Revision {} not found for note {}", revision_id, note_id))?;

    let mut row = read_note_row(op, ws_path, &class_name, note_id).await?;
    let new_rev_id = Uuid::new_v4().to_string();
    let mut timestamp = now_ts();
    if timestamp <= row.updated_at {
        timestamp = row.updated_at + 0.001;
    }

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
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    append_revision_row_to_table(
        rev_catalog.as_ref(),
        &rev_table,
        &restore_revision,
        &class_def,
    )
    .await?;

    Ok(serde_json::json!({
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }))
}
