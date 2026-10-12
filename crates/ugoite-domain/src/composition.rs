//! Typed Composition v1 domain contract.
//!
//! This module intentionally contains no YAML parser, storage, or query
//! execution code. It defines the shared Rust model that native and WASM
//! adapters use when parsing and resolving a Composition.

use crate::form::{FieldType, ListItemDefinition};
use crate::id::{EntryId, FieldId, FormId, RevisionId};
use chrono::{DateTime, Datelike, NaiveDate};
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

mod canonical;
mod yaml;

pub use canonical::{
    canonicalize_composition, canonicalize_composition_document_value,
    canonicalize_composition_yaml, CanonicalComposition,
};
pub use yaml::{
    parse_composition_yaml, MAX_COMPOSITION_COLLECTION_ITEMS, MAX_COMPOSITION_YAML_BYTES,
    MAX_COMPOSITION_YAML_DEPTH,
};

pub const COMPOSITION_FORMAT_VERSION: u32 = 1;
pub const COMPOSITION_FORMAT: &str = "ugoite.composition";

/// Check the portable envelope before applying the strict v1 schema.
///
/// Keeping this preflight separate lets adapters receiving an already decoded
/// JSON value report an unsupported future version even when that document
/// contains fields that are unknown to v1.
pub(super) fn validate_composition_document_envelope(
    value: &Value,
) -> Result<(), CompositionDiagnosticCode> {
    if value.get("format").and_then(Value::as_str) != Some(COMPOSITION_FORMAT) {
        return Err(CompositionDiagnosticCode::InvalidComposition);
    }
    let version = value
        .get("format_version")
        .and_then(Value::as_u64)
        .ok_or(CompositionDiagnosticCode::InvalidComposition)?;
    if version != u64::from(COMPOSITION_FORMAT_VERSION) {
        return Err(CompositionDiagnosticCode::UnsupportedFormatVersion);
    }
    Ok(())
}
/// Default page size for an EntryQuery source when the document omits it.
/// The core resolver still validates the value against EntryQuery's maximum.
pub const DEFAULT_COMPOSITION_PAGE_LIMIT: usize = 100;

/// A complete portable Composition document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDocument {
    pub format: CompositionFormat,
    pub format_version: u32,
    pub kind: CompositionKind,
    pub name: String,
    pub tags: Vec<String>,
    pub spec: CompositionSpec,
}

impl CompositionDocument {
    /// Return the stable domain diagnostic for an unsupported document
    /// version. Structural and source-reference checks are performed by the
    /// parser and resolver that consume this model.
    pub fn validate_format_version(&self) -> Result<(), CompositionDiagnosticCode> {
        if self.format_version == COMPOSITION_FORMAT_VERSION {
            Ok(())
        } else {
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        }
    }
}

/// The portable envelope marker for Composition documents.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum CompositionFormat {
    #[serde(rename = "ugoite.composition")]
    UgoiteComposition,
}

/// The executable Composition kind supported by format version 1.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionKind {
    Dashboard,
}

/// The dashboard definition carried by a Composition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<CompositionVariable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<CompositionSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<CompositionComponent>,
    pub layout: DashboardFlowLayout,
}

impl CompositionSpec {
    /// Return components in layout row order and each row's item order.
    ///
    /// Components are placed exactly once; Inputs reference declared variables
    /// and required variables without defaults have a placed Input. Layout
    /// order is presentation-only and does not define query execution order.
    pub fn components_in_render_order(
        &self,
    ) -> Result<Vec<&CompositionComponent>, CompositionDiagnosticCode> {
        let mut variables_by_id = HashMap::with_capacity(self.variables.len());
        for variable in &self.variables {
            if !variable.is_well_formed()
                || variables_by_id
                    .insert(variable.id.as_str(), variable)
                    .is_some()
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
        }

        let known_variables: HashSet<_> = variables_by_id.keys().copied().collect();
        let value_uses_known_variable = |value: &CompositionValue| match value {
            CompositionValue::Variable(reference) => {
                known_variables.contains(reference.variable.as_str())
            }
            CompositionValue::Literal(_) => true,
        };

        for source in &self.sources {
            match source {
                CompositionSource::EntryQuery { query, .. } => {
                    if query
                        .text
                        .as_ref()
                        .is_some_and(|value| !value_uses_known_variable(value))
                        || query
                            .filters
                            .iter()
                            .any(|filter| !value_uses_known_variable(&filter.value))
                    {
                        return Err(CompositionDiagnosticCode::InvalidComposition);
                    }
                }
                CompositionSource::SavedSql { variables, .. } => {
                    if variables
                        .values()
                        .any(|value| !value_uses_known_variable(value))
                    {
                        return Err(CompositionDiagnosticCode::InvalidComposition);
                    }
                }
            }
        }

        let mut components_by_id = HashMap::with_capacity(self.components.len());
        let mut input_variables = HashSet::with_capacity(self.variables.len());
        for component in &self.components {
            if component.id().trim().is_empty()
                || components_by_id.insert(component.id(), component).is_some()
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
            match component {
                CompositionComponent::Input { variable, .. } => {
                    if !known_variables.contains(variable.as_str()) {
                        return Err(CompositionDiagnosticCode::InvalidComposition);
                    }
                    input_variables.insert(variable.as_str());
                }
                CompositionComponent::Text {
                    content: CompositionTextContent::Variable { variable },
                    ..
                } => {
                    if !variables_by_id
                        .get(variable.as_str())
                        .is_some_and(|definition| {
                            definition.variable_type == CompositionVariableType::String
                        })
                    {
                        return Err(CompositionDiagnosticCode::InvalidComposition);
                    }
                }
                CompositionComponent::Text { .. }
                | CompositionComponent::Metric { .. }
                | CompositionComponent::Table { .. } => {}
            }
        }

        for variable in &self.variables {
            if variable.required
                && variable.default.is_none()
                && !input_variables.contains(variable.id.as_str())
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
        }

        if self.layout.rows.is_empty() {
            return Err(CompositionDiagnosticCode::InvalidComposition);
        }

        let mut row_ids = HashSet::with_capacity(self.layout.rows.len());
        let mut rendered_ids = HashSet::with_capacity(self.components.len());
        let mut ordered = Vec::with_capacity(self.components.len());
        for row in &self.layout.rows {
            if row.id.trim().is_empty() || !row_ids.insert(row.id.as_str()) || row.items.is_empty()
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
            for item in &row.items {
                let FlowItem::Component { component } = item;
                if !rendered_ids.insert(component.as_str()) {
                    return Err(CompositionDiagnosticCode::InvalidComposition);
                }
                let component = components_by_id
                    .get(component.as_str())
                    .copied()
                    .ok_or(CompositionDiagnosticCode::InvalidComposition)?;
                ordered.push(component);
            }
        }

        if rendered_ids.len() != components_by_id.len() {
            return Err(CompositionDiagnosticCode::InvalidComposition);
        }

        Ok(ordered)
    }
}

/// Maximum UTF-8 byte length accepted for a typed Composition string value.
pub const MAX_COMPOSITION_VARIABLE_STRING_BYTES: usize = 4 * 1024;

/// A typed value declared by a Composition and shared by its Inputs, Text,
/// and data Sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionVariable {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "type")]
    pub variable_type: CompositionVariableType,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<CompositionLiteral>,
    /// A presentation hint that never changes the value's type or parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<CompositionVariableFormat>,
}

impl CompositionVariable {
    fn is_well_formed(&self) -> bool {
        !self.id.trim().is_empty()
            && self
                .label
                .as_deref()
                .is_none_or(|label| !label.trim().is_empty())
            && self
                .default
                .as_ref()
                .is_none_or(|value| self.variable_type.accepts_value(value.as_json_value()))
            && self
                .format
                .is_none_or(|_| self.variable_type == CompositionVariableType::Date)
    }
}

/// Supported typed Composition variable values.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionVariableType {
    String,
    Boolean,
    Integer,
    Float,
    Date,
    Timestamp,
}

impl CompositionVariableType {
    /// Return whether a JSON scalar is a valid value of this exact type.
    ///
    /// These checks are shared by Domain defaults and Core runtime bindings;
    /// callers must not coerce values before invoking them.
    pub fn accepts_value(self, value: &Value) -> bool {
        match self {
            Self::String => value.as_str().is_some_and(valid_variable_string),
            Self::Boolean => value.is_boolean(),
            Self::Integer => value.as_i64().is_some_and(|value| {
                (-(9_007_199_254_740_991_i64)..=9_007_199_254_740_991_i64).contains(&value)
            }),
            Self::Float => value.as_f64().is_some_and(f64::is_finite),
            Self::Date => value.as_str().is_some_and(valid_variable_date),
            Self::Timestamp => value.as_str().is_some_and(valid_variable_timestamp),
        }
    }

    /// Parse one locale-independent raw Input buffer without coercion.
    ///
    /// Empty text is `Unset`; malformed text is `Invalid`. Callers retain the
    /// raw buffer separately so an invalid edit remains visible without
    /// turning it into a guessed typed value.
    pub fn parse_input(self, raw: &str) -> CompositionVariableInputValue {
        if raw.is_empty() {
            return CompositionVariableInputValue::Unset;
        }

        let value = match self {
            Self::String if valid_variable_string(raw) => Value::String(raw.to_owned()),
            Self::Boolean => match raw {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                _ => return CompositionVariableInputValue::Invalid,
            },
            Self::Integer => match parse_variable_integer(raw) {
                Some(value) => Value::from(value),
                None => return CompositionVariableInputValue::Invalid,
            },
            Self::Float => match parse_variable_float(raw) {
                Some(value) => {
                    let Some(number) = serde_json::Number::from_f64(value) else {
                        return CompositionVariableInputValue::Invalid;
                    };
                    Value::Number(number)
                }
                None => return CompositionVariableInputValue::Invalid,
            },
            Self::Date if valid_variable_date(raw) => Value::String(raw.to_owned()),
            Self::Timestamp if valid_variable_timestamp(raw) => Value::String(raw.to_owned()),
            _ => return CompositionVariableInputValue::Invalid,
        };

        if self.accepts_value(&value) {
            CompositionVariableInputValue::Valid(value)
        } else {
            CompositionVariableInputValue::Invalid
        }
    }
}

/// The outcome of validating one raw Composition Input buffer.
#[derive(Debug, Clone, PartialEq)]
pub enum CompositionVariableInputValue {
    Unset,
    Valid(Value),
    Invalid,
}

/// Supported variable display hints. Hints never change value parsing.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionVariableFormat {
    YearMonth,
}

fn valid_variable_string(value: &str) -> bool {
    value.len() <= MAX_COMPOSITION_VARIABLE_STRING_BYTES
        && value
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
}

fn parse_variable_integer(raw: &str) -> Option<i64> {
    if raw.len() > 17 || !raw.is_ascii() {
        return None;
    }

    let digits = raw
        .strip_prefix('+')
        .or_else(|| raw.strip_prefix('-'))
        .unwrap_or(raw)
        .as_bytes();
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }

    let value = raw.parse::<i64>().ok()?;
    (-(9_007_199_254_740_991_i64)..=9_007_199_254_740_991_i64)
        .contains(&value)
        .then_some(value)
}

fn parse_variable_float(raw: &str) -> Option<f64> {
    if raw.len() > 64 || !raw.is_ascii() || raw.is_empty() {
        return None;
    }

    let bytes = raw.as_bytes();
    let mut index = usize::from(matches!(bytes[0], b'+' | b'-'));
    if index == bytes.len() {
        return None;
    }

    let integer_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    let has_integer_digits = index > integer_start;

    let mut has_fraction_digits = false;
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let fraction_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        has_fraction_digits = index > fraction_start;
        if !has_fraction_digits {
            return None;
        }
    }
    if !has_integer_digits && !has_fraction_digits {
        return None;
    }

    if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
        index += 1;
        if index < bytes.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }
        let exponent_start = index;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
        if index == exponent_start {
            return None;
        }
    }
    if index != bytes.len() {
        return None;
    }

    let mantissa_end = bytes
        .iter()
        .position(|byte| matches!(*byte, b'e' | b'E'))
        .unwrap_or(bytes.len());
    if !bytes[..mantissa_end]
        .iter()
        .any(|byte| matches!(*byte, b'1'..=b'9'))
    {
        return Some(if bytes[0] == b'-' { -0.0 } else { 0.0 });
    }

    let value = raw.parse::<f64>().ok()?;
    if !value.is_finite() || (value == 0.0 && bytes.iter().any(|byte| matches!(byte, b'1'..=b'9')))
    {
        return None;
    }
    Some(value)
}

fn valid_variable_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || !value.is_ascii()
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return false;
    }

    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .is_some_and(|date| {
            (1..=9999).contains(&date.year()) && date.format("%Y-%m-%d").to_string() == value
        })
}

fn valid_variable_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len())
        || !value.is_ascii()
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || !valid_variable_date(&value[..10])
    {
        return false;
    }

    let zone_start = if bytes.last() == Some(&b'Z') {
        bytes.len() - 1
    } else {
        if bytes.len() < 25 {
            return false;
        }
        let zone_start = bytes.len() - 6;
        if !matches!(bytes[zone_start], b'+' | b'-') || bytes[zone_start + 3] != b':' {
            return false;
        }
        let hour = decimal_pair(bytes[zone_start + 1], bytes[zone_start + 2]);
        let minute = decimal_pair(bytes[zone_start + 4], bytes[zone_start + 5]);
        if hour.is_none_or(|hour| hour > 23)
            || minute.is_none_or(|minute| minute > 59)
            || (bytes[zone_start] == b'-' && hour == Some(0) && minute == Some(0))
        {
            return false;
        }
        zone_start
    };

    let local = &bytes[..zone_start];
    if local.len() < 19 {
        return false;
    }
    let time_digits = [11, 12, 14, 15, 17, 18];
    if !time_digits
        .iter()
        .all(|index| local[*index].is_ascii_digit())
    {
        return false;
    }
    let hour = decimal_pair(local[11], local[12]);
    let minute = decimal_pair(local[14], local[15]);
    let second = decimal_pair(local[17], local[18]);
    if hour.is_none_or(|hour| hour > 23)
        || minute.is_none_or(|minute| minute > 59)
        || second.is_none_or(|second| second > 59)
    {
        return false;
    }
    if local.len() > 19
        && (local[19] != b'.'
            || !(1..=9).contains(&(local.len() - 20))
            || !local[20..].iter().all(u8::is_ascii_digit))
    {
        return false;
    }

    DateTime::parse_from_rfc3339(value).is_ok()
}

fn decimal_pair(tens: u8, ones: u8) -> Option<u8> {
    (tens.is_ascii_digit() && ones.is_ascii_digit()).then_some((tens - b'0') * 10 + (ones - b'0'))
}

/// A source definition in a dashboard.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionSource {
    EntryQuery {
        id: String,
        form_id: FormId,
        #[serde(default)]
        field_schema: Vec<CompositionFieldSchemaEntry>,
        query: EntryQueryTemplate,
    },
    SavedSql {
        id: String,
        entry_id: EntryId,
        revision_id: RevisionId,
        expected_result: Vec<CompositionResultColumn>,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        variables: BTreeMap<String, CompositionValue>,
    },
}

impl CompositionSource {
    pub fn id(&self) -> &str {
        match self {
            Self::EntryQuery { id, .. } | Self::SavedSql { id, .. } => id,
        }
    }
}

/// One ordered output column declared for an exact Saved SQL revision.
///
/// SQL engines do not expose a portable static type system, so this contract
/// records the logical type the Composition expects rather than backend type
/// strings. Column order is significant and names must be unique within a
/// source result.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResultColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub result_type: CompositionResultFieldType,
}

/// Portable logical type of a source result column.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionResultFieldType {
    String,
    Boolean,
    Integer,
    Float,
    Decimal,
    Date,
    Timestamp,
    Json,
}

impl CompositionResultFieldType {
    /// Whether this result type can be selected by a single-value metric.
    pub const fn supports_metric(self) -> bool {
        !matches!(self, Self::Json)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Boolean => "boolean",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Decimal => "decimal",
            Self::Date => "date",
            Self::Timestamp => "timestamp",
            Self::Json => "json",
        }
    }
}

/// The logical schema snapshot of one Form field used by an EntryQuery,
/// including typed List item and RowReference target metadata.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct CompositionFieldSchemaEntry {
    pub field_id: FieldId,
    pub field_type: FieldType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_form: Option<FormId>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "items")]
    pub list_item: Option<ListItemDefinition>,
}

impl<'de> Deserialize<'de> for CompositionFieldSchemaEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct FieldSchemaEntryWire {
            field_id: FieldId,
            field_type: FieldType,
            #[serde(default)]
            reference_form: Option<FormId>,
            #[serde(default, rename = "items")]
            list_item: Option<ListItemSchemaWire>,
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ListItemSchemaWire {
            #[serde(rename = "type")]
            field_type: FieldType,
            #[serde(default, rename = "target_form")]
            reference_form: Option<FormId>,
        }

        let entry = FieldSchemaEntryWire::deserialize(deserializer)?;
        Ok(Self {
            field_id: entry.field_id,
            field_type: entry.field_type,
            reference_form: entry.reference_form,
            list_item: entry.list_item.map(|item| ListItemDefinition {
                field_type: item.field_type,
                reference_form: item.reference_form,
            }),
        })
    }
}

/// An EntryQuery template. The resolver compiles this into the existing
/// bounded `EntryQuery` type after binding variables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryQueryTemplate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<CompositionValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<EntryQueryFilterTemplate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sort: Vec<EntryQuerySortTemplate>,
    #[serde(default = "default_composition_page_limit")]
    pub page_limit: usize,
    #[serde(default)]
    pub projection: EntryQueryProjectionTemplate,
    /// Optional system columns rendered by Composition tables. Entry result
    /// rows already carry these timestamps, so they do not change the query
    /// projection sent to `entry.query`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub display_system_fields: Vec<EntryQueryDisplaySystemField>,
}

const fn default_composition_page_limit() -> usize {
    DEFAULT_COMPOSITION_PAGE_LIMIT
}

/// A filter whose value may be a literal or a named variable reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryQueryFilterTemplate {
    pub field_id: FieldId,
    pub operator: CompositionQueryOperator,
    pub value: CompositionValue,
}

/// The filter operators already supported by EntryQuery.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionQueryOperator {
    Equals,
    Contains,
    Lt,
    Lte,
    Gt,
    Gte,
}

/// One sort clause in an EntryQuery template.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct EntryQuerySortTemplate {
    pub field_id: FieldId,
    pub direction: CompositionSortDirection,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionSortDirection {
    Asc,
    Desc,
}

/// Projection semantics that compile to the existing EntryProjection type.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryQueryProjectionTemplate {
    #[default]
    Preview,
    Fields {
        fields: Vec<FieldId>,
    },
}

/// Entry identity timestamps that a Composition table may display.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryQueryDisplaySystemField {
    CreatedAt,
    UpdatedAt,
}

/// A source value expressed directly or as a Composition variable reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompositionValue {
    Variable(CompositionVariableReference),
    Literal(CompositionLiteral),
}

/// A JSON scalar used as a variable default or source literal.
///
/// YAML object and sequence values are intentionally not literals in the
/// Composition query contract.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CompositionLiteral(Value);

impl CompositionLiteral {
    pub fn as_json_value(&self) -> &Value {
        &self.0
    }

    pub fn into_json_value(self) -> Value {
        self.0
    }
}

impl<'de> Deserialize<'de> for CompositionLiteral {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(Self(value)),
            Value::Array(_) | Value::Object(_) => Err(D::Error::custom(
                "Composition literal values must be scalar",
            )),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionVariableReference {
    pub variable: String,
}

/// Portable formatting hints for a metric value. These fields affect display
/// only; they do not alter the stored definition or query semantics.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionMetricPresentation {
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fraction_digits: Option<u8>,
    },
}

impl CompositionMetricPresentation {
    pub(crate) fn is_well_formed(&self) -> bool {
        match self {
            Self::Number { unit, .. } => {
                unit.as_deref().is_none_or(|value| !value.trim().is_empty())
            }
        }
    }
}

/// A visual component supported by the dashboard renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionComponent {
    Input {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        variable: String,
    },
    Text {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        content: CompositionTextContent,
        style: TextStyle,
    },
    Metric {
        id: String,
        /// Optional display text; component and source bindings continue to use IDs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        /// Shared Saved SQL source whose exact revision is resolved by Core.
        source: String,
        /// One scalar output column returned by the source.
        column: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presentation: Option<CompositionMetricPresentation>,
    },
    Table {
        id: String,
        /// Optional display text; component and source bindings continue to use IDs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        source: String,
    },
}

/// Text content is a literal or a direct reference to a string variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionTextContent {
    Literal { text: String },
    Variable { variable: String },
}

impl CompositionComponent {
    pub fn id(&self) -> &str {
        match self {
            Self::Input { id, .. }
            | Self::Text { id, .. }
            | Self::Metric { id, .. }
            | Self::Table { id, .. } => id,
        }
    }

    /// The referenced source, if the component reads from one.
    pub fn source_id(&self) -> Option<&str> {
        match self {
            Self::Input { .. } | Self::Text { .. } => None,
            Self::Metric { source, .. } | Self::Table { source, .. } => Some(source),
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Input { label, .. }
            | Self::Text { label, .. }
            | Self::Metric { label, .. }
            | Self::Table { label, .. } => label.as_deref(),
        }
    }

    pub(crate) fn metric_shape_is_well_formed(&self) -> bool {
        let Self::Metric {
            source,
            column,
            presentation,
            ..
        } = self
        else {
            return true;
        };

        !source.trim().is_empty()
            && !column.trim().is_empty()
            && presentation
                .as_ref()
                .is_none_or(CompositionMetricPresentation::is_well_formed)
    }
}

/// Fixed typography roles for text components. Markdown, HTML, CSS, and
/// arbitrary fonts, sizes, colors, or event handlers are not part of v1.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextStyle {
    Title,
    Heading,
    Body,
    Caption,
}

/// The first-class dashboard flow layout. Rows render top to bottom; items
/// within a row render left to right on desktop and wrap on narrow screens.
/// Pixel coordinates, CSS, and canvas state are never layout content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardFlowLayout {
    pub kind: FlowLayoutKind,
    pub rows: Vec<FlowRow>,
}

/// The dashboard layout vocabulary supported by format version 1.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowLayoutKind {
    Flow,
}

/// One stable layout row carrying ordered component items.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowRow {
    pub id: String,
    pub items: Vec<FlowItem>,
}

/// One layout placement. Inputs are ordinary components in the flow.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowItem {
    Component { component: String },
}

/// Stable semantic diagnostic identifiers shared by Rust surfaces.
///
/// UI wording belongs to adapters and is intentionally not part of this
/// domain type.
///
/// Missing, denied, or mismatched source descriptors are reported as
/// `SourceUnavailable` without source metadata; the vocabulary carries no
/// existence-revealing codes.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionDiagnosticCode {
    UnsupportedFormatVersion,
    InvalidComposition,
    VariableUnknown,
    VariableMissing,
    VariableTypeMismatch,
    SourceUnavailable,
    MissingField,
    FieldTypeChanged,
    SourceSchemaChanged,
    MetricResultNotScalar,
    MetricResultTypeMismatch,
    MetricScalarNoRows,
    MetricScalarMultipleRows,
    MetricResultColumnMissing,
    MetricResultColumnAmbiguous,
}

impl CompositionDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedFormatVersion => "unsupported_format_version",
            Self::InvalidComposition => "invalid_composition",
            Self::VariableUnknown => "variable_unknown",
            Self::VariableMissing => "variable_missing",
            Self::VariableTypeMismatch => "variable_type_mismatch",
            Self::SourceUnavailable => "source_unavailable",
            Self::MissingField => "missing_field",
            Self::FieldTypeChanged => "field_type_changed",
            Self::SourceSchemaChanged => "source_schema_changed",
            Self::MetricResultNotScalar => "metric_result_not_scalar",
            Self::MetricResultTypeMismatch => "metric_result_type_mismatch",
            Self::MetricScalarNoRows => "metric_scalar_no_rows",
            Self::MetricScalarMultipleRows => "metric_scalar_multiple_rows",
            Self::MetricResultColumnMissing => "metric_result_column_missing",
            Self::MetricResultColumnAmbiguous => "metric_result_column_ambiguous",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_composition_yaml, CompositionComponent, CompositionDiagnosticCode,
        CompositionDocument, CompositionFormat, CompositionKind, CompositionMetricPresentation,
        CompositionQueryOperator, CompositionResultFieldType, CompositionSortDirection,
        CompositionSource, CompositionSpec, CompositionValue, DashboardFlowLayout,
        EntryQueryProjectionTemplate, EntryQueryTemplate, FlowItem, FlowLayoutKind, FlowRow,
        TextStyle, COMPOSITION_FORMAT, COMPOSITION_FORMAT_VERSION, DEFAULT_COMPOSITION_PAGE_LIMIT,
    };

    const MONTHLY_EXPENSE: &str =
        include_str!("../tests/fixtures/composition/monthly-expense.ugcomp.yaml");

    fn layout_row(id: &str, items: Vec<FlowItem>) -> FlowRow {
        FlowRow {
            id: id.to_owned(),
            items,
        }
    }

    fn component_item(id: &str) -> FlowItem {
        FlowItem::Component {
            component: id.to_owned(),
        }
    }

    fn flow_layout(rows: Vec<FlowRow>) -> DashboardFlowLayout {
        DashboardFlowLayout {
            kind: FlowLayoutKind::Flow,
            rows,
        }
    }

    #[test]
    fn multiple_metrics_select_columns_from_one_saved_sql_source() {
        let canonical = super::canonicalize_composition_yaml(include_str!(
            "../tests/fixtures/composition/saved-sql-metric-columns.ugcomp.yaml"
        ))
        .expect("one Saved SQL source may feed multiple scalar Metrics");
        let CompositionSource::SavedSql {
            expected_result, ..
        } = &canonical.document.spec.sources[1]
        else {
            panic!("second source should be Saved SQL")
        };
        assert_eq!(expected_result.len(), 3);
        assert!(canonical.document.spec.components[..3].iter().all(|component| {
            matches!(component, CompositionComponent::Metric { source, .. } if source == "monthly_summary")
        }));
        assert!(matches!(
            &canonical.document.spec.components[0],
            CompositionComponent::Metric { column, presentation: Some(CompositionMetricPresentation::Number { .. }), .. }
                if column == "count"
        ));
        assert!(matches!(
            &canonical.document.spec.components[1],
            CompositionComponent::Metric { column, .. } if column == "total"
        ));
        assert!(matches!(
            &canonical.document.spec.components[2],
            CompositionComponent::Metric { column, .. } if column == "latest"
        ));
        assert!(matches!(
            canonical.document.spec.components[3],
            CompositionComponent::Table { .. }
        ));
    }

    #[test]
    fn components_follow_layout_row_and_item_order_independent_of_declarations() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.components.reverse();
        spec.layout = flow_layout(vec![
            layout_row(
                "controls",
                vec![
                    component_item("month_end_input"),
                    component_item("month_start_input"),
                ],
            ),
            layout_row("summary", vec![component_item("total")]),
            layout_row("detail", vec![component_item("transactions")]),
        ]);

        let component_ids: Vec<_> = spec
            .components_in_render_order()
            .unwrap()
            .into_iter()
            .map(CompositionComponent::id)
            .collect();

        assert_eq!(
            component_ids,
            [
                "month_end_input",
                "month_start_input",
                "total",
                "transactions"
            ]
        );
    }

    #[test]
    fn multiple_inputs_can_reference_one_variable() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.components.push(CompositionComponent::Input {
            id: "month_start_secondary".to_owned(),
            label: Some("Start date".to_owned()),
            variable: "month_start".to_owned(),
        });
        spec.layout.rows[0]
            .items
            .push(component_item("month_start_secondary"));

        assert!(spec.components_in_render_order().is_ok());
    }

    #[test]
    fn unknown_input_variable_is_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        let CompositionComponent::Input { variable, .. } = &mut spec.components[0] else {
            panic!("first component should be an Input")
        };
        *variable = "unknown".to_owned();

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn variable_text_requires_a_declared_string_variable() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables.push(super::CompositionVariable {
            id: "enabled".to_owned(),
            label: None,
            variable_type: super::CompositionVariableType::Boolean,
            required: false,
            default: None,
            format: None,
        });
        spec.components.push(CompositionComponent::Text {
            id: "enabled_text".to_owned(),
            label: None,
            content: super::CompositionTextContent::Variable {
                variable: "enabled".to_owned(),
            },
            style: TextStyle::Body,
        });
        spec.layout.rows[0]
            .items
            .push(component_item("enabled_text"));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn unknown_variable_source_references_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        let CompositionSource::EntryQuery { query, .. } = &mut spec.sources[0] else {
            panic!("first source should be an EntryQuery")
        };
        query.filters[0].value = CompositionValue::Variable(super::CompositionVariableReference {
            variable: "unknown".to_owned(),
        });

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_variable_ids_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables.push(spec.variables[0].clone());

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn required_variable_without_default_needs_a_placed_input() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.components
            .retain(|component| component.id() != "month_end_input");
        spec.layout.rows[0]
            .items
            .retain(|item| *item != component_item("month_end_input"));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn optional_or_defaulted_variable_does_not_require_an_input() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables[0].required = false;
        spec.components
            .retain(|component| component.id() != "month_start_input");
        spec.layout.rows[0]
            .items
            .retain(|item| *item != component_item("month_start_input"));
        assert!(spec.components_in_render_order().is_ok());

        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables[1].default =
            Some(serde_json::from_value(serde_json::json!("2026-11-01")).unwrap());
        spec.components
            .retain(|component| component.id() != "month_end_input");
        spec.layout.rows[0]
            .items
            .retain(|item| *item != component_item("month_end_input"));
        assert!(spec.components_in_render_order().is_ok());
    }

    #[test]
    fn variable_defaults_must_match_the_type_and_lexical_contract() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables[0].default =
            Some(serde_json::from_value(serde_json::json!("2026-02-30")).unwrap());
        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables[0].default = Some(serde_json::from_value(serde_json::json!(false)).unwrap());
        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn variable_types_enforce_bounded_locale_independent_values() {
        use super::{CompositionVariableType as Type, MAX_COMPOSITION_VARIABLE_STRING_BYTES};
        use serde_json::json;

        assert!(Type::String.accepts_value(&json!("")));
        assert!(
            Type::String.accepts_value(&json!("x".repeat(MAX_COMPOSITION_VARIABLE_STRING_BYTES)))
        );
        assert!(!Type::String
            .accepts_value(&json!("x".repeat(MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1))));
        assert!(!Type::String.accepts_value(&json!("bad\u{0000}control")));
        assert!(Type::String.accepts_value(&json!("line one\nline two")));

        assert!(Type::Boolean.accepts_value(&json!(false)));
        assert!(!Type::Boolean.accepts_value(&json!("false")));

        assert!(Type::Integer.accepts_value(&json!(9_007_199_254_740_991_i64)));
        assert!(Type::Integer.accepts_value(&json!(-9_007_199_254_740_991_i64)));
        assert!(!Type::Integer.accepts_value(&json!(9_007_199_254_740_992_i64)));
        assert!(!Type::Integer.accepts_value(&json!(1.0)));

        assert!(Type::Float.accepts_value(&json!(1.25)));
        assert!(!Type::Float.accepts_value(&json!("1.25")));

        assert!(Type::Date.accepts_value(&json!("2024-02-29")));
        assert!(!Type::Date.accepts_value(&json!("2025-02-29")));
        assert!(!Type::Date.accepts_value(&json!("2026-1-01")));
        assert!(!Type::Date.accepts_value(&json!("0000-01-01")));

        assert!(Type::Timestamp.accepts_value(&json!("2026-10-13T23:59:59.123456789+09:00")));
        assert!(Type::Timestamp.accepts_value(&json!("2026-10-13T14:59:59Z")));
        for invalid in [
            "2026-10-13T14:59:59",
            "2026-10-13T14:59:60Z",
            "2026-10-13T14:59:59-00:00",
            "2026-10-13t14:59:59z",
            "2026-10-13T14:59:59.1234567890Z",
            "2026-10-13T25:59:59Z",
        ] {
            assert!(
                !Type::Timestamp.accepts_value(&json!(invalid)),
                "accepted {invalid}"
            );
        }
    }

    #[test]
    fn typed_input_parser_enforces_the_pinned_lexical_contract() {
        use super::{CompositionVariableInputValue as Parsed, CompositionVariableType as Type};

        assert_eq!(Type::String.parse_input(""), Parsed::Unset);
        assert_eq!(
            Type::String.parse_input("preserve\ttext\n"),
            Parsed::Valid(serde_json::json!("preserve\ttext\n"))
        );
        assert_eq!(Type::String.parse_input(&"é".repeat(2049)), Parsed::Invalid);

        assert_eq!(
            Type::Boolean.parse_input("true"),
            Parsed::Valid(serde_json::json!(true))
        );
        assert_eq!(
            Type::Boolean.parse_input("false"),
            Parsed::Valid(serde_json::json!(false))
        );
        for invalid in ["True", "FALSE", " true", "0", "yes"] {
            assert_eq!(Type::Boolean.parse_input(invalid), Parsed::Invalid);
        }

        assert_eq!(
            Type::Integer.parse_input("+00042"),
            Parsed::Valid(serde_json::json!(42))
        );
        assert_eq!(
            Type::Integer.parse_input("-9007199254740991"),
            Parsed::Valid(serde_json::json!(-9_007_199_254_740_991_i64))
        );
        assert_eq!(
            Type::Integer.parse_input("9007199254740991"),
            Parsed::Valid(serde_json::json!(9_007_199_254_740_991_i64))
        );
        for invalid in [
            "-",
            "+",
            " 1",
            "1 ",
            "1.0",
            "1e2",
            "9007199254740992",
            "123456789012345678",
        ] {
            assert_eq!(Type::Integer.parse_input(invalid), Parsed::Invalid);
        }

        for valid in ["1", "1.25", ".5", "+.5", "-1.0", "1e3", "5e-324", "0e-9999"] {
            assert!(
                matches!(Type::Float.parse_input(valid), Parsed::Valid(_)),
                "rejected {valid}"
            );
        }
        assert_eq!(Type::Float.parse_input(""), Parsed::Unset);
        for invalid in [
            "1.", "1,25", "NaN", "Infinity", "1e309", "1e-324", " 1", "1 ",
        ] {
            assert_eq!(
                Type::Float.parse_input(invalid),
                Parsed::Invalid,
                "accepted {invalid}"
            );
        }
        let max_float_token = format!("1{}", "0".repeat(63));
        assert!(matches!(
            Type::Float.parse_input(&max_float_token),
            Parsed::Valid(_)
        ));
        assert_eq!(
            Type::Float.parse_input(&(max_float_token + "0")),
            Parsed::Invalid
        );

        assert_eq!(Type::Date.parse_input(""), Parsed::Unset);
        assert!(matches!(
            Type::Date.parse_input("2024-02-29"),
            Parsed::Valid(_)
        ));
        assert_eq!(Type::Date.parse_input("2025-02-29"), Parsed::Invalid);
        assert!(matches!(
            Type::Timestamp.parse_input("2026-10-13T14:59:59.123456789+09:00"),
            Parsed::Valid(_)
        ));
        assert_eq!(
            Type::Timestamp.parse_input("2026-10-13T14:59:59"),
            Parsed::Invalid
        );
    }

    #[test]
    fn empty_layout_is_rejected() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows.clear();

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn empty_rows_are_rejected() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows.push(layout_row("empty", vec![]));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_row_ids_are_rejected() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        let duplicated = spec.layout.rows[0].clone();
        spec.layout.rows.push(duplicated);

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_component_ids_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.components[1] = spec.components[0].clone();

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_component_references_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0].items.push(component_item("total"));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout
            .rows
            .push(layout_row("duplicate", vec![component_item("total")]));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn unknown_component_references_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0].items[2] = component_item("unknown");

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn unreferenced_components_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0].items.pop();

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn unknown_layout_references_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0].items.push(component_item("unknown"));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_input_placements_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows.push(layout_row(
            "repeated",
            vec![component_item("month_start_input")],
        ));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn required_variable_without_default_needs_a_layout_input() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0]
            .items
            .retain(|item| *item != component_item("month_end_input"));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn optional_and_defaulted_variables_do_not_need_an_input() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables[0].required = false;
        spec.components
            .retain(|component| component.id() != "month_start_input");
        spec.layout.rows[0]
            .items
            .retain(|item| *item != component_item("month_start_input"));

        assert!(spec.components_in_render_order().is_ok());

        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.variables[1].default =
            Some(serde_json::from_value(serde_json::json!("2026-11-01")).unwrap());
        spec.components
            .retain(|component| component.id() != "month_end_input");
        spec.layout.rows[0]
            .items
            .retain(|item| *item != component_item("month_end_input"));

        assert!(spec.components_in_render_order().is_ok());
    }

    #[test]
    fn text_style_enum_values_round_trip() {
        for (style, spelling) in [
            (TextStyle::Title, "\"title\""),
            (TextStyle::Heading, "\"heading\""),
            (TextStyle::Body, "\"body\""),
            (TextStyle::Caption, "\"caption\""),
        ] {
            assert_eq!(serde_json::to_string(&style).unwrap(), spelling);
            assert_eq!(serde_json::from_str::<TextStyle>(spelling).unwrap(), style);
        }

        let component = CompositionComponent::Text {
            id: "heading".to_string(),
            label: None,
            content: super::CompositionTextContent::Literal {
                text: "Summary".to_string(),
            },
            style: TextStyle::Heading,
        };
        assert_eq!(component.id(), "heading");
        assert_eq!(component.source_id(), None);
        assert_eq!(
            serde_json::to_value(&component).unwrap(),
            serde_json::json!({
                "kind": "text",
                "id": "heading",
                "content": {"kind": "literal", "text": "Summary"},
                "style": "heading",
            })
        );
    }

    #[test]
    fn unknown_text_styles_and_layout_kinds_are_rejected() {
        assert!(serde_json::from_str::<TextStyle>("\"banner\"").is_err());
        let unknown_style = MONTHLY_EXPENSE.replacen(
            "  components:",
            "  components:\n    - id: note\n      kind: text\n      text: Note\n      style: banner",
            1,
        );
        assert_eq!(
            parse_composition_yaml(&unknown_style),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let unknown_layout_kind = MONTHLY_EXPENSE.replace("  kind: flow", "  kind: grid");
        assert_eq!(
            parse_composition_yaml(&unknown_layout_kind),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn document_uses_the_portable_envelope_field_names() {
        let document: CompositionDocument = serde_json::from_str(
            r#"{"format":"ugoite.composition","format_version":1,"kind":"dashboard","name":"Example","tags":["demo"],"spec":{"variables":[],"sources":[],"components":[],"layout":{"kind":"flow","rows":[]}}}"#,
        )
        .unwrap();

        assert_eq!(document.format, CompositionFormat::UgoiteComposition);
        assert_eq!(document.format_version, 1);
        assert_eq!(document.kind, CompositionKind::Dashboard);
        assert_eq!(document.name, "Example");
        assert_eq!(document.tags, ["demo"]);
        assert_eq!(
            document.spec,
            CompositionSpec {
                variables: vec![],
                sources: vec![],
                components: vec![],
                layout: DashboardFlowLayout {
                    kind: FlowLayoutKind::Flow,
                    rows: vec![],
                },
            }
        );
        assert_eq!(serde_json::to_value(document).unwrap()["kind"], "dashboard");
    }

    #[test]
    fn unknown_model_fields_are_rejected() {
        let result = serde_json::from_str::<CompositionDocument>(
            r#"{"format":"ugoite.composition","format_version":1,"kind":"dashboard","name":"Example","tags":[],"spec":{"variables":[],"sources":[],"components":[],"layout":{"kind":"flow","rows":[]}},"extra":true}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn query_template_has_a_stable_page_limit_default() {
        let template: EntryQueryTemplate =
            serde_json::from_str(r#"{"filters":[],"sort":[],"projection":{"kind":"preview"}}"#)
                .unwrap();
        assert_eq!(template.page_limit, DEFAULT_COMPOSITION_PAGE_LIMIT);
    }

    #[test]
    fn entry_query_field_schema_preserves_typed_list_and_reference_metadata() {
        let yaml = MONTHLY_EXPENSE.replace(
            "        - field_id: 102\n          field_type: double",
            "        - field_id: 102\n          field_type: list\n          items:\n            type: string\n        - field_id: 103\n          field_type: row_reference\n          reference_form: \"00000000-0000-7000-8000-000000000010\"",
        );
        let document = parse_composition_yaml(&yaml).expect("typed EntryQuery snapshot parses");
        let CompositionSource::EntryQuery { field_schema, .. } = &document.spec.sources[0] else {
            panic!("expected EntryQuery source");
        };

        assert_eq!(field_schema[2].field_type, super::FieldType::List);
        assert_eq!(
            field_schema[2].list_item,
            Some(super::ListItemDefinition {
                field_type: super::FieldType::String,
                reference_form: None,
            })
        );
        assert_eq!(field_schema[3].field_type, super::FieldType::RowReference);
        assert_eq!(
            field_schema[3].reference_form,
            Some(super::FormId::from_uuid(
                uuid::Uuid::parse_str("00000000-0000-7000-8000-000000000010").unwrap()
            ))
        );
    }

    #[test]
    fn dashboard_model_carries_exact_sources_and_query_templates() {
        let document: CompositionDocument = serde_json::from_value(serde_json::json!({
            "format": "ugoite.composition",
            "format_version": 1,
            "kind": "dashboard",
            "name": "Monthly expenses",
            "tags": [],
            "spec": {
                "variables": [
                    {"id": "search", "type": "string", "required": false},
                    {"id": "month", "type": "date", "required": true, "format": "year-month"}
                ],
                "sources": [
                    {
                        "kind": "entry_query",
                        "id": "expense_rows",
                        "form_id": "00000000-0000-7000-8000-000000000010",
                        "field_schema": [{"field_id": 100, "field_type": "string"}],
                        "query": {
                            "text": {"variable": "search"},
                            "filters": [{"field_id": 100, "operator": "contains", "value": "rent"}],
                            "sort": [{"field_id": 100, "direction": "asc"}],
                            "projection": {"kind": "fields", "fields": [100]}
                        }
                    },
                    {
                        "kind": "saved_sql",
                        "id": "monthly_total",
                        "entry_id": "00000000-0000-7000-8000-000000000020",
                        "revision_id": "00000000-0000-7000-8000-000000000021",
                        "expected_result": [{"name": "total", "type": "float"}],
                        "variables": {"month": {"variable": "month"}}
                    }
                ],
                "components": [
                    {"kind": "metric", "id": "total", "source": "monthly_total", "column": "total"},
                    {"kind": "table", "id": "transactions", "source": "expense_rows"}
                ],
                "layout": {
                    "kind": "flow",
                    "rows": [
                        {"id": "main", "items": [{"kind": "component", "component": "total"}, {"kind": "component", "component": "transactions"}]}
                    ]
                }
            }
        }))
        .unwrap();

        assert_eq!(document.spec.sources[0].id(), "expense_rows");
        let CompositionSource::EntryQuery { query, .. } = &document.spec.sources[0] else {
            panic!("first source should be an EntryQuery")
        };
        assert_eq!(query.page_limit, DEFAULT_COMPOSITION_PAGE_LIMIT);
        assert!(matches!(
            &query.text,
            Some(CompositionValue::Variable(reference)) if reference.variable == "search"
        ));
        assert_eq!(document.spec.sources[1].id(), "monthly_total");
        assert!(matches!(
            &document.spec.components[0],
            CompositionComponent::Metric { source, column, .. }
                if source == "monthly_total" && column == "total"
        ));
    }

    #[test]
    fn variable_reference_and_literal_values_reject_unknown_mapping_keys() {
        assert!(
            serde_json::from_str::<CompositionValue>(r#"{"variable":"month","extra":true}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<CompositionValue>(r#"{"parameter":"month"}"#).is_err());
        assert!(serde_json::from_str::<CompositionValue>(r#"{"unrecognized":"value"}"#).is_err());
        assert!(serde_json::from_str::<CompositionValue>(r#"["not", "scalar"]"#).is_err());
    }

    #[test]
    fn diagnostic_codes_have_stable_machine_spellings() {
        let codes = [
            (
                CompositionDiagnosticCode::UnsupportedFormatVersion,
                "unsupported_format_version",
            ),
            (
                CompositionDiagnosticCode::InvalidComposition,
                "invalid_composition",
            ),
            (
                CompositionDiagnosticCode::VariableUnknown,
                "variable_unknown",
            ),
            (
                CompositionDiagnosticCode::VariableMissing,
                "variable_missing",
            ),
            (
                CompositionDiagnosticCode::VariableTypeMismatch,
                "variable_type_mismatch",
            ),
            (
                CompositionDiagnosticCode::SourceUnavailable,
                "source_unavailable",
            ),
            (CompositionDiagnosticCode::MissingField, "missing_field"),
            (
                CompositionDiagnosticCode::FieldTypeChanged,
                "field_type_changed",
            ),
            (
                CompositionDiagnosticCode::SourceSchemaChanged,
                "source_schema_changed",
            ),
            (
                CompositionDiagnosticCode::MetricScalarNoRows,
                "metric_scalar_no_rows",
            ),
            (
                CompositionDiagnosticCode::MetricScalarMultipleRows,
                "metric_scalar_multiple_rows",
            ),
            (
                CompositionDiagnosticCode::MetricResultColumnMissing,
                "metric_result_column_missing",
            ),
            (
                CompositionDiagnosticCode::MetricResultColumnAmbiguous,
                "metric_result_column_ambiguous",
            ),
        ];

        for (code, spelling) in codes {
            assert_eq!(code.as_str(), spelling);
            let serialized = serde_json::to_string(&code).unwrap();
            assert_eq!(serialized, format!("\"{spelling}\""));
            assert_eq!(
                serde_json::from_str::<CompositionDiagnosticCode>(&serialized).unwrap(),
                code
            );
        }
    }

    /// Composition v1 implementation-contract freeze.
    ///
    /// This test pins the portable values frozen for v1. Any change here is
    /// a v1 compatibility decision, not a refactor: update the contract
    /// documents and the Mitase feature status alongside the code, and record
    /// the ruling. Additions to wire-visible sets remain possible only
    /// through that same ruling; this test guards the frozen values against
    /// silent drift, not against ruled extensions.
    #[test]
    fn composition_v1_freeze_pins_portable_contract() {
        use super::yaml::{
            MAX_COMPOSITION_COLLECTION_ITEMS, MAX_COMPOSITION_YAML_BYTES,
            MAX_COMPOSITION_YAML_DEPTH,
        };

        // Canonical envelope: fixed marker, version 1, dashboard only.
        assert_eq!(COMPOSITION_FORMAT, "ugoite.composition");
        assert_eq!(COMPOSITION_FORMAT_VERSION, 1);
        assert_eq!(
            serde_json::to_value(CompositionKind::Dashboard).unwrap(),
            serde_json::json!("dashboard")
        );

        // Restricted YAML resource limits.
        assert_eq!(MAX_COMPOSITION_YAML_BYTES, 64 * 1024);
        assert_eq!(MAX_COMPOSITION_YAML_DEPTH, 64);
        assert_eq!(MAX_COMPOSITION_COLLECTION_ITEMS, 256);

        // Bounded page size shared by Composition query templates.
        assert_eq!(DEFAULT_COMPOSITION_PAGE_LIMIT, 100);

        // Caller-visible diagnostic vocabulary: the exact frozen wire set in
        // canonical order. Existence-revealing codes stay out.
        let frozen_codes = [
            "unsupported_format_version",
            "invalid_composition",
            "variable_unknown",
            "variable_missing",
            "variable_type_mismatch",
            "source_unavailable",
            "missing_field",
            "field_type_changed",
            "source_schema_changed",
            "metric_result_not_scalar",
            "metric_result_type_mismatch",
            "metric_scalar_no_rows",
            "metric_scalar_multiple_rows",
            "metric_result_column_missing",
            "metric_result_column_ambiguous",
        ];
        assert_eq!(frozen_codes.len(), 15);
        for spelling in frozen_codes {
            let code: CompositionDiagnosticCode =
                serde_json::from_str(&format!("\"{spelling}\"")).unwrap();
            assert_eq!(code.as_str(), spelling);
        }

        // Source grammar: entry_query / saved_sql sources, six filter
        // operators, two sort directions, two projection kinds.
        for (operator, spelling) in [
            (CompositionQueryOperator::Equals, "\"equals\""),
            (CompositionQueryOperator::Contains, "\"contains\""),
            (CompositionQueryOperator::Lt, "\"lt\""),
            (CompositionQueryOperator::Lte, "\"lte\""),
            (CompositionQueryOperator::Gt, "\"gt\""),
            (CompositionQueryOperator::Gte, "\"gte\""),
        ] {
            assert_eq!(serde_json::to_string(&operator).unwrap(), spelling);
        }
        for (direction, spelling) in [
            (CompositionSortDirection::Asc, "\"asc\""),
            (CompositionSortDirection::Desc, "\"desc\""),
        ] {
            assert_eq!(serde_json::to_string(&direction).unwrap(), spelling);
        }

        // Projection kinds: whole-row preview or explicit field lists.
        assert_eq!(
            serde_json::to_value(EntryQueryProjectionTemplate::Preview).unwrap(),
            serde_json::json!({"kind": "preview"})
        );
        assert_eq!(
            serde_json::to_value(EntryQueryProjectionTemplate::Fields { fields: Vec::new() })
                .unwrap(),
            serde_json::json!({"kind": "fields", "fields": []})
        );

        // Portable Saved SQL logical types: the exact frozen set.
        for spelling in [
            "\"string\"",
            "\"boolean\"",
            "\"integer\"",
            "\"float\"",
            "\"date\"",
            "\"timestamp\"",
            "\"json\"",
        ] {
            let result_type: CompositionResultFieldType = serde_json::from_str(spelling).unwrap();
            assert_eq!(serde_json::to_string(&result_type).unwrap(), spelling);
        }

        // Metrics carry a Saved SQL source and selected scalar result column.
        let metric = CompositionComponent::Metric {
            id: "total".to_string(),
            label: None,
            source: "monthly_total".to_string(),
            column: "total".to_string(),
            presentation: None,
        };
        assert_eq!(
            serde_json::to_value(&metric).unwrap(),
            serde_json::json!({
                "kind": "metric",
                "id": "total",
                "source": "monthly_total",
                "column": "total"
            })
        );

        // Dashboard flow layout (ADR-019 re-freeze): flow is the only layout
        // kind; rows carry stable IDs with ordered component
        // items. Components are text, metric, or table; text styles are the
        // fixed title, heading, body, and caption roles.
        assert_eq!(
            serde_json::to_value(FlowLayoutKind::Flow).unwrap(),
            serde_json::json!("flow")
        );
        assert_eq!(
            serde_json::to_value(FlowItem::Component {
                component: "total".to_string(),
            })
            .unwrap(),
            serde_json::json!({"kind": "component", "component": "total"})
        );
        for (style, spelling) in [
            (TextStyle::Title, "\"title\""),
            (TextStyle::Heading, "\"heading\""),
            (TextStyle::Body, "\"body\""),
            (TextStyle::Caption, "\"caption\""),
        ] {
            assert_eq!(serde_json::to_string(&style).unwrap(), spelling);
        }
        let text_component = CompositionComponent::Text {
            id: "heading".to_string(),
            label: None,
            content: super::CompositionTextContent::Literal {
                text: "Summary".to_string(),
            },
            style: TextStyle::Heading,
        };
        assert_eq!(
            serde_json::to_value(&text_component).unwrap()["kind"],
            serde_json::json!("text")
        );
    }

    #[test]
    fn entry_query_display_timestamps_are_optional_and_old_preview_documents_read() {
        let old_query: EntryQueryTemplate = serde_json::from_value(serde_json::json!({
            "filters": [],
            "sort": [],
            "page_limit": 100,
            "projection": {"kind": "preview"}
        }))
        .unwrap();
        assert!(old_query.display_system_fields.is_empty());
        assert_eq!(
            serde_json::to_value(&old_query).unwrap()["projection"],
            serde_json::json!({"kind": "preview"})
        );
        assert!(serde_json::to_value(old_query)
            .unwrap()
            .get("display_system_fields")
            .is_none());

        let selected: EntryQueryTemplate = serde_json::from_value(serde_json::json!({
            "filters": [],
            "sort": [],
            "page_limit": 100,
            "projection": {"kind": "preview"},
            "display_system_fields": ["created_at", "updated_at"]
        }))
        .unwrap();
        assert_eq!(
            serde_json::to_value(selected).unwrap()["display_system_fields"],
            serde_json::json!(["created_at", "updated_at"])
        );
    }
}
