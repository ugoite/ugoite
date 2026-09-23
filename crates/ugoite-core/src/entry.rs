//! Single structured Entry application boundary for the current product
//! surface.
//!
//! Canonical path:
//!
//! - Structured payload -> [`structured_fields_to_draft`]
//! - Draft -> [`normalize_and_validate_draft`] -> existing 0.1 persistence
//!
//! The [`StructuredEntryDraft`] is the only product mutation input shape;
//! storage owns no conversion rules. `EntryRevision::validate_payload` remains the final authority for typed
//! values. Space format/version, storage encoding, and revision/history
//! semantics are unchanged.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StructuredEntryDraft {
    #[serde(default)]
    pub form_name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
    #[serde(default)]
    pub extra_attributes: BTreeMap<String, Value>,
}

/// Validated structured Entry state ready for existing 0.1 persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedStructuredEntry {
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
    form_name: Option<impl Into<String>>,
    tags: Vec<String>,
    fields: BTreeMap<String, Value>,
    extra_attributes: BTreeMap<String, Value>,
) -> StructuredEntryDraft {
    StructuredEntryDraft {
        form_name: form_name.map(Into::into),
        tags,
        fields,
        extra_attributes,
    }
}

/// Validate and normalize one draft against its Form.
///
/// This owns every semantic coercion: Markdown list syntax, boolean aliases,
/// numeric strings, date/time/timestamp normalization, UUID case, binary
/// encoding, list item coercion, asset-reference JSON strings, and
/// null markers. Failures use the existing `AppError` taxonomy
/// (`UnknownFormFields` / `FormValidationFailed`) so persistence adapters do
/// not need their own error shapes.
///
/// Empty-section semantics: an absent field is "not provided" (a required
/// field warns `missing_field`); an explicit empty string is also "not
/// provided" for required fields, matching the long-standing Markdown
/// compatibility rule where an empty `## Section` means "not provided".
/// For optional fields the empty value flows through normal coercion and a
/// typed error surfaces when the value cannot coerce.
///
/// Duplicate keys across `fields` and `extra_attributes` are a caller
/// contract violation, never a precedence question: concurrent authors must
/// not silently win, so the shared boundary rejects with an `InvalidInput`
/// diagnostic naming the duplicate keys instead of preferring either side.
/// Transports resolve overlap explicitly before calling (CLI `--field` /
/// `--fields-file` values replace preserved extras; CLI duplicate inputs
/// are a usage error).
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
    // A key supplied in both `fields` and `extra_attributes`, or an explicit
    // extra that shadows a real field name, has no silent precedence:
    // reject deterministically before any coercion runs so concurrent
    // authors never silently win and stale extras are never silently
    // dropped. Callers resolve overlap explicitly before calling.
    let mut duplicates: Vec<String> = draft
        .extra_attributes
        .keys()
        .filter(|key| draft.fields.contains_key(*key) || by_name.contains_key(key.as_str()))
        .cloned()
        .collect();
    duplicates.sort();
    if !duplicates.is_empty() {
        return Err(AppError::invalid_input_with_detail(
            ErrorCode::InvalidInput,
            "Entry fields and extra_attributes must not contain the same key",
            serde_json::json!({"duplicate_fields": duplicates}),
        ));
    }

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
                // An empty object carries no properties, so it is never a
                // meaningful object_list item on the write path. New writes
                // containing one are rejected with the same invalid_type
                // diagnostic as any other contract violation (INVALID_INPUT
                // kind with field guidance). Historical storage is untouched:
                // `stored_fields_to_values` keeps decoding legacy `[{}]`
                // rows so existing spaces remain readable without migration.
                if field.field_type == FieldType::ObjectList
                    && matches!(&value, FieldValue::List(items) if items.iter().any(|item| matches!(item, FieldValue::Object(properties) if properties.is_empty())))
                {
                    let (expected_type, expected_format) =
                        expected_field_contract(field.field_type.as_str());
                    warnings.push(ValidationWarning {
                        code: "invalid_type".to_string(),
                        field: field.name.clone(),
                        expected_type: expected_type.to_string(),
                        expected_format: expected_format.to_string(),
                        reason: format!(
                            "value does not match the {expected_format} contract: object list items must not be empty objects"
                        ),
                        message: format!(
                            "Field '{}' has invalid type; expected {}",
                            field.name, expected_format
                        ),
                    });
                    continue;
                }
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

    // Unknown field names are extra-attribute candidates. Duplicate keys
    // across `fields`/`extra_attributes` and extras shadowing real field
    // names were already rejected above, so no precedence applies here.
    let mut extras: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in &draft.fields {
        if !by_name.contains_key(key.as_str()) {
            extras.insert(key.clone(), value.clone());
        }
    }
    for (key, value) in &draft.extra_attributes {
        extras.insert(key.clone(), value.clone());
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
        tags: draft.tags.clone(),
        values,
        extra_attributes: if form.allow_extra_attributes {
            extras
        } else {
            BTreeMap::new()
        },
    })
}

/// Decode persisted Form fields through the same coercion boundary used by
/// structured drafts.
///
/// This intentionally does not apply required-field or extra-attribute
/// admission. Those checks belong to the revision validator; historical rows
/// only need their typed value map. The conversion itself remains shared so
/// storage cannot grow a second type system. The `INVALID_INPUT` error code
/// and empty-map handling for malformed legacy storage are retained here
/// because this helper is also used by existing read/restore paths.
pub fn stored_fields_to_values(
    fields: &Value,
    form: &FormDefinition,
) -> Result<BTreeMap<FieldId, FieldValue>, AppError> {
    let object = fields.as_object().cloned().unwrap_or_default();
    let mut values = BTreeMap::new();
    for field in &form.fields {
        let Some(value) = object.get(&field.name) else {
            continue;
        };
        let value =
            coerce_value(value, &field.field_type, field.list_item.as_ref()).map_err(|reason| {
                AppError::invalid_input(
                    ErrorCode::InvalidInput,
                    format!("Field '{}': {reason}", field.name),
                )
            })?;
        values.insert(field.id, value);
    }
    Ok(values)
}

/// Preview one structured draft without touching Storage.
///
/// This is the pre-save diagnostics boundary for Frontend and CLI: it calls
/// exactly [`normalize_and_validate_draft`], so the returned normalized draft
/// or field-addressed diagnostics (`code` / `field` / `message` /
/// `expected_type` / `expected_format` / `reason` via [`ValidationWarning`])
/// match the mutation path. It never writes Storage and never creates a
/// revision or Change.
pub fn preview_structured_draft(
    form: &FormDefinition,
    draft: &StructuredEntryDraft,
) -> Result<NormalizedStructuredEntry, AppError> {
    normalize_and_validate_draft(form, draft)
}

/// Extract field-addressed diagnostics from a preview/mutation error.
///
/// Returns the `warnings` payload for `FORM_VALIDATION_FAILED`, or `None`
/// when the error carries no field diagnostics (e.g. unknown-field or
/// form-mismatch errors).
pub fn validation_warnings(error: &AppError) -> Option<Vec<ValidationWarning>> {
    let detail = error.detail()?;
    let warnings = detail.get("warnings")?;
    serde_json::from_value(warnings.clone()).ok()
}

/// Extract unknown field names from an `UNKNOWN_FORM_FIELDS` error.
pub fn unknown_field_names(error: &AppError) -> Option<Vec<String>> {
    let detail = error.detail()?;
    let fields = detail.get("fields")?;
    serde_json::from_value(fields.clone()).ok()
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

pub fn render_markdown(
    form_name: &str,
    tags: &[String],
    fields: &Value,
    field_order: &[String],
) -> String {
    let mut markdown = String::new();
    markdown.push_str(&render_frontmatter(form_name, tags));

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
