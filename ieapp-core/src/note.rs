use crate::class;
use crate::iceberg_store;
use crate::index;
use crate::integrity::IntegrityProvider;
use crate::link::Link;
use anyhow::{anyhow, Result};
use arrow_array::builder::{ListBuilder, StringBuilder};
use arrow_array::{
    Array, ArrayRef, BooleanArray, Date32Array, Float64Array, ListArray, RecordBatch, StringArray,
    StructArray, TimestampMicrosecondArray,
};
use arrow_schema::Fields;
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

fn parse_json_or_default<T>(value: &str, default: T) -> T
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(value).unwrap_or(default)
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

fn list_array_from_values(values: Option<&Value>) -> ArrayRef {
    let mut builder = ListBuilder::new(StringBuilder::new());
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
    Arc::new(builder.finish())
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
            "list" => list_array_from_values(value),
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

fn note_rows_from_batches(batches: &[RecordBatch]) -> Result<Vec<NoteRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        let note_ids = column_as::<StringArray>(batch, "note_id")?;
        let titles = column_as::<StringArray>(batch, "title")?;
        let class_names = column_as::<StringArray>(batch, "class")?;
        let tags = column_as::<StringArray>(batch, "tags")?;
        let links = column_as::<StringArray>(batch, "links")?;
        let canvas_positions = column_as::<StringArray>(batch, "canvas_position")?;
        let created_at = column_as::<TimestampMicrosecondArray>(batch, "created_at")?;
        let updated_at = column_as::<TimestampMicrosecondArray>(batch, "updated_at")?;
        let fields = column_as::<StringArray>(batch, "fields")?;
        let revision_ids = column_as::<StringArray>(batch, "revision_id")?;
        let parent_revision_ids = column_as::<StringArray>(batch, "parent_revision_id")?;
        let attachments = column_as::<StringArray>(batch, "attachments")?;
        let integrity = column_as::<StringArray>(batch, "integrity")?;
        let deleted = column_as::<BooleanArray>(batch, "deleted")?;
        let deleted_at = column_as::<TimestampMicrosecondArray>(batch, "deleted_at")?;
        let author = column_as::<StringArray>(batch, "author")?;

        for row_idx in 0..batch.num_rows() {
            if note_ids.is_null(row_idx) {
                continue;
            }
            let class_name = if class_names.is_null(row_idx) {
                "".to_string()
            } else {
                class_names.value(row_idx).to_string()
            };

            let tags_value = if tags.is_null(row_idx) {
                Vec::<String>::new()
            } else {
                parse_json_or_default(tags.value(row_idx), Vec::<String>::new())
            };
            let links_value = if links.is_null(row_idx) {
                Vec::<Link>::new()
            } else {
                parse_json_or_default(links.value(row_idx), Vec::<Link>::new())
            };
            let canvas_value = if canvas_positions.is_null(row_idx) {
                Value::Object(Map::new())
            } else {
                parse_json_or_default(canvas_positions.value(row_idx), Value::Object(Map::new()))
            };
            let attachments_value = if attachments.is_null(row_idx) {
                Vec::<Value>::new()
            } else {
                parse_json_or_default(attachments.value(row_idx), Vec::<Value>::new())
            };
            let integrity_value = if integrity.is_null(row_idx) {
                IntegrityPayload::default()
            } else {
                parse_json_or_default(integrity.value(row_idx), IntegrityPayload::default())
            };

            let fields_value = if fields.is_null(row_idx) {
                Value::Object(Map::new())
            } else {
                serde_json::from_str(fields.value(row_idx))
                    .unwrap_or_else(|_| Value::Object(Map::new()))
            };

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
                class: class_name,
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
                revision_id: if revision_ids.is_null(row_idx) {
                    "".to_string()
                } else {
                    revision_ids.value(row_idx).to_string()
                },
                parent_revision_id: if parent_revision_ids.is_null(row_idx) {
                    None
                } else {
                    Some(parent_revision_ids.value(row_idx).to_string())
                },
                attachments: attachments_value,
                integrity: integrity_value,
                deleted: !deleted.is_null(row_idx) && deleted.value(row_idx),
                deleted_at: deleted_at_value,
                author: if author.is_null(row_idx) {
                    "".to_string()
                } else {
                    author.value(row_idx).to_string()
                },
            });
        }
    }
    Ok(rows)
}

fn revision_rows_from_batches(batches: &[RecordBatch]) -> Result<Vec<RevisionRow>> {
    let mut rows = Vec::new();
    for batch in batches {
        let revision_ids = column_as::<StringArray>(batch, "revision_id")?;
        let note_ids = column_as::<StringArray>(batch, "note_id")?;
        let parent_revision_ids = column_as::<StringArray>(batch, "parent_revision_id")?;
        let timestamps = column_as::<TimestampMicrosecondArray>(batch, "timestamp")?;
        let authors = column_as::<StringArray>(batch, "author")?;
        let fields = column_as::<StringArray>(batch, "fields")?;
        let checksums = column_as::<StringArray>(batch, "markdown_checksum")?;
        let integrity = column_as::<StringArray>(batch, "integrity")?;
        let restored_from = column_as::<StringArray>(batch, "restored_from")?;

        for row_idx in 0..batch.num_rows() {
            if revision_ids.is_null(row_idx) {
                continue;
            }

            let integrity_value = if integrity.is_null(row_idx) {
                IntegrityPayload::default()
            } else {
                parse_json_or_default(integrity.value(row_idx), IntegrityPayload::default())
            };

            let fields_value = if fields.is_null(row_idx) {
                Value::Object(Map::new())
            } else {
                serde_json::from_str(fields.value(row_idx))
                    .unwrap_or_else(|_| Value::Object(Map::new()))
            };

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
    table_schema: &iceberg::spec::Schema,
) -> Result<RecordBatch> {
    let arrow_schema = Arc::new(schema_to_arrow_schema(table_schema)?);

    let mut arrays = Vec::new();
    for field in arrow_schema.fields() {
        let array: ArrayRef = match field.name().as_str() {
            "note_id" => Arc::new(StringArray::from(vec![Some(row.note_id.clone())])),
            "title" => Arc::new(StringArray::from(vec![Some(row.title.clone())])),
            "class" => Arc::new(StringArray::from(vec![Some(row.class.clone())])),
            "tags" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.tags,
            )?)])),
            "links" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.links,
            )?)])),
            "canvas_position" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.canvas_position,
            )?)])),
            "created_at" => Arc::new(TimestampMicrosecondArray::from(vec![Some(
                to_timestamp_micros(row.created_at),
            )])),
            "updated_at" => Arc::new(TimestampMicrosecondArray::from(vec![Some(
                to_timestamp_micros(row.updated_at),
            )])),
            "fields" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.fields,
            )?)])),
            "revision_id" => Arc::new(StringArray::from(vec![Some(row.revision_id.clone())])),
            "parent_revision_id" => {
                Arc::new(StringArray::from(vec![row.parent_revision_id.clone()]))
            }
            "attachments" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.attachments,
            )?)])),
            "integrity" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.integrity,
            )?)])),
            "deleted" => Arc::new(BooleanArray::from(vec![Some(row.deleted)])),
            "deleted_at" => Arc::new(TimestampMicrosecondArray::from(vec![row
                .deleted_at
                .map(to_timestamp_micros)])),
            "author" => Arc::new(StringArray::from(vec![Some(row.author.clone())])),
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
            "fields" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.fields,
            )?)])),
            "markdown_checksum" => {
                Arc::new(StringArray::from(vec![Some(row.markdown_checksum.clone())]))
            }
            "integrity" => Arc::new(StringArray::from(vec![Some(serde_json::to_string(
                &row.integrity,
            )?)])),
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
) -> Result<()> {
    let batch = note_row_to_record_batch(row, table.metadata().current_schema())?;
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
) -> Result<()> {
    let batch = revision_row_to_record_batch(row, table.metadata().current_schema())?;
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
    let (_, table) = iceberg_store::load_notes_table(op, ws_path, class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = note_rows_from_batches(&batches)?;
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
    selected.ok_or_else(|| anyhow!("Note not found: {}", note_id))
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
    append_note_row_to_table(catalog.as_ref(), &table, row).await
}

pub(crate) async fn list_note_rows(op: &Operator, ws_path: &str) -> Result<Vec<(String, NoteRow)>> {
    let mut latest: std::collections::HashMap<String, (String, NoteRow)> =
        std::collections::HashMap::new();
    for class_name in list_class_names(op, ws_path).await? {
        let (_, table) = iceberg_store::load_notes_table(op, ws_path, &class_name).await?;
        let batches = scan_table_batches(&table).await?;
        let rows = note_rows_from_batches(&batches)?;
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
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    append_revision_row_to_table(rev_catalog.as_ref(), &rev_table, &revision).await?;

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
    let (rev_catalog, rev_table): (Arc<MemoryCatalog>, iceberg::table::Table) =
        iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    append_revision_row_to_table(rev_catalog.as_ref(), &rev_table, &revision).await?;

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
        row.deleted = true;
        row.deleted_at = Some(now_ts());
        row.updated_at = now_ts();
        write_note_row(op, ws_path, &class_name, note_id, &row).await?;
        return Ok(());
    }

    row.deleted = true;
    row.deleted_at = Some(now_ts());
    row.updated_at = now_ts();
    write_note_row(op, ws_path, &class_name, note_id, &row).await?;
    Ok(())
}

pub async fn get_note_history(op: &Operator, ws_path: &str, note_id: &str) -> Result<Value> {
    let class_name = find_note_class(op, ws_path, note_id)
        .await?
        .ok_or_else(|| anyhow!("Note not found: {}", note_id))?;
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches)?;

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
    let (_, table) = iceberg_store::load_revisions_table(op, ws_path, &class_name).await?;
    let batches = scan_table_batches(&table).await?;
    let rows = revision_rows_from_batches(&batches)?;
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
    let revisions = revision_rows_from_batches(&batches)?;
    let revision = revisions
        .into_iter()
        .find(|rev| rev.note_id == note_id && rev.revision_id == revision_id)
        .ok_or_else(|| anyhow!("Revision {} not found for note {}", revision_id, note_id))?;

    let mut row = read_note_row(op, ws_path, &class_name, note_id).await?;
    let new_rev_id = Uuid::new_v4().to_string();
    let timestamp = now_ts();

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
    append_revision_row_to_table(rev_catalog.as_ref(), &rev_table, &restore_revision).await?;

    Ok(serde_json::json!({
        "revision_id": new_rev_id,
        "restored_from": revision_id,
        "timestamp": timestamp,
    }))
}
