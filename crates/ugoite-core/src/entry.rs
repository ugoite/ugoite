//! Single structured Entry application boundary for 0.1.x.
//!
//! Canonical path:
//!
//! - Markdown compatibility input -> [`legacy_markdown_to_draft`]
//! - Structured payload -> [`structured_fields_to_draft`]
//! - Draft -> [`normalize_and_validate_draft`] -> existing 0.1 persistence
//!
//! Markdown is a compatibility adapter. The [`StructuredEntryDraft`] is the
//! only application input shape; storage owns no conversion rules.
//! `EntryRevision::validate_payload` remains the final authority for typed
//! values. Space format/version, storage encoding, and revision/history
//! semantics are unchanged.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ugoite_domain::entry::FieldValue;
use ugoite_domain::form::{FieldType, FormDefinition, ListItemDefinition};
use ugoite_domain::id::FieldId;

use crate::error::{AppError, ErrorCode};

/// Unresolved structured Entry state.
///
/// `fields` holds every field-like input keyed by field name. Markdown
/// sections arrive as strings; structured payloads arrive as typed JSON.
/// `extra_attributes` holds explicitly structured extras. Unknown field names
/// found in `fields` are treated as extra-attribute candidates during
/// normalization.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StructuredEntryDraft {
    pub title: String,
    pub form_name: Option<String>,
    pub tags: Vec<String>,
    pub fields: BTreeMap<String, Value>,
    pub extra_attributes: BTreeMap<String, Value>,
}

/// Validated structured Entry state ready for existing 0.1 persistence.
#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedStructuredEntry {
    pub title: String,
    pub tags: Vec<String>,
    pub values: BTreeMap<FieldId, FieldValue>,
    pub extra_attributes: BTreeMap<String, Value>,
}

/// Machine-readable validation warning, matching the existing 0.1 payload
/// contract consumed by CLI and frontend renderers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationWarning {
    pub code: String,
    pub field: String,
    pub expected_type: String,
    pub expected_format: String,
    pub reason: String,
    pub message: String,
}

/// Build a draft from a structured payload.
pub fn structured_fields_to_draft(
    title: impl Into<String>,
    form_name: Option<impl Into<String>>,
    tags: Vec<String>,
    fields: BTreeMap<String, Value>,
    extra_attributes: BTreeMap<String, Value>,
) -> StructuredEntryDraft {
    StructuredEntryDraft {
        title: title.into(),
        form_name: form_name.map(Into::into),
        tags,
        fields,
        extra_attributes,
    }
}

/// Parse legacy Markdown into the shared draft shape.
///
/// Frontmatter supplies `form` and `tags`; the first `# ` line supplies the
/// title; every `## ` section supplies one raw string field. Frontmatter keys
/// other than `form`/`tags` are kept as field candidates (sections win),
/// matching the long-standing `extract_properties` behavior where frontmatter
/// flows into properties.
pub fn legacy_markdown_to_draft(markdown: &str, fallback_title: &str) -> StructuredEntryDraft {
    let (frontmatter, sections) = parse_markdown(markdown);
    let title = extract_title(markdown, fallback_title);
    let form_name = extract_form(&frontmatter);
    let tags = extract_tags(&frontmatter);
    let mut fields = BTreeMap::new();
    if let Some(map) = frontmatter.as_object() {
        for (key, value) in map {
            if key == "form" || key == "tags" {
                continue;
            }
            fields.insert(key.clone(), value.clone());
        }
    }
    if let Some(map) = sections.as_object() {
        for (key, value) in map {
            // Sections are raw strings by construction; keep them verbatim so
            // normalization sees exactly what Markdown authors wrote.
            let raw = value.as_str().unwrap_or_default().to_string();
            fields.insert(key.clone(), Value::String(raw));
        }
    }
    StructuredEntryDraft {
        title,
        form_name,
        tags,
        fields,
        extra_attributes: BTreeMap::new(),
    }
}

/// Whether legacy Markdown frontmatter explicitly carries `tags`.
///
/// Updates keep stored tags when `tags` is absent; an explicit `tags: []`
/// clears them. This preserves the long-standing compatibility rule without
/// letting storage parse Markdown itself.
pub fn markdown_frontmatter_has_tags(markdown: &str) -> bool {
    let (frontmatter, _) = parse_markdown(markdown);
    frontmatter.get("tags").is_some()
}

/// Validate and normalize one draft against its Form.
///
/// This owns every semantic coercion: Markdown list syntax, boolean aliases,
/// numeric strings, date/time/timestamp normalization, UUID case, binary
/// encoding, list item coercion, asset-reference JSON strings, and
/// null markers. Failures use the existing `AppError` taxonomy
/// (`UnknownFormFields` / `FormValidationFailed`) so persistence adapters do
/// not need their own error shapes.
pub fn normalize_and_validate_draft(
    form: &FormDefinition,
    draft: &StructuredEntryDraft,
) -> Result<NormalizedStructuredEntry, AppError> {
    if let Some(name) = draft.form_name.as_deref() {
        if name != form.name {
            return Err(AppError::invalid_input(
                ErrorCode::InvalidInput,
                format!(
                    "Entry form '{}' does not match expected form '{}'",
                    name, form.name
                ),
            ));
        }
    }

    let mut warnings: Vec<ValidationWarning> = Vec::new();
    let mut values: BTreeMap<FieldId, FieldValue> = BTreeMap::new();
    let by_name: BTreeMap<&str, _> = form
        .fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect();

    for field in &form.fields {
        let raw = draft.fields.get(&field.name);
        let Some(raw) = raw else {
            if field.required && !field.deprecated {
                let (expected_type, expected_format) =
                    expected_field_contract(field.field_type.as_str());
                warnings.push(ValidationWarning {
                    code: "missing_field".to_string(),
                    field: field.name.clone(),
                    expected_type: expected_type.to_string(),
                    expected_format: expected_format.to_string(),
                    reason: "required value is missing".to_string(),
                    message: format!(
                        "Missing required field: {} (expected {})",
                        field.name, expected_format
                    ),
                });
            }
            continue;
        };
        // An explicitly empty string never satisfies a required field. This
        // matches the long-standing Markdown compatibility behavior where an
        // empty `## Section` means "not provided".
        if field.required && !field.deprecated && raw.as_str().is_some_and(|text| text.is_empty()) {
            let (expected_type, expected_format) =
                expected_field_contract(field.field_type.as_str());
            warnings.push(ValidationWarning {
                code: "missing_field".to_string(),
                field: field.name.clone(),
                expected_type: expected_type.to_string(),
                expected_format: expected_format.to_string(),
                reason: "required value is missing".to_string(),
                message: format!(
                    "Missing required field: {} (expected {})",
                    field.name, expected_format
                ),
            });
            continue;
        }
        match coerce_value(raw, &field.field_type, field.list_item.as_ref()) {
            Ok(FieldValue::Null) if field.required && !field.deprecated => {
                let (expected_type, expected_format) =
                    expected_field_contract(field.field_type.as_str());
                // Null never satisfies required, except for an empty list
                // check handled by the domain validator. Keep the warning
                // here so both transports agree before reaching storage.
                warnings.push(ValidationWarning {
                    code: "missing_field".to_string(),
                    field: field.name.clone(),
                    expected_type: expected_type.to_string(),
                    expected_format: expected_format.to_string(),
                    reason: "required value is missing".to_string(),
                    message: format!(
                        "Missing required field: {} (expected {})",
                        field.name, expected_format
                    ),
                });
            }
            Ok(value) => {
                // A required empty list is missing per domain semantics.
                if field.required
                    && !field.deprecated
                    && matches!(&value, FieldValue::List(items) if items.is_empty())
                {
                    let (expected_type, expected_format) =
                        expected_field_contract(field.field_type.as_str());
                    warnings.push(ValidationWarning {
                        code: "missing_field".to_string(),
                        field: field.name.clone(),
                        expected_type: expected_type.to_string(),
                        expected_format: expected_format.to_string(),
                        reason: "required value is missing".to_string(),
                        message: format!(
                            "Missing required field: {} (expected {})",
                            field.name, expected_format
                        ),
                    });
                    continue;
                }
                values.insert(field.id, value);
            }
            Err(reason) => {
                let (expected_type, expected_format) =
                    expected_field_contract(field.field_type.as_str());
                warnings.push(ValidationWarning {
                    code: "invalid_type".to_string(),
                    field: field.name.clone(),
                    expected_type: expected_type.to_string(),
                    expected_format: expected_format.to_string(),
                    reason: format!(
                        "value does not match the {expected_format} contract: {reason}"
                    ),
                    message: format!(
                        "Field '{}' has invalid type; expected {}",
                        field.name, expected_format
                    ),
                });
            }
        }
    }

    // Unknown field names are extra-attribute candidates.
    let mut extras: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in &draft.fields {
        if !by_name.contains_key(key.as_str()) {
            extras.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in &draft.extra_attributes {
        extras.insert(key.clone(), value.clone());
    }
    // Explicit extras that duplicate a real field name are a caller bug, not
    // an unknown field. Prefer the typed field value.
    for field in &form.fields {
        extras.remove(&field.name);
    }

    if !extras.is_empty() && !form.allow_extra_attributes {
        let mut names: Vec<String> = extras.keys().cloned().collect();
        names.sort();
        return Err(AppError::invalid_input_with_detail(
            ErrorCode::UnknownFormFields,
            "Entry contains unknown form fields",
            serde_json::json!({"fields": names}),
        ));
    }

    if !warnings.is_empty() {
        let payload = serde_json::to_value(&warnings).unwrap_or(Value::Array(Vec::new()));
        return Err(AppError::invalid_input_with_detail(
            ErrorCode::FormValidationFailed,
            "Entry form validation failed",
            serde_json::json!({"warnings": payload}),
        ));
    }

    Ok(NormalizedStructuredEntry {
        title: draft.title.clone(),
        tags: draft.tags.clone(),
        values,
        extra_attributes: if form.allow_extra_attributes {
            extras
        } else {
            BTreeMap::new()
        },
    })
}

/// Render one draft back to the existing 0.1 Markdown representation.
///
/// Field order follows the Form definition; unknown extras render last in
/// sorted order. This is the compatibility encoding used by persistence until
/// a structured-only representation ships (out of scope for 0.1.x).
pub fn draft_to_legacy_representation(
    form: &FormDefinition,
    draft: &StructuredEntryDraft,
) -> String {
    let field_order: Vec<String> = form.fields.iter().map(|field| field.name.clone()).collect();
    let merged = merge_maps(&draft.fields, &draft.extra_attributes);
    render_markdown(
        &draft.title,
        draft.form_name.as_deref().unwrap_or(form.name.as_str()),
        &draft.tags,
        &merged,
        &field_order,
    )
}

/// Render one normalized entry back to the existing 0.1 representation.
pub fn normalized_to_legacy_representation(
    form: &FormDefinition,
    form_name: &str,
    normalized: &NormalizedStructuredEntry,
) -> String {
    let mut fields = Map::new();
    for field in &form.fields {
        if let Some(value) = normalized.values.get(&field.id) {
            if let Ok(json) = serde_json::to_value(value) {
                fields.insert(field.name.clone(), json);
            }
        }
    }
    let merged = merge_entry_fields(
        &Value::Object(fields),
        &Value::Object(
            normalized
                .extra_attributes
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        ),
    );
    let field_order: Vec<String> = form.fields.iter().map(|field| field.name.clone()).collect();
    render_markdown(
        &normalized.title,
        form_name,
        &normalized.tags,
        &merged,
        &field_order,
    )
}

// ---------------------------------------------------------------------------
// Markdown compatibility adapter (semantic subset owned by core)
// ---------------------------------------------------------------------------

fn extract_title(content: &str, fallback: &str) -> String {
    for line in content.lines() {
        if let Some(stripped) = line.strip_prefix("# ") {
            return stripped.trim().to_string();
        }
    }
    fallback.to_string()
}

fn extract_frontmatter(content: &str) -> (Value, String) {
    let pattern =
        regex::Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").expect("valid frontmatter regex");
    if let Some(caps) = pattern.captures(content) {
        let yaml_str = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let fm_yaml: Option<serde_yaml::Value> = serde_yaml::from_str(yaml_str).ok();
        let fm_json = fm_yaml
            .and_then(|value| serde_json::to_value(value).ok())
            .unwrap_or_else(|| Value::Object(Map::new()));
        let end = caps.get(0).map(|m| m.end()).unwrap_or(0);
        return (fm_json, content[end..].to_string());
    }
    (Value::Object(Map::new()), content.to_string())
}

fn extract_sections(body: &str) -> Value {
    let mut sections = Map::new();
    let header = regex::Regex::new(r"^##\s+(.+)$").expect("valid section header regex");
    let mut current_key: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();
    for line in body.lines() {
        if let Some(caps) = header.captures(line) {
            if let Some(key) = current_key.take() {
                sections.insert(key, Value::String(buffer.join("\n").trim().to_string()));
            }
            current_key = Some(
                caps.get(1)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default(),
            );
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
            .filter_map(|value| value.as_str().map(str::to_string))
            .collect(),
        Some(Value::String(tag)) => vec![tag.clone()],
        _ => Vec::new(),
    }
}

fn extract_form(frontmatter: &Value) -> Option<String> {
    frontmatter
        .get("form")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn merge_entry_fields(fields: &Value, extra_attributes: &Value) -> Value {
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

fn merge_maps(fields: &BTreeMap<String, Value>, extra: &BTreeMap<String, Value>) -> Value {
    let mut merged = Map::new();
    for (key, value) in fields {
        merged.insert(key.clone(), value.clone());
    }
    for (key, value) in extra {
        merged.insert(key.clone(), value.clone());
    }
    Value::Object(merged)
}

fn section_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
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
                        Value::String(text) => format!("- {text}"),
                        Value::Number(number) => format!("- {number}"),
                        Value::Bool(flag) => format!("- {flag}"),
                        _ => "-".to_string(),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
        Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn render_frontmatter(form_name: &str, tags: &[String]) -> String {
    let mut frontmatter = String::from("---\n");
    frontmatter.push_str(&format!("form: {form_name}\n"));
    if !tags.is_empty() {
        frontmatter.push_str("tags:\n");
        for tag in tags {
            frontmatter.push_str(&format!("  - {tag}\n"));
        }
    }
    frontmatter.push_str("---\n");
    frontmatter
}

fn render_markdown(
    title: &str,
    form_name: &str,
    tags: &[String],
    fields: &Value,
    field_order: &[String],
) -> String {
    let mut markdown = String::new();
    markdown.push_str(&render_frontmatter(form_name, tags));
    markdown.push_str(&format!("# {title}\n\n"));

    let mut ordered = Vec::new();
    if let Some(map) = fields.as_object() {
        let mut seen = HashSet::new();
        for name in field_order {
            if let Some(value) = map.get(name) {
                ordered.push((name.clone(), value.clone()));
                seen.insert(name.clone());
            }
        }
        let mut remaining = Vec::new();
        for (name, value) in map {
            if !seen.contains(name) {
                remaining.push((name.clone(), value.clone()));
            }
        }
        remaining.sort_by(|left, right| left.0.cmp(&right.0));
        ordered.extend(remaining);
    }

    for (name, value) in ordered {
        markdown.push_str(&format!("## {name}\n"));
        let rendered = section_value_to_string(&value);
        if !rendered.is_empty() {
            markdown.push_str(&rendered);
            markdown.push('\n');
        }
        markdown.push('\n');
    }

    markdown.trim_end().to_string()
}

// ---------------------------------------------------------------------------
// Shared coercion/normalization (single authority)
// ---------------------------------------------------------------------------

fn expected_field_contract(field_type: &str) -> (&'static str, &'static str) {
    match field_type {
        "number" | "double" | "float" => ("number", "a numeric value"),
        "integer" => ("integer", "a 32-bit integer"),
        "long" => ("long", "an integer"),
        "date" => ("date", "ISO date YYYY-MM-DD"),
        "time" => ("time", "ISO time HH:MM[:SS[.fraction]]"),
        "timestamp" => ("timestamp", "timezone-free ISO timestamp"),
        "timestamp_tz" => ("timestamp_tz", "RFC3339 timestamp with timezone"),
        "timestamp_ns" => ("timestamp_ns", "nanosecond timezone-free ISO timestamp"),
        "timestamp_tz_ns" => (
            "timestamp_tz_ns",
            "nanosecond RFC3339 timestamp with timezone",
        ),
        "uuid" => ("uuid", "UUID"),
        "binary" => ("binary", "base64: or hex: encoded bytes"),
        "boolean" => ("boolean", "true, false, yes, no, on, off, 1, or 0"),
        "list" => ("list", "a JSON array or Markdown list"),
        "object_list" => ("object_list", "a list of JSON objects"),
        "asset_reference" => ("asset_reference", "an asset reference object"),
        "row_reference" => ("row_reference", "an Entry ID"),
        "markdown" => ("markdown", "Markdown text"),
        "sql" => ("sql", "SQL text"),
        _ => ("string", "text"),
    }
}

fn parse_boolean_text(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

fn parse_wall_timestamp(value: &str) -> Option<chrono::NaiveDateTime> {
    ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M"]
        .into_iter()
        .find_map(|format| chrono::NaiveDateTime::parse_from_str(value, format).ok())
}

fn parse_zoned_timestamp(value: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(value).ok()
}

fn format_wall_timestamp(timestamp: chrono::NaiveDateTime, nanosecond_precision: bool) -> String {
    use chrono::Timelike;
    let base = timestamp.format("%Y-%m-%dT%H:%M:%S").to_string();
    let nanos = if nanosecond_precision {
        timestamp.nanosecond()
    } else {
        (timestamp.nanosecond() / 1_000) * 1_000
    };
    if nanos == 0 {
        return base;
    }
    let fraction = format!("{nanos:09}").trim_end_matches('0').to_string();
    format!("{base}.{fraction}")
}

fn normalize_wall_timestamp(value: &str, nanosecond_precision: bool) -> Option<String> {
    parse_wall_timestamp(value.trim())
        .map(|timestamp| format_wall_timestamp(timestamp, nanosecond_precision))
}

fn normalize_zoned_timestamp(value: &str, nanosecond_precision: bool) -> Option<String> {
    parse_zoned_timestamp(value.trim()).map(|timestamp| {
        let timestamp = timestamp.with_timezone(&chrono::Utc);
        if nanosecond_precision {
            timestamp.to_rfc3339_opts(chrono::SecondsFormat::Nanos, false)
        } else {
            // Match the existing 0.1 path (`to_rfc3339`, i.e. AutoSi):
            // preserve fractional seconds when present, do not truncate.
            timestamp.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, false)
        }
    })
}

fn normalize_time(value: &str) -> Option<String> {
    let trimmed = value.trim();
    for format in ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"] {
        if let Ok(time) = chrono::NaiveTime::parse_from_str(trimmed, format) {
            use chrono::Timelike;
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
    use base64::Engine;
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
    // Markdown list syntax only, matching the existing 0.1 compatibility
    // path. JSON arrays arrive typed; JSON strings are handled per field type
    // in `coerce_value` (asset-reference lists accept an encoded array).
    let mut items = Vec::new();
    for line in value.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let item = if let Some(rest) = line.strip_prefix("- [ ] ") {
            rest
        } else if let Some(rest) = line.strip_prefix("- [x] ") {
            rest
        } else if let Some(rest) = line.strip_prefix("- [X] ") {
            rest
        } else if let Some(rest) = line.strip_prefix("- ") {
            rest
        } else if let Some(rest) = line.strip_prefix("* ") {
            rest
        } else if let Some(rest) = line.strip_prefix("+ ") {
            rest
        } else {
            line
        };
        if !item.is_empty() {
            items.push(Value::String(item.to_string()));
        }
    }
    items
}

fn json_to_untyped_field_value(value: &Value) -> Result<FieldValue, String> {
    match value {
        Value::Null => Ok(FieldValue::Null),
        Value::Bool(flag) => Ok(FieldValue::Boolean(*flag)),
        Value::String(text) => Ok(FieldValue::String(text.clone())),
        Value::Number(number) => number
            .as_f64()
            .filter(|value| value.is_finite())
            .map(FieldValue::Number)
            .ok_or_else(|| "invalid number".to_string()),
        Value::Array(values) => values
            .iter()
            .map(json_to_untyped_field_value)
            .collect::<Result<Vec<_>, _>>()
            .map(FieldValue::List),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| Ok((key.clone(), json_to_untyped_field_value(value)?)))
            .collect::<Result<BTreeMap<_, _>, String>>()
            .map(FieldValue::Object),
    }
}

/// Convert one transport JSON value to its canonical domain value.
///
/// Markdown sections arrive as strings; structured payloads arrive typed.
/// Both use this exact function so legacy and structured creates agree.
fn coerce_value(
    value: &Value,
    field_type: &FieldType,
    list_item: Option<&ListItemDefinition>,
) -> Result<FieldValue, String> {
    if value.is_null() {
        return Ok(FieldValue::Null);
    }
    // Explicit null transport markers stay null for typed fields while the
    // literal string "null" remains valid text for string-like fields.
    if !matches!(
        field_type,
        FieldType::String | FieldType::Markdown | FieldType::Sql
    ) && value
        .as_str()
        .is_some_and(|text| matches!(text.trim(), "null" | "~"))
    {
        return Ok(FieldValue::Null);
    }
    match field_type {
        FieldType::String | FieldType::Markdown | FieldType::Sql | FieldType::RowReference => value
            .as_str()
            .map(|text| FieldValue::String(text.to_string()))
            .ok_or_else(|| "typed string field must be a string".to_string()),
        FieldType::Boolean => {
            if let Some(flag) = value.as_bool() {
                return Ok(FieldValue::Boolean(flag));
            }
            if let Some(number) = value.as_i64() {
                return match number {
                    1 => Ok(FieldValue::Boolean(true)),
                    0 => Ok(FieldValue::Boolean(false)),
                    _ => Err("boolean field must be a boolean".to_string()),
                };
            }
            value
                .as_str()
                .and_then(parse_boolean_text)
                .map(FieldValue::Boolean)
                .ok_or_else(|| "boolean field must be a boolean".to_string())
        }
        FieldType::Integer => {
            let raw = value
                .as_i64()
                .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
                .ok_or_else(|| "integer field must be an integer".to_string())?;
            i32::try_from(raw)
                .map(|narrow| FieldValue::Integer(i64::from(narrow)))
                .map_err(|_| "integer field is outside the Int32 range".to_string())
        }
        FieldType::Long => value
            .as_i64()
            .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
            .map(FieldValue::Integer)
            .ok_or_else(|| "long field must be an integer".to_string()),
        FieldType::Float | FieldType::Double => {
            let parsed = value
                .as_f64()
                .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
                .ok_or_else(|| "floating field must be a number".to_string())?;
            if !parsed.is_finite() {
                return Err("floating field must be finite".to_string());
            }
            Ok(FieldValue::Number(parsed))
        }
        FieldType::Date => {
            let text = value
                .as_str()
                .ok_or_else(|| "date field must be a string".to_string())?;
            let date = chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
                .map_err(|_| "invalid date field".to_string())?;
            Ok(FieldValue::String(date.format("%Y-%m-%d").to_string()))
        }
        FieldType::Time => {
            let text = value
                .as_str()
                .ok_or_else(|| "time field must be a string".to_string())?;
            normalize_time(text)
                .map(FieldValue::String)
                .ok_or_else(|| "invalid time field".to_string())
        }
        FieldType::Timestamp => {
            let text = value
                .as_str()
                .ok_or_else(|| "timestamp field must be a string".to_string())?;
            // Reject RFC3339 offsets: wall timestamps are timezone-free.
            if parse_zoned_timestamp(text.trim()).is_some()
                && normalize_wall_timestamp(text, false).is_none()
            {
                return Err("invalid timestamp field".to_string());
            }
            normalize_wall_timestamp(text, false)
                .map(FieldValue::String)
                .ok_or_else(|| "invalid timestamp field".to_string())
        }
        FieldType::TimestampNs => {
            let text = value
                .as_str()
                .ok_or_else(|| "timestamp_ns field must be a string".to_string())?;
            if parse_zoned_timestamp(text.trim()).is_some()
                && normalize_wall_timestamp(text, true).is_none()
            {
                return Err("invalid timestamp_ns field".to_string());
            }
            normalize_wall_timestamp(text, true)
                .map(FieldValue::String)
                .ok_or_else(|| "invalid timestamp_ns field".to_string())
        }
        FieldType::TimestampTz => {
            let text = value
                .as_str()
                .ok_or_else(|| "timestamp_tz field must be a string".to_string())?;
            normalize_zoned_timestamp(text, false)
                .map(FieldValue::String)
                .ok_or_else(|| "invalid timestamp_tz field".to_string())
        }
        FieldType::TimestampTzNs => {
            let text = value
                .as_str()
                .ok_or_else(|| "timestamp_tz_ns field must be a string".to_string())?;
            normalize_zoned_timestamp(text, true)
                .map(FieldValue::String)
                .ok_or_else(|| "invalid timestamp_tz_ns field".to_string())
        }
        FieldType::Uuid => {
            let text = value
                .as_str()
                .ok_or_else(|| "UUID field must be a string".to_string())?;
            uuid::Uuid::parse_str(text.trim())
                .map(|id| FieldValue::String(id.to_string()))
                .map_err(|_| "invalid UUID field".to_string())
        }
        FieldType::Binary => {
            let text = value
                .as_str()
                .ok_or_else(|| "binary field must be a string".to_string())?;
            normalize_binary(text)
                .map(FieldValue::String)
                .ok_or_else(|| "invalid binary field".to_string())
        }
        FieldType::AssetReference => {
            let parsed: Value = match value {
                Value::String(raw) => serde_json::from_str(raw.trim())
                    .map_err(|_| "asset reference field must contain a JSON object".to_string())?,
                value => value.clone(),
            };
            serde_json::from_value::<ugoite_domain::entry::AssetReference>(parsed)
                .map(FieldValue::AssetReference)
                .map_err(|_| "invalid asset reference value".to_string())
        }
        FieldType::List => {
            let is_asset_list = list_item
                .as_ref()
                .is_some_and(|item| item.field_type == FieldType::AssetReference);
            let items: Vec<Value> = match value {
                Value::Array(items) => items.clone(),
                Value::String(raw) => {
                    // Match the existing 0.1 path: asset-reference lists
                    // accept an encoded JSON array string, while plain lists
                    // use Markdown list syntax.
                    if is_asset_list {
                        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(raw.trim()) {
                            items
                        } else {
                            parse_markdown_list(raw)
                        }
                    } else {
                        parse_markdown_list(raw)
                    }
                }
                _ => return Err("typed list field must be an array".to_string()),
            };
            let item_type = list_item
                .map(|item| &item.field_type)
                .unwrap_or(&FieldType::String);
            items
                .iter()
                .map(|item| coerce_value(item, item_type, None))
                .collect::<Result<Vec<_>, _>>()
                .map(FieldValue::List)
        }
        FieldType::ObjectList => {
            let items: Vec<Value> = match value {
                Value::Array(items) => items.clone(),
                Value::String(raw) => serde_json::from_str::<Value>(raw.trim())
                    .ok()
                    .and_then(|parsed| parsed.as_array().cloned())
                    .ok_or_else(|| "object list field must be an array".to_string())?,
                _ => return Err("object list field must be an array".to_string()),
            };
            items
                .iter()
                .map(|item| {
                    let converted = json_to_untyped_field_value(item)?;
                    match converted {
                        FieldValue::Object(_) => Ok(converted),
                        _ => Err("object list items must be objects".to_string()),
                    }
                })
                .collect::<Result<Vec<_>, _>>()
                .map(FieldValue::List)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_domain::form::{FormField, FormVersion};
    use ugoite_domain::id::{FieldId, FormId};

    fn form_id(seed: u128) -> FormId {
        FormId::from(uuid::Uuid::from_u128(seed))
    }

    fn field(id: i32, name: &str, field_type: FieldType) -> FormField {
        FormField {
            id: FieldId::new(id).expect("test field id"),
            name: name.to_string(),
            field_type,
            required: false,
            label: None,
            description: None,
            semantic_role: None,
            reference_form: None,
            list_item: None,
            validation: None,
            enum_values: Vec::new(),
            deprecated: false,
        }
    }

    fn required_field(id: i32, name: &str, field_type: FieldType) -> FormField {
        let mut field = field(id, name, field_type);
        field.required = true;
        field
    }

    fn test_form() -> FormDefinition {
        FormDefinition {
            id: form_id(0xA1),
            version: FormVersion::new(1).expect("version"),
            name: "Note".to_string(),
            description: None,
            fields: vec![
                required_field(100, "Body", FieldType::String),
                field(101, "Done", FieldType::Boolean),
                field(102, "Count", FieldType::Integer),
            ],
            allow_extra_attributes: false,
            extension_metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn legacy_and_structured_drafts_normalize_to_the_same_values() {
        let form = test_form();
        let markdown =
            "---\nform: Note\n---\n# Title\n\n## Body\n\nhello\n\n## Done\nyes\n\n## Count\n42\n";
        let legacy = legacy_markdown_to_draft(markdown, "fallback");
        assert_eq!(legacy.title, "Title");
        assert_eq!(legacy.form_name.as_deref(), Some("Note"));

        let mut fields = BTreeMap::new();
        fields.insert("Body".to_string(), Value::String("hello".to_string()));
        fields.insert("Done".to_string(), Value::Bool(true));
        fields.insert("Count".to_string(), Value::Number(42.into()));
        let structured =
            structured_fields_to_draft("Title", Some("Note"), Vec::new(), fields, BTreeMap::new());

        let from_legacy = normalize_and_validate_draft(&form, &legacy).expect("legacy valid");
        let from_structured =
            normalize_and_validate_draft(&form, &structured).expect("structured valid");
        assert_eq!(from_legacy.values, from_structured.values);
        assert_eq!(
            from_legacy.values.get(&FieldId::new(101).expect("id")),
            Some(&FieldValue::Boolean(true))
        );
    }

    #[test]
    fn markdown_render_round_trips_through_the_draft_boundary() {
        let form = test_form();
        let markdown = "---\nform: Note\n---\n# Title\n\n## Body\n\nhello\n\n## Done\ntrue\n";
        let draft = legacy_markdown_to_draft(markdown, "fallback");
        let normalized = normalize_and_validate_draft(&form, &draft).expect("valid");
        let rendered = normalized_to_legacy_representation(&form, "Note", &normalized);
        let reparsed = legacy_markdown_to_draft(&rendered, "fallback");
        let renormalized = normalize_and_validate_draft(&form, &reparsed).expect("reparsed valid");
        assert_eq!(normalized, renormalized);
    }

    #[test]
    fn unknown_fields_follow_the_extra_attributes_policy() {
        let mut form = test_form();
        let mut fields = BTreeMap::new();
        fields.insert("Body".to_string(), Value::String("hello".to_string()));
        fields.insert("Scratch".to_string(), Value::String("keep".to_string()));
        let draft =
            structured_fields_to_draft("T", Some("Note"), Vec::new(), fields, BTreeMap::new());

        let error = normalize_and_validate_draft(&form, &draft).expect_err("deny extras");
        assert_eq!(error.code(), ErrorCode::UnknownFormFields);

        form.allow_extra_attributes = true;
        let normalized = normalize_and_validate_draft(&form, &draft).expect("allow extras");
        assert_eq!(
            normalized.extra_attributes.get("Scratch"),
            Some(&Value::String("keep".to_string()))
        );
    }

    #[test]
    fn required_fields_fail_on_both_paths() {
        let form = test_form();
        let markdown = "---\nform: Note\n---\n# T\n\n## Done\ntrue\n";
        let legacy = legacy_markdown_to_draft(markdown, "T");
        let error = normalize_and_validate_draft(&form, &legacy).expect_err("missing Body");
        assert_eq!(error.code(), ErrorCode::FormValidationFailed);

        let draft = structured_fields_to_draft(
            "T",
            Some("Note"),
            Vec::new(),
            BTreeMap::from([("Done".to_string(), Value::Bool(true))]),
            BTreeMap::new(),
        );
        let error = normalize_and_validate_draft(&form, &draft).expect_err("missing Body");
        assert_eq!(error.code(), ErrorCode::FormValidationFailed);
    }

    #[test]
    fn boolean_aliases_and_markdown_lists_share_one_coercion() {
        let form = FormDefinition {
            id: form_id(0xB0),
            version: FormVersion::new(1).expect("version"),
            name: "List".to_string(),
            description: None,
            fields: vec![
                field(100, "Done", FieldType::Boolean),
                FormField {
                    list_item: None,
                    ..field(101, "Items", FieldType::List)
                },
            ],
            allow_extra_attributes: false,
            extension_metadata: BTreeMap::new(),
        };
        let markdown = "---\nform: List\n---\n# T\n\n## Done\nON\n\n## Items\n- a\n* b\n";
        let draft = legacy_markdown_to_draft(markdown, "T");
        let normalized = normalize_and_validate_draft(&form, &draft).expect("aliases valid");
        assert_eq!(
            normalized.values.get(&FieldId::new(100).expect("id")),
            Some(&FieldValue::Boolean(true))
        );
        assert_eq!(
            normalized.values.get(&FieldId::new(101).expect("id")),
            Some(&FieldValue::List(vec![
                FieldValue::String("a".to_string()),
                FieldValue::String("b".to_string()),
            ]))
        );
    }

    #[test]
    fn draft_render_uses_form_order_then_sorted_extras() {
        let mut form = test_form();
        form.allow_extra_attributes = true;
        let mut fields = BTreeMap::new();
        fields.insert("Count".to_string(), Value::Number(1.into()));
        fields.insert("Body".to_string(), Value::String("b".to_string()));
        let mut extra = BTreeMap::new();
        extra.insert("Zeta".to_string(), Value::String("z".to_string()));
        extra.insert("Alpha".to_string(), Value::String("a".to_string()));
        let draft = structured_fields_to_draft("T", Some("Note"), Vec::new(), fields, extra);
        let rendered = draft_to_legacy_representation(&form, &draft);
        let body_pos = rendered.find("## Body").expect("body");
        let count_pos = rendered.find("## Count").expect("count");
        let alpha_pos = rendered.find("## Alpha").expect("alpha");
        let zeta_pos = rendered.find("## Zeta").expect("zeta");
        assert!(body_pos < count_pos && count_pos < alpha_pos && alpha_pos < zeta_pos);
    }

    #[test]
    fn structured_compat_fixtures_agree_across_both_inputs() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join("../../fixtures/entry/structured-compat");
        for name in [
            "01-core-scalars.json",
            "02-temporal.json",
            "03-collections.json",
            "04-compat-rules.json",
            "05-required.json",
            "09-existing-space-reopen.json",
        ] {
            assert!(dir.join(name).is_file(), "missing fixture {name}");
        }

        // Spot-check the scalar corpus end to end: Markdown and structured
        // inputs must normalize to identical typed values.
        let raw = std::fs::read_to_string(dir.join("01-core-scalars.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let form: FormDefinition = serde_json::from_value(fixture["form"].clone()).expect("form");
        let markdown = fixture["markdown"].as_str().expect("markdown");
        let legacy = legacy_markdown_to_draft(markdown, "fallback");
        let normalized = normalize_and_validate_draft(&form, &legacy).expect("legacy valid");
        let structured = fixture["structured"].clone();
        let mut fields = BTreeMap::new();
        for (key, value) in structured["fields"]
            .as_object()
            .cloned()
            .unwrap_or_default()
        {
            fields.insert(key, value);
        }
        let structured_draft = structured_fields_to_draft(
            structured["title"].as_str().unwrap_or_default(),
            structured["form"].as_str().map(str::to_string),
            structured["tags"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect(),
            fields,
            BTreeMap::new(),
        );
        let from_structured =
            normalize_and_validate_draft(&form, &structured_draft).expect("structured valid");
        assert_eq!(normalized.values, from_structured.values, "scalar parity");
        // Existing 0.1 representation round-trips through the same boundary.
        let rendered = normalized_to_legacy_representation(&form, &form.name, &normalized);
        let reparsed = legacy_markdown_to_draft(&rendered, "fallback");
        let renormalized =
            normalize_and_validate_draft(&form, &reparsed).expect("round-trip valid");
        assert_eq!(normalized.values, renormalized.values, "scalar round-trip");

        // Temporal corpus pins normalization (uuid case, binary prefix,
        // timezone canonicalization).
        let raw = std::fs::read_to_string(dir.join("02-temporal.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let form: FormDefinition = serde_json::from_value(fixture["form"].clone()).expect("form");
        let markdown = fixture["markdown"].as_str().expect("markdown");
        let draft = legacy_markdown_to_draft(markdown, "fallback");
        let normalized = normalize_and_validate_draft(&form, &draft).expect("temporal valid");
        let expected = &fixture["expected"]["values"];
        for (id, value) in expected.as_object().cloned().unwrap_or_default() {
            let id = FieldId::new(id.parse().expect("field id")).expect("id");
            let expected_value: FieldValue = serde_json::from_value(value).expect("field value");
            assert_eq!(
                normalized.values.get(&id),
                Some(&expected_value),
                "temporal {id:?}"
            );
        }

        // Compat rules: unknown fields are extras when allowed, errors when denied.
        let raw = std::fs::read_to_string(dir.join("04-compat-rules.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let allow: FormDefinition =
            serde_json::from_value(fixture["form_allow"].clone()).expect("form");
        let deny: FormDefinition =
            serde_json::from_value(fixture["form_deny"].clone()).expect("form");
        let unknown_markdown =
            "---\nform: CompatAllow\n---\n# T\n\n## Title\nhello\n\n## Scratch\nkeep me\n";
        let draft = legacy_markdown_to_draft(unknown_markdown, "T");
        let normalized = normalize_and_validate_draft(&allow, &draft).expect("allowed extra");
        assert_eq!(
            normalized.extra_attributes.get("Scratch"),
            Some(&Value::String("keep me".to_string()))
        );
        let error = normalize_and_validate_draft(&deny, &draft).expect_err("denied extra");
        assert_eq!(error.code(), ErrorCode::UnknownFormFields);

        // Required corpus fails identically on both paths.
        let raw = std::fs::read_to_string(dir.join("05-required.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let form: FormDefinition = serde_json::from_value(fixture["form"].clone()).expect("form");
        for markdown in fixture["invalid_markdowns"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let draft = legacy_markdown_to_draft(markdown.as_str().unwrap_or_default(), "T");
            assert!(
                normalize_and_validate_draft(&form, &draft).is_err(),
                "invalid markdown must fail"
            );
        }
    }

    #[test]
    fn legacy_collections_and_asset_references_normalize() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join("../../fixtures/entry/structured-compat");
        let raw = std::fs::read_to_string(dir.join("03-collections.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        // Collections form uses a row_reference target; give the test a local
        // FormId for that target without touching storage.
        let mut form_value = fixture["form"].clone();
        form_value["fields"].as_array_mut().expect("fields")[3]["reference_form"] =
            Value::String(form_id(0xA1).to_string());
        let form: FormDefinition = serde_json::from_value(form_value).expect("form");
        let markdown = fixture["markdown"].as_str().expect("markdown");
        let draft = legacy_markdown_to_draft(markdown, "fallback");
        let normalized = normalize_and_validate_draft(&form, &draft).expect("legacy valid");
        assert_eq!(
            normalized.values.get(&FieldId::new(100).expect("id")),
            Some(&FieldValue::List(vec![
                FieldValue::String("alpha".to_string()),
                FieldValue::String("beta".to_string()),
            ]))
        );
        assert_eq!(
            normalized.values.get(&FieldId::new(103).expect("id")),
            Some(&FieldValue::String("task-01".to_string()))
        );

        let structured = &fixture["structured"]["fields"];
        let mut fields = BTreeMap::new();
        for (key, value) in structured.as_object().cloned().unwrap_or_default() {
            fields.insert(key, value);
        }
        let structured_draft = structured_fields_to_draft(
            "Website",
            Some("Project"),
            Vec::new(),
            fields,
            BTreeMap::new(),
        );
        let from_structured =
            normalize_and_validate_draft(&form, &structured_draft).expect("structured valid");
        // Both paths agree on the Markdown-representable subset; structured
        // extras (typed lists/objects/assets) validate on the same boundary.
        assert_eq!(
            from_structured.values.get(&FieldId::new(100).expect("id")),
            Some(&FieldValue::List(vec![
                FieldValue::String("alpha".to_string()),
                FieldValue::String("beta".to_string()),
            ]))
        );
        assert!(matches!(
            from_structured.values.get(&FieldId::new(104).expect("id")),
            Some(FieldValue::AssetReference(_))
        ));
        assert!(matches!(
            from_structured.values.get(&FieldId::new(102).expect("id")),
            Some(FieldValue::List(_))
        ));
    }
}
