//! Single structured Entry application boundary for the current product
//! surface.
//!
//! Canonical path:
//!
//! - Structured payload -> [`structured_fields_to_draft`]
//! - Draft -> [`normalize_and_validate_draft`] -> existing 0.1 persistence
//!
//! The [`StructuredEntryDraft`] is the only product mutation input shape;
//! storage owns no conversion rules. The legacy Markdown decoder below is
//! retained only for test fixtures that exercise reading existing Space 0.1
//! data, not as a user-facing mutation adapter.
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StructuredEntryDraft {
    #[serde(default)]
    pub title: String,
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

/// A loss or ambiguity found while decoding legacy Markdown test fixtures into
/// the canonical structured draft.
///
/// These diagnostics are deliberately separate from Form validation. A
/// Markdown document can be valid Markdown and still contain structure that
/// has no lossless representation in a structured Entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarkdownConversionDiagnostic {
    pub code: String,
    pub message: String,
}

/// Result of the legacy Markdown fixture decoder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownConversion {
    pub draft: StructuredEntryDraft,
    pub diagnostics: Vec<MarkdownConversionDiagnostic>,
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

/// Parse legacy Markdown into the shared draft shape for fixture decoding.
/// Product transports must use [`structured_fields_to_draft`] instead.
///
/// Frontmatter supplies `form` and `tags`; the first `# ` line supplies the
/// title; every `## ` section supplies one raw string field. Frontmatter keys
/// other than `form`/`tags` are kept as field candidates (sections win),
/// matching the long-standing `extract_properties` behavior where frontmatter
/// flows into properties.
pub fn legacy_markdown_to_draft(markdown: &str, fallback_title: &str) -> MarkdownConversion {
    let (frontmatter, body, mut diagnostics) = extract_frontmatter(markdown);
    let (sections, section_diagnostics) = extract_sections(&body);
    diagnostics.extend(section_diagnostics);
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
    MarkdownConversion {
        draft: StructuredEntryDraft {
            title,
            form_name,
            tags,
            fields,
            extra_attributes: BTreeMap::new(),
        },
        diagnostics,
    }
}

/// Turn Markdown conversion diagnostics into the shared application error
/// used by core, server, and CLI mutation paths.
pub fn markdown_conversion_error(diagnostics: &[MarkdownConversionDiagnostic]) -> AppError {
    AppError::invalid_input_with_detail(
        ErrorCode::MarkdownConversionLoss,
        "Markdown contains content that cannot be represented losslessly as a structured Entry",
        serde_json::json!({"diagnostics": diagnostics}),
    )
}

/// Whether legacy Markdown frontmatter explicitly carries `tags`.
///
/// Updates keep stored tags when `tags` is absent; an explicit `tags: []`
/// clears them. This preserves the long-standing compatibility rule without
/// letting storage parse Markdown itself.
pub fn markdown_frontmatter_has_tags(markdown: &str) -> bool {
    let (frontmatter, _, _) = extract_frontmatter(markdown);
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
/// are a usage error; Markdown duplicate `##` sections are a
/// `MarkdownConversionLoss` diagnostic).
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

/// Decode persisted Form fields through the same coercion boundary used by
/// structured and Markdown drafts.
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

/// Preview legacy Markdown without touching Storage.
///
/// Parses Markdown via [`legacy_markdown_to_draft`] then
/// validates with the same [`normalize_and_validate_draft`] implementation
/// used by mutations, so raw and structured inputs produce identical durable
/// values and identical diagnostics. Loss-producing Markdown is rejected
/// before Form validation.
pub fn preview_legacy_markdown(
    form: &FormDefinition,
    markdown: &str,
    fallback_title: &str,
) -> Result<NormalizedStructuredEntry, AppError> {
    let conversion = legacy_markdown_to_draft(markdown, fallback_title);
    if !conversion.diagnostics.is_empty() {
        return Err(markdown_conversion_error(&conversion.diagnostics));
    }
    normalize_and_validate_draft(form, &conversion.draft)
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

fn extract_frontmatter(content: &str) -> (Value, String, Vec<MarkdownConversionDiagnostic>) {
    let pattern =
        regex::Regex::new(r"(?s)^---\s*\n(.*?)\n---\s*\n").expect("valid frontmatter regex");
    if let Some(caps) = pattern.captures(content) {
        let yaml_str = caps.get(1).map(|m| m.as_str()).unwrap_or_default();
        let end = caps.get(0).map(|m| m.end()).unwrap_or(0);
        let body = content[end..].to_string();
        let mut diagnostics = Vec::new();
        let frontmatter = match serde_yaml::from_str::<serde_yaml::Value>(yaml_str)
            .ok()
            .and_then(|value| serde_json::to_value(value).ok())
        {
            Some(Value::Object(map)) => Value::Object(map),
            Some(_) => {
                diagnostics.push(MarkdownConversionDiagnostic {
                    code: "markdown_frontmatter_not_object".to_string(),
                    message:
                        "Frontmatter must be a key/value object before the Entry can be saved."
                            .to_string(),
                });
                Value::Object(Map::new())
            }
            None => {
                diagnostics.push(MarkdownConversionDiagnostic {
                    code: "markdown_frontmatter_invalid".to_string(),
                    message: "Frontmatter could not be parsed without losing content; fix it before saving."
                        .to_string(),
                });
                Value::Object(Map::new())
            }
        };
        return (frontmatter, body, diagnostics);
    }
    let diagnostics = if content
        .lines()
        .next()
        .is_some_and(|line| line.trim() == "---")
    {
        vec![MarkdownConversionDiagnostic {
            code: "markdown_frontmatter_unclosed".to_string(),
            message: "Frontmatter is not closed; close it before the Entry can be saved."
                .to_string(),
        }]
    } else {
        Vec::new()
    };
    (Value::Object(Map::new()), content.to_string(), diagnostics)
}

fn extract_sections(body: &str) -> (Value, Vec<MarkdownConversionDiagnostic>) {
    let mut sections = Map::new();
    let header = regex::Regex::new(r"^##\s+(.+)$").expect("valid section header regex");
    let mut current_key: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut saw_section = false;
    let mut saw_title = false;
    let mut fenced: Option<String> = None;

    let finish_section =
        |sections: &mut Map<String, Value>,
         current_key: &mut Option<String>,
         buffer: &mut Vec<String>,
         diagnostics: &mut Vec<MarkdownConversionDiagnostic>| {
            if let Some(key) = current_key.take() {
                if sections.contains_key(&key) {
                    diagnostics.push(MarkdownConversionDiagnostic {
                    code: "markdown_duplicate_field_section".to_string(),
                    message: format!("Markdown contains more than one '## {key}' section; merge them before saving."),
                });
                }
                sections.insert(key, Value::String(buffer.join("\n").trim().to_string()));
            }
            buffer.clear();
        };

    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(fence) = fenced.as_ref() {
            if trimmed.starts_with(fence) {
                fenced = None;
            }
            if current_key.is_some() {
                buffer.push(line.to_string());
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = Some(trimmed[..3].to_string());
            if current_key.is_some() {
                buffer.push(line.to_string());
            } else if !saw_section && !trimmed.is_empty() {
                diagnostics.push(MarkdownConversionDiagnostic {
                    code: "markdown_unassigned_preamble".to_string(),
                    message: "Content before the first Entry field cannot be assigned losslessly; move it into a field before saving."
                        .to_string(),
                });
            }
            continue;
        }
        if let Some(caps) = header.captures(line) {
            finish_section(
                &mut sections,
                &mut current_key,
                &mut buffer,
                &mut diagnostics,
            );
            current_key = Some(
                caps.get(1)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default(),
            );
            saw_section = true;
            continue;
        }
        if current_key.is_some() {
            // A field value is itself Markdown text. In particular, nested
            // headings must remain part of the field rather than being
            // discarded as a parser boundary.
            buffer.push(line.to_string());
            continue;
        }

        if !saw_section && !trimmed.is_empty() {
            if !saw_title && line.starts_with("# ") {
                saw_title = true;
            } else {
                diagnostics.push(MarkdownConversionDiagnostic {
                    code: "markdown_unassigned_preamble".to_string(),
                    message: "Content before the first Entry field cannot be assigned losslessly; move it into a field before saving."
                        .to_string(),
                });
            }
        }
    }
    finish_section(
        &mut sections,
        &mut current_key,
        &mut buffer,
        &mut diagnostics,
    );
    (Value::Object(sections), diagnostics)
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

pub fn render_markdown(
    title: &str,
    form_name: &str,
    tags: &[String],
    fields: &Value,
    field_order: &[String],
) -> String {
    let mut markdown = String::new();
    markdown.push_str(&render_frontmatter(form_name, tags));
    if !title.trim().is_empty() {
        markdown.push_str(&format!("# {title}\n\n"));
    }

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

/// Render field values into the section object used by the existing Entry
/// response contract. The string conversion is the same compatibility
/// representation used by [`render_markdown`].
pub fn fields_to_sections(fields: &Value) -> Value {
    let mut sections = Map::new();
    if let Some(map) = fields.as_object() {
        for (key, value) in map {
            sections.insert(key.clone(), Value::String(section_value_to_string(value)));
        }
    }
    Value::Object(sections)
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

    fn draft_from_fixture(value: &Value, fallback_title: &str) -> StructuredEntryDraft {
        let fields = value
            .get("fields")
            .or_else(|| value.get("structured_fields"))
            .and_then(Value::as_object)
            .cloned()
            .or_else(|| value.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .collect();
        structured_fields_to_draft(
            value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or(fallback_title),
            value
                .get("form")
                .or_else(|| value.get("form_name"))
                .and_then(Value::as_str)
                .map(str::to_string),
            value
                .get("tags")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect(),
            fields,
            BTreeMap::new(),
        )
    }

    #[test]
    fn markdown_conversion_preserves_nested_headings_inside_fields() {
        let markdown = "---\nform: Note\n---\n# T\n\n## Body\nintro\n### Details\nkept\n";
        let conversion = legacy_markdown_to_draft(markdown, "fallback");

        assert!(conversion.diagnostics.is_empty());
        assert_eq!(
            conversion.draft.fields.get("Body"),
            Some(&Value::String("intro\n### Details\nkept".to_string()))
        );
    }

    #[test]
    fn markdown_conversion_reports_unassigned_preamble() {
        let markdown = "---\nform: Note\n---\n# T\n\nThis text has no field.\n\n## Body\nkept\n";
        let conversion = legacy_markdown_to_draft(markdown, "fallback");

        assert_eq!(conversion.diagnostics.len(), 1);
        assert_eq!(
            conversion.diagnostics[0].code,
            "markdown_unassigned_preamble"
        );
        let error = preview_legacy_markdown(&test_form(), markdown, "fallback")
            .expect_err("lossy Markdown must not be previewed as saveable");
        assert_eq!(error.code(), ErrorCode::MarkdownConversionLoss);
        assert!(error.detail().is_some_and(|detail| {
            detail["diagnostics"][0]["code"] == "markdown_unassigned_preamble"
        }));
    }

    #[test]
    fn markdown_conversion_reports_invalid_frontmatter() {
        let markdown = "---\nform: [broken\n---\n# T\n\n## Body\nkept\n";
        let conversion = legacy_markdown_to_draft(markdown, "fallback");

        assert_eq!(
            conversion.diagnostics[0].code,
            "markdown_frontmatter_invalid"
        );
        assert_eq!(
            markdown_conversion_error(&conversion.diagnostics).code_str(),
            "MARKDOWN_CONVERSION_LOSS"
        );
    }

    #[test]
    fn markdown_conversion_reports_unclosed_frontmatter() {
        let markdown = "---\nform: Note\n# T\n\n## Body\nkept\n";
        let conversion = legacy_markdown_to_draft(markdown, "fallback");

        assert_eq!(
            conversion.diagnostics[0].code,
            "markdown_frontmatter_unclosed"
        );
    }

    #[test]
    fn legacy_and_structured_drafts_normalize_to_the_same_values() {
        let form = test_form();
        let markdown =
            "---\nform: Note\n---\n# Title\n\n## Body\n\nhello\n\n## Done\nyes\n\n## Count\n42\n";
        let legacy = legacy_markdown_to_draft(markdown, "fallback");
        assert_eq!(legacy.draft.title, "Title");
        assert_eq!(legacy.draft.form_name.as_deref(), Some("Note"));

        let mut fields = BTreeMap::new();
        fields.insert("Body".to_string(), Value::String("hello".to_string()));
        fields.insert("Done".to_string(), Value::Bool(true));
        fields.insert("Count".to_string(), Value::Number(42.into()));
        let structured =
            structured_fields_to_draft("Title", Some("Note"), Vec::new(), fields, BTreeMap::new());

        let from_legacy = normalize_and_validate_draft(&form, &legacy.draft).expect("legacy valid");
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
        let normalized = normalize_and_validate_draft(&form, &draft.draft).expect("valid");
        let rendered = normalized_to_legacy_representation(&form, "Note", &normalized);
        let reparsed = legacy_markdown_to_draft(&rendered, "fallback");
        let renormalized =
            normalize_and_validate_draft(&form, &reparsed.draft).expect("reparsed valid");
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
        let error = normalize_and_validate_draft(&form, &legacy.draft).expect_err("missing Body");
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
    fn duplicate_keys_across_fields_and_extras_are_invalid_input() {
        let form = test_form();
        let draft = structured_fields_to_draft(
            "T",
            Some("Note"),
            Vec::new(),
            BTreeMap::from([("Body".to_string(), Value::String("x".to_string()))]),
            BTreeMap::from([("Body".to_string(), Value::String("shadow".to_string()))]),
        );
        let error = normalize_and_validate_draft(&form, &draft).expect_err("overlap");
        // Stable contract: exactly one code and one detail shape. Message
        // text is not contract.
        assert_eq!(error.code(), ErrorCode::InvalidInput);
        assert_eq!(error.code().as_str(), "INVALID_INPUT");
        assert_eq!(
            error.detail().expect("detail").clone(),
            serde_json::json!({"duplicate_fields": ["Body"]})
        );

        // Deterministic order: multiple duplicates sort lexically regardless
        // of input order.
        let multi = structured_fields_to_draft(
            "T",
            Some("Note"),
            Vec::new(),
            BTreeMap::from([("Done".to_string(), Value::Bool(true))]),
            BTreeMap::from([
                ("Done".to_string(), Value::Bool(false)),
                ("Count".to_string(), Value::Number(1.into())),
            ]),
        );
        let error = normalize_and_validate_draft(&form, &multi).expect_err("multi overlap");
        assert_eq!(error.code().as_str(), "INVALID_INPUT");
        assert_eq!(
            error.detail().expect("detail").clone(),
            serde_json::json!({"duplicate_fields": ["Count", "Done"]})
        );

        // An explicit extra shadowing a real field is the same caller bug
        // even when `fields` does not claim the key.
        let shadow = structured_fields_to_draft(
            "T",
            Some("Note"),
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::from([("Count".to_string(), Value::Number(1.into()))]),
        );
        let error = normalize_and_validate_draft(&form, &shadow).expect_err("shadow");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
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
        let normalized = normalize_and_validate_draft(&form, &draft.draft).expect("aliases valid");
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
            "10-structured-authoring-parity.json",
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
        let normalized = normalize_and_validate_draft(&form, &legacy.draft).expect("legacy valid");
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
            normalize_and_validate_draft(&form, &reparsed.draft).expect("round-trip valid");
        assert_eq!(normalized.values, renormalized.values, "scalar round-trip");

        // Temporal corpus pins normalization (uuid case, binary prefix,
        // timezone canonicalization).
        let raw = std::fs::read_to_string(dir.join("02-temporal.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let form: FormDefinition = serde_json::from_value(fixture["form"].clone()).expect("form");
        let markdown = fixture["markdown"].as_str().expect("markdown");
        let draft = legacy_markdown_to_draft(markdown, "fallback");
        let normalized = normalize_and_validate_draft(&form, &draft.draft).expect("temporal valid");
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
        let normalized = normalize_and_validate_draft(&allow, &draft.draft).expect("allowed extra");
        assert_eq!(
            normalized.extra_attributes.get("Scratch"),
            Some(&Value::String("keep me".to_string()))
        );
        let error = normalize_and_validate_draft(&deny, &draft.draft).expect_err("denied extra");
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
                normalize_and_validate_draft(&form, &draft.draft).is_err(),
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
        let normalized = normalize_and_validate_draft(&form, &draft.draft).expect("legacy valid");
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

    #[test]
    fn lane1_parity_fixture_converges_every_field_family() {
        use ugoite_domain::entry::FieldValue as DomainFieldValue;

        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join("../../fixtures/entry/structured-compat");
        let raw =
            std::fs::read_to_string(dir.join("10-structured-authoring-parity.json")).expect("read");
        let fixture: Value = serde_json::from_str(&raw).expect("json");
        let form: FormDefinition = serde_json::from_value(fixture["form"].clone()).expect("form");

        let draft_from_fields = |title: &str, tags: Vec<String>, fields: Value| {
            let mut map = BTreeMap::new();
            for (key, value) in fields.as_object().cloned().unwrap_or_default() {
                map.insert(key, value);
            }
            structured_fields_to_draft(title, Some("Parity"), tags, map, BTreeMap::new())
        };
        let tags_of = |value: &Value| {
            value
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        };

        // Legacy Markdown and structured inputs reach the same durable outcome.
        let markdown = fixture["markdown"].as_str().expect("markdown");
        let legacy = legacy_markdown_to_draft(markdown, "fallback");
        let from_legacy = preview_structured_draft(&form, &legacy.draft).expect("legacy valid");
        let structured = &fixture["structured"];
        let structured_draft = draft_from_fields(
            structured["title"].as_str().unwrap_or_default(),
            tags_of(&structured["tags"]),
            structured["fields"].clone(),
        );
        let from_structured =
            normalize_and_validate_draft(&form, &structured_draft).expect("structured valid");
        assert_eq!(from_legacy.values, from_structured.values);
        assert_eq!(from_legacy.title, "Website");
        assert_eq!(from_legacy.tags, vec!["inbox".to_string()]);
        assert_eq!(from_structured.tags, vec!["inbox".to_string()]);

        // Stored values match the fixture contract field by field.
        for (id, value) in fixture["expected"]["values"]
            .as_object()
            .cloned()
            .unwrap_or_default()
        {
            let id = FieldId::new(id.parse().expect("field id")).expect("id");
            let expected: DomainFieldValue = serde_json::from_value(value).expect("value");
            assert_eq!(
                from_structured.values.get(&id),
                Some(&expected),
                "parity {id:?}"
            );
        }

        // Updates normalize the same way on every surface.
        let update = &fixture["update"];
        let update_draft = draft_from_fields(
            update["title"].as_str().unwrap_or_default(),
            Vec::new(),
            update["fields"].clone(),
        );
        let updated = preview_structured_draft(&form, &update_draft).expect("update valid");
        for (id, value) in update["expected_values"]
            .as_object()
            .cloned()
            .unwrap_or_default()
        {
            let id = FieldId::new(id.parse().expect("field id")).expect("id");
            let expected: DomainFieldValue = serde_json::from_value(value).expect("value");
            assert_eq!(
                updated.values.get(&id),
                Some(&expected),
                "update parity {id:?}"
            );
        }

        // Every invalid logical input fails with the same code on the
        // mutation implementation and the preview boundary.
        for case in fixture["invalid_cases"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let draft = draft_from_fields("T", Vec::new(), case["fields"].clone());
            let via_mutation = normalize_and_validate_draft(&form, &draft).expect_err("must fail");
            let via_preview = preview_structured_draft(&form, &draft).expect_err("must fail");
            let expected_code = case["code"].as_str().unwrap_or_default();
            assert_eq!(via_mutation.code_str(), expected_code);
            assert_eq!(via_preview.code_str(), expected_code);
            if expected_code == "FORM_VALIDATION_FAILED" {
                let warnings = validation_warnings(&via_preview).expect("warnings");
                assert_eq!(warnings.len(), 1);
                assert_eq!(
                    warnings[0].field,
                    case["field"].as_str().unwrap_or_default()
                );
                assert_eq!(
                    warnings[0].code,
                    case["warning"].as_str().unwrap_or_default()
                );
            } else {
                let names = unknown_field_names(&via_preview).expect("unknown names");
                assert_eq!(
                    names,
                    vec![case["field"].as_str().unwrap_or_default().to_string()]
                );
            }
        }

        // The 0.1 representation round-trips: close/reopen keeps values.
        let rendered = normalized_to_legacy_representation(&form, &form.name, &from_structured);
        let reparsed = legacy_markdown_to_draft(&rendered, "fallback");
        let reopened = preview_structured_draft(&form, &reparsed.draft).expect("reopen valid");
        assert_eq!(from_structured.values, reopened.values);
    }

    fn preview_test_form() -> FormDefinition {
        FormDefinition {
            id: form_id(0xC0),
            version: FormVersion::new(1).expect("version"),
            name: "Preview".to_string(),
            description: None,
            fields: vec![
                required_field(100, "Title", FieldType::String),
                field(101, "Done", FieldType::Boolean),
                field(102, "Count", FieldType::Integer),
                field(103, "Score", FieldType::Double),
                field(104, "Due", FieldType::Date),
                field(105, "At", FieldType::Timestamp),
                field(106, "AtTz", FieldType::TimestampTz),
                FormField {
                    list_item: None,
                    ..field(107, "Tags", FieldType::List)
                },
                field(108, "Rows", FieldType::ObjectList),
                field(109, "Ref", FieldType::RowReference),
                field(110, "File", FieldType::AssetReference),
            ],
            allow_extra_attributes: false,
            extension_metadata: BTreeMap::new(),
        }
    }

    fn invalid_preview_drafts() -> Vec<(String, BTreeMap<String, Value>)> {
        let mut cases = Vec::new();
        let bad_asset = serde_json::json!({"asset_id": "not-a-uuid"});
        let push = |name: &str, value: Value, out: &mut Vec<(String, BTreeMap<String, Value>)>| {
            let mut fields = BTreeMap::new();
            fields.insert("Title".to_string(), Value::String("ok".to_string()));
            fields.insert(name.to_string(), value);
            out.push((name.to_string(), fields));
        };
        push("Done", Value::String("maybe".to_string()), &mut cases);
        push("Count", Value::String("not-an-int".to_string()), &mut cases);
        push(
            "Score",
            Value::String("not-a-number".to_string()),
            &mut cases,
        );
        push("Due", Value::String("2026-13-40".to_string()), &mut cases);
        push(
            "At",
            Value::String("not-a-timestamp".to_string()),
            &mut cases,
        );
        push(
            "AtTz",
            Value::String("2026-09-11T10:00:00".to_string()),
            &mut cases,
        );
        push("Tags", Value::Number(42.into()), &mut cases);
        push(
            "Rows",
            Value::String("not-an-array".to_string()),
            &mut cases,
        );
        push("Ref", Value::Number(7.into()), &mut cases);
        push("File", bad_asset, &mut cases);
        cases
    }

    #[test]
    fn preview_matches_mutation_diagnostics_for_every_field_family() {
        let form = preview_test_form();
        for (field_name, fields) in invalid_preview_drafts() {
            let draft = structured_fields_to_draft(
                "T",
                Some("Preview"),
                Vec::new(),
                fields,
                BTreeMap::new(),
            );
            let via_mutation =
                normalize_and_validate_draft(&form, &draft).expect_err("mutation path must fail");
            let via_preview =
                preview_structured_draft(&form, &draft).expect_err("preview must fail");
            assert_eq!(
                via_mutation.code(),
                via_preview.code(),
                "code parity for {field_name}"
            );
            assert_eq!(
                via_mutation.code(),
                ErrorCode::FormValidationFailed,
                "invalid {field_name} is a field failure"
            );
            let mutation_warnings = validation_warnings(&via_mutation).expect("mutation warnings");
            let preview_warnings = validation_warnings(&via_preview).expect("preview warnings");
            assert_eq!(
                mutation_warnings, preview_warnings,
                "diagnostic parity for {field_name}"
            );
            assert_eq!(
                preview_warnings.len(),
                1,
                "one field fails for {field_name}"
            );
            let warning = &preview_warnings[0];
            assert_eq!(warning.field, field_name);
            assert_eq!(warning.code, "invalid_type");
            assert!(
                !warning.expected_type.is_empty(),
                "expected_type for {field_name}"
            );
            assert!(
                !warning.expected_format.is_empty(),
                "expected_format for {field_name}"
            );
            assert!(!warning.reason.is_empty(), "reason for {field_name}");
            assert!(!warning.message.is_empty(), "message for {field_name}");
        }
    }

    #[test]
    fn preview_markdown_and_structured_share_diagnostics() {
        let form = test_form();
        let markdown = "---\nform: Note\n---\n# T\n\n## Body\nhello\n\n## Done\nmaybe\n";
        let via_markdown =
            preview_legacy_markdown(&form, markdown, "T").expect_err("markdown invalid");
        let mut fields = BTreeMap::new();
        fields.insert("Body".to_string(), Value::String("hello".to_string()));
        fields.insert("Done".to_string(), Value::String("maybe".to_string()));
        fields.insert("Count".to_string(), Value::Number(1.into()));
        let draft =
            structured_fields_to_draft("T", Some("Note"), Vec::new(), fields, BTreeMap::new());
        let via_structured =
            preview_structured_draft(&form, &draft).expect_err("structured invalid");
        assert_eq!(via_markdown.code(), via_structured.code());
        assert_eq!(
            validation_warnings(&via_markdown),
            validation_warnings(&via_structured)
        );
    }

    #[test]
    fn preview_unknown_fields_match_mutation_taxonomy() {
        let form = test_form();
        let mut fields = BTreeMap::new();
        fields.insert("Body".to_string(), Value::String("hello".to_string()));
        fields.insert("Nope".to_string(), Value::String("x".to_string()));
        let draft =
            structured_fields_to_draft("T", Some("Note"), Vec::new(), fields, BTreeMap::new());
        let via_mutation = normalize_and_validate_draft(&form, &draft).expect_err("unknown field");
        let via_preview = preview_structured_draft(&form, &draft).expect_err("unknown field");
        assert_eq!(via_mutation.code(), ErrorCode::UnknownFormFields);
        assert_eq!(via_preview.code(), ErrorCode::UnknownFormFields);
        assert_eq!(
            unknown_field_names(&via_mutation),
            Some(vec!["Nope".to_string()])
        );
        assert_eq!(
            unknown_field_names(&via_preview),
            unknown_field_names(&via_mutation)
        );
        assert_eq!(validation_warnings(&via_preview), None);
    }

    #[test]
    fn preview_is_pure_and_returns_normalized_draft() {
        let form = test_form();
        let mut fields = BTreeMap::new();
        fields.insert("Body".to_string(), Value::String("hello".to_string()));
        fields.insert("Done".to_string(), Value::Bool(true));
        fields.insert("Count".to_string(), Value::Number(3.into()));
        let draft = structured_fields_to_draft(
            "T",
            Some("Note"),
            vec!["a".to_string()],
            fields,
            BTreeMap::new(),
        );
        // Preview never touches Storage: calling it repeatedly yields the
        // same normalized value with no revision/Change side effects (it is a
        // pure function over form + draft).
        let first = preview_structured_draft(&form, &draft).expect("valid");
        let second = preview_structured_draft(&form, &draft).expect("valid");
        assert_eq!(first, second);
        assert_eq!(
            normalize_and_validate_draft(&form, &draft).expect("mutation impl"),
            first
        );
        assert_eq!(first.title, "T");
        assert_eq!(first.tags, vec!["a".to_string()]);
    }

    #[test]
    fn draft_and_normalized_values_serde_round_trip_with_stable_field_keys() {
        let form = preview_test_form();
        let draft = structured_fields_to_draft(
            "T",
            Some("Preview"),
            vec!["one".into()],
            BTreeMap::from([
                ("Title".into(), Value::String("hello".into())),
                ("Score".into(), serde_json::json!(1.23456789012345)),
            ]),
            BTreeMap::new(),
        );
        let draft_json = serde_json::to_value(&draft).expect("draft serializes");
        assert_eq!(
            draft_json["fields"]["Score"],
            serde_json::json!(1.23456789012345)
        );
        assert_eq!(
            serde_json::from_value::<StructuredEntryDraft>(draft_json).unwrap(),
            draft
        );

        let normalized = normalize_and_validate_draft(&form, &draft).expect("draft is valid");
        let normalized_json = serde_json::to_value(&normalized).expect("normalized serializes");
        assert_eq!(
            normalized_json["values"]["103"],
            serde_json::json!(1.23456789012345)
        );
        assert_eq!(
            serde_json::from_value::<NormalizedStructuredEntry>(normalized_json).unwrap(),
            normalized
        );
    }

    #[test]
    fn diagnostic_extractors_fail_closed_for_missing_or_unexpected_detail() {
        let errors = [
            AppError::invalid_input_with_detail(
                ErrorCode::FormValidationFailed,
                "missing",
                Value::Null,
            ),
            AppError::invalid_input_with_detail(
                ErrorCode::FormValidationFailed,
                "wrong warnings",
                serde_json::json!({"warnings": {"field": "Body"}}),
            ),
            AppError::invalid_input_with_detail(
                ErrorCode::UnknownFormFields,
                "wrong fields",
                serde_json::json!({"fields": [1, {"unexpected": true}]}),
            ),
        ];
        assert!(errors
            .iter()
            .all(|error| validation_warnings(error).is_none()));
        assert!(errors
            .iter()
            .all(|error| unknown_field_names(error).is_none()));
    }

    #[test]
    fn malformed_asset_reference_is_rejected_by_the_core_boundary() {
        let form = preview_test_form();
        let invalid = serde_json::json!({
            "asset_id": "01900000-0000-7000-8000-000000000001",
            "name": "file.txt"
        });
        let draft = structured_fields_to_draft(
            "T",
            Some("Preview"),
            Vec::new(),
            BTreeMap::from([
                ("Title".into(), Value::String("hello".into())),
                ("File".into(), invalid),
            ]),
            BTreeMap::new(),
        );
        let error = preview_structured_draft(&form, &draft).expect_err("incomplete reference");
        assert_eq!(error.code(), ErrorCode::FormValidationFailed);
        assert_eq!(validation_warnings(&error).unwrap()[0].field, "File");
    }

    #[test]
    fn persisted_field_decode_keeps_legacy_read_contract() {
        let form = preview_test_form();
        let error = stored_fields_to_values(&serde_json::json!({"Count": "not-an-int"}), &form)
            .expect_err("invalid persisted value");
        assert_eq!(error.code(), ErrorCode::InvalidInput);
        assert_eq!(
            stored_fields_to_values(&Value::Null, &form).expect("legacy non-object payload"),
            BTreeMap::new()
        );
    }

    #[test]
    fn empty_object_list_items_are_rejected_on_write_but_readable_from_legacy_storage() {
        use crate::error::ErrorKind;

        let form = preview_test_form();
        // New writes carrying an empty object as a meaningful object_list
        // item are rejected with field guidance (INVALID_INPUT kind).
        let draft = structured_fields_to_draft(
            "T",
            Some("Preview"),
            Vec::new(),
            BTreeMap::from([
                ("Title".into(), Value::String("hello".into())),
                ("Rows".into(), serde_json::json!([{}])),
            ]),
            BTreeMap::new(),
        );
        let error =
            normalize_and_validate_draft(&form, &draft).expect_err("[{}] must fail on write");
        assert_eq!(error.code(), ErrorCode::FormValidationFailed);
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let warnings = validation_warnings(&error).expect("field guidance");
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].field, "Rows");
        assert_eq!(warnings[0].code, "invalid_type");
        // The preview boundary agrees with the mutation path.
        let preview_error = preview_structured_draft(&form, &draft).expect_err("preview must fail");
        assert_eq!(preview_error.code(), ErrorCode::FormValidationFailed);
        // The JSON-string write shape is rejected the same way.
        let string_draft = structured_fields_to_draft(
            "T",
            Some("Preview"),
            Vec::new(),
            BTreeMap::from([
                ("Title".into(), Value::String("hello".into())),
                ("Rows".into(), Value::String("[{}]".to_string())),
            ]),
            BTreeMap::new(),
        );
        normalize_and_validate_draft(&form, &string_draft).expect_err("[{}] string must fail");
        // Legacy storage rows stay readable without migration or format change.
        let values = stored_fields_to_values(&serde_json::json!({"Rows": [{}]}), &form)
            .expect("legacy [{}] stays readable");
        assert_eq!(
            values.get(&FieldId::new(108).unwrap()),
            Some(&FieldValue::List(vec![FieldValue::Object(BTreeMap::new())]))
        );
        // Non-empty items keep passing the write boundary.
        let valid_draft = structured_fields_to_draft(
            "T",
            Some("Preview"),
            Vec::new(),
            BTreeMap::from([
                ("Title".into(), Value::String("hello".into())),
                ("Rows".into(), serde_json::json!([{"name": "alpha"}])),
            ]),
            BTreeMap::new(),
        );
        normalize_and_validate_draft(&form, &valid_draft).expect("non-empty items pass");
    }

    #[test]
    fn compatibility_fixture_cases_cover_all_declared_rules() {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/entry/structured-compat");
        let fixture: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("04-compat-rules.json")).expect("read rules"),
        )
        .expect("rules JSON");
        let allow: FormDefinition =
            serde_json::from_value(fixture["form_allow"].clone()).expect("allow form");
        let deny: FormDefinition =
            serde_json::from_value(fixture["form_deny"].clone()).expect("deny form");

        for case in fixture["cases"].as_array().expect("cases") {
            let markdown = case["markdown"].as_str().expect("case markdown");
            let parsed = legacy_markdown_to_draft(markdown, "fallback");
            assert!(parsed.diagnostics.is_empty(), "{}", case["name"]);
            let form = if case["allow_extra"].as_bool().unwrap_or(true) {
                &allow
            } else {
                &deny
            };
            let normalized = normalize_and_validate_draft(form, &parsed.draft);
            if let Some(expected_fields) = case["expected_values"].as_object() {
                let normalized = normalized.expect("case should normalize");
                for (id, expected) in expected_fields {
                    let id = FieldId::new(id.parse().expect("field id")).expect("valid id");
                    let expected = if expected == "Null" {
                        FieldValue::Null
                    } else {
                        serde_json::from_value::<FieldValue>(expected.clone()).expect("field value")
                    };
                    assert_eq!(
                        normalized.values.get(&id),
                        Some(&expected),
                        "{}",
                        case["name"]
                    );
                }
            } else if let Some(expected_extra) = case["expected_extra"].as_object() {
                let normalized = normalized.expect("extra case should normalize");
                for (name, expected) in expected_extra {
                    assert_eq!(normalized.extra_attributes.get(name), Some(expected));
                }
            } else if let Some(expected_fields) = case["expect_unknown_fields"].as_array() {
                let error = normalized.expect_err("unknown field should fail");
                assert_eq!(error.code(), ErrorCode::UnknownFormFields);
                assert_eq!(
                    unknown_field_names(&error).unwrap(),
                    expected_fields
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                );
            } else {
                let normalized = normalized.expect("alias/list case should normalize");
                if case["name"] == "boolean-aliases" {
                    assert_eq!(
                        normalized.values.get(&FieldId::new(101).unwrap()),
                        Some(&FieldValue::Boolean(true))
                    );
                    let structured = draft_from_fixture(&case["structured_fields"], "T");
                    let structured = normalize_and_validate_draft(&allow, &structured)
                        .expect("structured alias");
                    assert_eq!(
                        structured.values.get(&FieldId::new(101).unwrap()),
                        Some(&FieldValue::Boolean(false))
                    );
                }
                if case["name"] == "markdown-list-syntax" {
                    assert_eq!(
                        normalized.values.get(&FieldId::new(103).unwrap()),
                        Some(&FieldValue::List(vec![
                            FieldValue::String("Alpha".into()),
                            FieldValue::String("Beta".into()),
                            FieldValue::String("Gamma".into()),
                        ]))
                    );
                }
            }
        }

        let required: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("05-required.json")).expect("read required"),
        )
        .expect("required JSON");
        let form: FormDefinition =
            serde_json::from_value(required["form"].clone()).expect("required form");
        let valid = &required["valid"];
        let markdown = legacy_markdown_to_draft(valid["markdown"].as_str().unwrap(), "T");
        assert!(normalize_and_validate_draft(&form, &markdown.draft).is_ok());
        assert!(normalize_and_validate_draft(&form, &draft_from_fixture(valid, "T")).is_ok());
        for fields in required["invalid_structured"].as_array().unwrap() {
            assert!(normalize_and_validate_draft(
                &form,
                &draft_from_fixture(&serde_json::json!({"fields": fields}), "T")
            )
            .is_err());
        }
        for markdown in required["invalid_markdowns"].as_array().unwrap() {
            let draft = legacy_markdown_to_draft(markdown.as_str().unwrap(), "T");
            assert!(normalize_and_validate_draft(&form, &draft.draft).is_err());
        }
    }
}
