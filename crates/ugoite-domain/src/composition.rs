//! Typed Composition v1 domain contract.
//!
//! This module intentionally contains no YAML parser, storage, or query
//! execution code. It defines the shared Rust model that native and WASM
//! adapters use when parsing and resolving a Composition.

use crate::id::{EntryId, FormId, RevisionId};
use chrono::{DateTime, Datelike, NaiveDate};
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

mod canonical;
pub mod variable_graph;
pub mod variable_graph_input;
pub mod variable_invalidation;
mod yaml;

pub use variable_graph::{
    compile_variable_graph, CompiledVariableGraph, VariableGraph, VariableGraphArgumentBinding,
    VariableGraphColumn, VariableGraphDiagnostic, VariableGraphDiagnosticCode,
    VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind, VariableGraphQueryDefinition,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
};
pub use variable_graph_input::{
    resolve_variable_graph_inputs, ResolvedVariableGraphInputs, VariableGraphInputDiagnostic,
    VariableGraphInputDiagnosticCode, VariableGraphInputError,
};

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
/// A complete portable Composition document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDocument {
    pub format: CompositionFormat,
    pub format_version: u32,
    pub kind: CompositionKind,
    pub name: String,
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

    /// Whether every source has a server-confirmed result descriptor suitable
    /// for persistence. SQL drafts may have the exact empty descriptor tuple
    /// so Core can preview and populate it before a Composition save.
    pub fn has_complete_source_descriptors(&self) -> bool {
        self.spec
            .sources
            .iter()
            .all(CompositionSource::has_complete_source_descriptors)
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

        let mut sources_by_id = HashSet::with_capacity(self.sources.len());
        for source in &self.sources {
            if source.id().trim().is_empty()
                || !sources_by_id.insert(source.id())
                || !source.is_well_formed()
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
            if let CompositionSource::SavedSql { variables, .. } = source {
                if variables.iter().any(|(name, value)| {
                    name.trim().is_empty() || !value_uses_known_variable(value)
                }) {
                    return Err(CompositionDiagnosticCode::InvalidComposition);
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
                CompositionComponent::Table { source, .. }
                | CompositionComponent::Metric { source, .. } => {
                    if !sources_by_id.contains(source.as_str()) {
                        return Err(CompositionDiagnosticCode::InvalidComposition);
                    }
                }
                CompositionComponent::Text { .. } => {}
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
/// Maximum magnitude accepted for a typed Composition integer value.
///
/// Keeping integers within JavaScript's safe range preserves exact values
/// across portable JSON clients.
pub const MAX_COMPOSITION_VARIABLE_INTEGER: i64 = 9_007_199_254_740_991;

/// A typed value declared by a Composition and shared by its Inputs, Text,
/// and data Sources.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionVariable {
    pub id: String,
    #[serde(rename = "type")]
    pub variable_type: CompositionVariableType,
    pub required: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_variable_default"
    )]
    pub default: Option<CompositionLiteral>,
    /// A presentation hint that never changes the value's type or parsing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<CompositionVariableFormat>,
}

fn deserialize_variable_default<'de, D>(
    deserializer: D,
) -> Result<Option<CompositionLiteral>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = CompositionLiteral::deserialize(deserializer)?;
    if value.as_json_value().is_null() {
        return Err(D::Error::custom(
            "Composition variable defaults cannot be null",
        ));
    }
    Ok(Some(value))
}

impl CompositionVariable {
    fn is_well_formed(&self) -> bool {
        !self.id.trim().is_empty()
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
                (-MAX_COMPOSITION_VARIABLE_INTEGER..=MAX_COMPOSITION_VARIABLE_INTEGER)
                    .contains(&value)
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
    InlineSql {
        id: String,
        /// Editable semantic SQL object name, independent from the stable ID.
        name: String,
        sql: String,
        form_bindings: Vec<CompositionFormBinding>,
        expected_result: Vec<CompositionResultColumn>,
        schema_fingerprint: String,
    },
    SavedSql {
        id: String,
        entry_id: EntryId,
        revision_id: RevisionId,
        expected_result: Vec<CompositionResultColumn>,
        schema_fingerprint: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        variables: BTreeMap<String, CompositionValue>,
    },
}

impl CompositionSource {
    pub fn id(&self) -> &str {
        match self {
            Self::InlineSql { id, .. } | Self::SavedSql { id, .. } => id,
        }
    }

    fn is_well_formed(&self) -> bool {
        match self {
            Self::InlineSql {
                name,
                sql,
                form_bindings,
                expected_result,
                schema_fingerprint,
                ..
            } => {
                !name.trim().is_empty()
                    && name.len() <= MAX_COMPOSITION_YAML_BYTES
                    && !sql.trim().is_empty()
                    && sql.len() <= MAX_COMPOSITION_YAML_BYTES
                    && form_bindings
                        .iter()
                        .all(CompositionFormBinding::is_well_formed)
                    && unique_by(form_bindings, |binding| binding.relation.clone())
                    && (self.has_complete_source_descriptors()
                        || (form_bindings.is_empty()
                            && expected_result.is_empty()
                            && schema_fingerprint.is_empty()))
            }
            Self::SavedSql {
                expected_result,
                schema_fingerprint,
                ..
            } => {
                result_schema_is_well_formed(expected_result, schema_fingerprint)
                    || (expected_result.is_empty() && schema_fingerprint.is_empty())
            }
        }
    }

    pub fn expected_result(&self) -> &[CompositionResultColumn] {
        match self {
            Self::InlineSql {
                expected_result, ..
            }
            | Self::SavedSql {
                expected_result, ..
            } => expected_result,
        }
    }

    /// Whether this source has a complete server-derived result descriptor.
    /// An SQL draft is structurally parseable before this is true.
    pub fn has_complete_source_descriptors(&self) -> bool {
        match self {
            Self::InlineSql {
                form_bindings,
                expected_result,
                schema_fingerprint,
                ..
            } => {
                form_bindings
                    .iter()
                    .all(CompositionFormBinding::is_well_formed)
                    && unique_by(form_bindings, |binding| binding.relation.clone())
                    && result_schema_is_well_formed(expected_result, schema_fingerprint)
            }
            Self::SavedSql {
                expected_result,
                schema_fingerprint,
                ..
            } => result_schema_is_well_formed(expected_result, schema_fingerprint),
        }
    }
}

/// A server-derived binding from one SQL relation identifier to its stable
/// Form identity. It is stored in Composition Knowledge, never accepted as a
/// trusted binding on the general SQL query request.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionFormBinding {
    pub relation: String,
    pub form_id: FormId,
}

impl CompositionFormBinding {
    fn is_well_formed(&self) -> bool {
        !self.relation.trim().is_empty() && self.relation.len() <= MAX_COMPOSITION_YAML_BYTES
    }
}

fn result_schema_is_well_formed(
    expected_result: &[CompositionResultColumn],
    schema_fingerprint: &str,
) -> bool {
    !expected_result.is_empty()
        && expected_result.len() <= MAX_COMPOSITION_COLLECTION_ITEMS
        && expected_result.iter().all(|column| {
            !column.name.trim().is_empty() && column.name.len() <= MAX_COMPOSITION_YAML_BYTES
        })
        && unique_by(expected_result, |column| column.name.clone())
        && is_sha256_fingerprint(schema_fingerprint)
}

fn unique_by<T, K: Eq + std::hash::Hash>(items: &[T], key: impl Fn(&T) -> K) -> bool {
    let mut seen = HashSet::with_capacity(items.len());
    items.iter().all(|item| seen.insert(key(item)))
}

fn is_sha256_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// One ordered output column declared for a SQL Composition source.
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
        variable: String,
    },
    Text {
        id: String,
        content: CompositionTextContent,
        style: TextStyle,
    },
    Metric {
        id: String,
        /// SQL source whose result descriptor is resolved by Core.
        source: String,
        /// One scalar output column returned by the source.
        column: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presentation: Option<CompositionMetricPresentation>,
    },
    Table {
        id: String,
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
        parse_composition_yaml, CompositionComponent, CompositionDiagnosticCode, CompositionFormat,
        CompositionSource, CompositionTextContent, CompositionVariable,
        CompositionVariableInputValue, CompositionVariableType, FlowItem, FlowRow, TextStyle,
        MAX_COMPOSITION_COLLECTION_ITEMS,
    };
    use serde_json::json;

    const SQL_FIRST: &str = include_str!("../tests/fixtures/composition/sql-first.ugcomp.yaml");

    fn component_item(id: &str) -> FlowItem {
        FlowItem::Component {
            component: id.to_owned(),
        }
    }

    fn row(id: &str, components: &[&str]) -> FlowRow {
        FlowRow {
            id: id.to_owned(),
            items: components.iter().map(|id| component_item(id)).collect(),
        }
    }

    #[test]
    fn parses_the_sql_first_v1_fixture() {
        let document = parse_composition_yaml(SQL_FIRST).unwrap();
        assert_eq!(document.format, CompositionFormat::UgoiteComposition);
        assert_eq!(document.format_version, 1);
        assert_eq!(document.name, "Monthly expenses");
        assert_eq!(document.spec.variables.len(), 2);
        assert_eq!(document.spec.sources.len(), 2);
        assert_eq!(document.spec.components.len(), 4);
        assert_eq!(document.spec.layout.rows.len(), 1);

        let CompositionSource::InlineSql {
            name,
            sql,
            form_bindings,
            expected_result,
            schema_fingerprint,
            ..
        } = &document.spec.sources[0]
        else {
            panic!("first source should be Inline SQL");
        };
        assert_eq!(name, "Expense rows");
        assert!(sql.contains("FROM \"Expense\""));
        assert_eq!(form_bindings.len(), 1);
        assert_eq!(form_bindings[0].relation, "Expense");
        assert_eq!(expected_result.len(), 3);
        assert_eq!(schema_fingerprint.len(), 64);

        let CompositionSource::SavedSql {
            entry_id,
            revision_id,
            expected_result,
            schema_fingerprint,
            variables,
            ..
        } = &document.spec.sources[1]
        else {
            panic!("second source should be Saved SQL");
        };
        assert_ne!(entry_id.to_string(), revision_id.to_string());
        assert_eq!(expected_result[0].name, "total");
        assert_eq!(schema_fingerprint.len(), 64);
        assert_eq!(variables.len(), 2);

        let ordered = document.spec.components_in_render_order().unwrap();
        assert_eq!(
            ordered
                .iter()
                .map(|component| component.id())
                .collect::<Vec<_>>(),
            [
                "month_start_input",
                "month_end_input",
                "total",
                "transactions",
            ]
        );
    }

    #[test]
    fn old_composition_v1_grammar_and_display_fields_fail_closed() {
        let old_source = SQL_FIRST.replace("kind: inline_sql", "kind: entry_query");
        assert_eq!(
            parse_composition_yaml(&old_source),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let root_tags = SQL_FIRST.replace(
            "name: Monthly expenses\n",
            "name: Monthly expenses\ntags: [finance]\n",
        );
        assert_eq!(
            parse_composition_yaml(&root_tags),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let component_label = SQL_FIRST.replace(
            "      variable: month_start\n",
            "      variable: month_start\n      label: Start month\n",
        );
        assert_eq!(
            parse_composition_yaml(&component_label),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let variable_label = SQL_FIRST.replace(
            "    - id: month_start\n      type: date\n",
            "    - id: month_start\n      label: Start month\n      type: date\n",
        );
        assert_eq!(
            parse_composition_yaml(&variable_label),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
        let source_alias = SQL_FIRST.replace(
            "      kind: saved_sql\n",
            "      kind: saved_sql\n      label: Summary\n",
        );
        assert_eq!(
            parse_composition_yaml(&source_alias),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn source_shapes_require_descriptors_and_a_sha256_fingerprint() {
        for malformed in [
            SQL_FIRST.replace("schema_fingerprint: 99a98", "schema_fingerprint: 99A98"),
            SQL_FIRST.replace(
                "      sql: |-
",
                "      sql: ''\n",
            ),
            SQL_FIRST.replace("      expected_result:\n", "      expected_result: []\n"),
        ] {
            assert_eq!(
                parse_composition_yaml(&malformed),
                Err(CompositionDiagnosticCode::InvalidComposition)
            );
        }

        let duplicate_binding = SQL_FIRST.replace(
            "        - relation: Expense\n          form_id: \"00000000-0000-7000-8000-000000000010\"\n",
            "        - relation: Expense\n          form_id: \"00000000-0000-7000-8000-000000000010\"\n        - relation: Expense\n          form_id: \"00000000-0000-7000-8000-000000000010\"\n",
        );
        assert_eq!(
            parse_composition_yaml(&duplicate_binding),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn exact_empty_inline_descriptor_is_a_previewable_draft_only() {
        let incomplete_inline = SQL_FIRST.replace(
            "      form_bindings:\n        - relation: Expense\n          form_id: \"00000000-0000-7000-8000-000000000010\"\n      expected_result:\n        - name: Date\n          type: date\n        - name: category\n          type: string\n        - name: amount\n          type: decimal\n      schema_fingerprint: 99a980a869d3c01c212a4d175e0616b3237ddfc19b35a6f027829f7cadad3d67",
            "      form_bindings: []\n      expected_result: []\n      schema_fingerprint: ''",
        );
        let mut parsed = parse_composition_yaml(&incomplete_inline).unwrap();
        let CompositionComponent::Metric { source, column, .. } = &mut parsed.spec.components[2]
        else {
            panic!("fixture component should be a Metric");
        };
        *source = "expense_rows".to_owned();
        *column = "amount".to_owned();
        assert!(!parsed.has_complete_source_descriptors());
        let canonical = super::canonicalize_composition(&parsed).unwrap();
        assert!(!canonical.document.has_complete_source_descriptors());

        let partial_inline = incomplete_inline.replace(
            "      form_bindings: []",
            "      form_bindings:\n        - relation: Expense\n          form_id: \"00000000-0000-7000-8000-000000000010\"",
        );
        assert_eq!(
            parse_composition_yaml(&partial_inline),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn exact_empty_saved_sql_descriptor_is_a_previewable_draft_only() {
        let incomplete_saved_sql = SQL_FIRST.replace(
            "      expected_result:\n        - name: total\n          type: decimal\n      schema_fingerprint: f907f602a58c987ee0678cbdd199189fb05085e9e4d21dd05e8dbf64b63d87e1",
            "      expected_result: []\n      schema_fingerprint: ''",
        );
        let parsed = parse_composition_yaml(&incomplete_saved_sql).unwrap();
        assert!(!parsed.has_complete_source_descriptors());
        let canonical = super::canonicalize_composition(&parsed).unwrap();
        assert!(!canonical.document.has_complete_source_descriptors());

        let partial_saved_sql = incomplete_saved_sql.replace(
            "      expected_result: []\n      schema_fingerprint: ''",
            "      expected_result: []\n      schema_fingerprint: f907f602a58c987ee0678cbdd199189fb05085e9e4d21dd05e8dbf64b63d87e1",
        );
        assert_eq!(
            parse_composition_yaml(&partial_saved_sql),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_source_ids_and_unknown_source_references_are_invalid() {
        let duplicate_id = SQL_FIRST.replace(
            "    - id: month_total\n      kind: saved_sql",
            "    - id: expense_rows\n      kind: saved_sql",
        );
        assert_eq!(
            parse_composition_yaml(&duplicate_id),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let missing_source =
            SQL_FIRST.replace("      source: expense_rows\n", "      source: missing\n");
        assert_eq!(
            parse_composition_yaml(&missing_source),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn required_variables_need_inputs_unless_they_have_defaults() {
        let mut spec = parse_composition_yaml(SQL_FIRST).unwrap().spec;
        spec.components
            .retain(|component| component.id() != "month_end_input");
        spec.layout.rows[0]
            .items
            .retain(|item| *item != component_item("month_end_input"));
        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        spec.variables[1].default =
            Some(serde_json::from_value(json!("2026-11-01")).expect("scalar literal"));
        assert!(spec.components_in_render_order().is_ok());
    }

    #[test]
    fn text_variable_binding_requires_a_string_variable() {
        let mut spec = parse_composition_yaml(SQL_FIRST).unwrap().spec;
        spec.variables.push(CompositionVariable {
            id: "enabled".to_owned(),
            variable_type: CompositionVariableType::Boolean,
            required: false,
            default: None,
            format: None,
        });
        spec.components.push(CompositionComponent::Text {
            id: "enabled_text".to_owned(),
            content: CompositionTextContent::Variable {
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
    fn component_and_layout_identity_is_stable_and_unique() {
        let mut spec = parse_composition_yaml(SQL_FIRST).unwrap().spec;
        spec.layout.rows = vec![
            row("controls", &["month_end_input", "month_start_input"]),
            row("results", &["total", "transactions"]),
        ];
        assert_eq!(
            spec.components_in_render_order()
                .unwrap()
                .iter()
                .map(|component| component.id())
                .collect::<Vec<_>>(),
            [
                "month_end_input",
                "month_start_input",
                "total",
                "transactions"
            ]
        );

        spec.layout.rows[1].items.push(component_item("total"));
        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn canonicalization_rejects_empty_or_unreferenced_layouts() {
        let mut empty = parse_composition_yaml(SQL_FIRST).unwrap();
        empty.spec.layout.rows.clear();
        assert!(super::canonicalize_composition(&empty).is_err());

        let mut empty_row = parse_composition_yaml(SQL_FIRST).unwrap();
        empty_row.spec.layout.rows[0].items.clear();
        assert!(super::canonicalize_composition(&empty_row).is_err());

        let mut duplicate_row = parse_composition_yaml(SQL_FIRST).unwrap();
        duplicate_row
            .spec
            .layout
            .rows
            .push(duplicate_row.spec.layout.rows[0].clone());
        assert!(super::canonicalize_composition(&duplicate_row).is_err());

        let mut unreferenced = parse_composition_yaml(SQL_FIRST).unwrap();
        unreferenced.spec.components.pop();
        assert!(super::canonicalize_composition(&unreferenced).is_err());
    }

    #[test]
    fn metrics_accept_either_sql_source_when_the_selected_column_is_scalar() {
        let mut document = parse_composition_yaml(SQL_FIRST).unwrap();
        {
            let CompositionComponent::Metric { source, column, .. } =
                &mut document.spec.components[2]
            else {
                panic!("fixture component should be a Metric");
            };
            *source = "expense_rows".to_owned();
            *column = "amount".to_owned();
        }
        assert!(super::canonicalize_composition(&document).is_ok());

        {
            let CompositionComponent::Metric { source, column, .. } =
                &mut document.spec.components[2]
            else {
                panic!("fixture component should be a Metric");
            };
            *source = "month_total".to_owned();
            *column = "total".to_owned();
        }
        assert!(super::canonicalize_composition(&document).is_ok());
    }

    #[test]
    fn enforces_per_collection_item_limit_without_legacy_collections() {
        fn document_with_variables(count: usize) -> String {
            let mut input = String::from(
                "format: ugoite.composition\nformat_version: 1\nkind: dashboard\nname: Example\nspec:\n  variables:\n",
            );
            for index in 0..count {
                input.push_str(&format!(
                    "    - id: variable_{index}\n      type: string\n      required: false\n"
                ));
            }
            input.push_str(
                "  sources: []\n  components:\n    - id: marker\n      kind: text\n      content: {kind: literal, text: marker}\n      style: body\n  layout:\n    kind: flow\n    rows:\n      - id: main\n        items:\n          - kind: component\n            component: marker\n",
            );
            input
        }

        assert!(
            parse_composition_yaml(&document_with_variables(MAX_COMPOSITION_COLLECTION_ITEMS))
                .is_ok()
        );
        assert_eq!(
            parse_composition_yaml(&document_with_variables(
                MAX_COMPOSITION_COLLECTION_ITEMS + 1
            )),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn unknown_text_styles_and_layout_kinds_are_rejected() {
        let with_text = SQL_FIRST
            .replace(
                "  layout:\n",
                "    - id: title\n      kind: text\n      content:\n        kind: literal\n        text: Summary\n      style: heading\n  layout:\n",
            )
            .replace(
                "          - kind: component\n            component: month_start_input",
                "          - kind: component\n            component: title\n          - kind: component\n            component: month_start_input",
            );
        for invalid in [
            with_text.replace("style: heading", "style: marquee"),
            with_text.replace("kind: flow", "kind: grid"),
        ] {
            assert_eq!(
                parse_composition_yaml(&invalid),
                Err(CompositionDiagnosticCode::InvalidComposition)
            );
        }
    }

    #[test]
    fn variable_parser_preserves_typed_non_coercing_contract() {
        assert_eq!(
            CompositionVariableType::String.parse_input(""),
            CompositionVariableInputValue::Unset
        );
        assert_eq!(
            CompositionVariableType::Boolean.parse_input("true"),
            CompositionVariableInputValue::Valid(json!(true))
        );
        assert_eq!(
            CompositionVariableType::Boolean.parse_input("True"),
            CompositionVariableInputValue::Invalid
        );
        assert_eq!(
            CompositionVariableType::Integer.parse_input("+00042"),
            CompositionVariableInputValue::Valid(json!(42))
        );
        assert_eq!(
            CompositionVariableType::Integer.parse_input("1.0"),
            CompositionVariableInputValue::Invalid
        );
        assert!(matches!(
            CompositionVariableType::Float.parse_input("1.25"),
            CompositionVariableInputValue::Valid(_)
        ));
        assert_eq!(
            CompositionVariableType::Float.parse_input("1e309"),
            CompositionVariableInputValue::Invalid
        );
        assert!(matches!(
            CompositionVariableType::Date.parse_input("2024-02-29"),
            CompositionVariableInputValue::Valid(_)
        ));
        assert_eq!(
            CompositionVariableType::Date.parse_input("2025-02-29"),
            CompositionVariableInputValue::Invalid
        );
        assert!(matches!(
            CompositionVariableType::Timestamp.parse_input("2026-10-13T14:59:59Z"),
            CompositionVariableInputValue::Valid(_)
        ));
        assert_eq!(
            CompositionVariableType::Timestamp.parse_input("2026-10-13T14:59:59"),
            CompositionVariableInputValue::Invalid
        );
    }

    #[test]
    fn variable_types_enforce_bounded_locale_independent_values() {
        use super::{CompositionVariableType as Type, MAX_COMPOSITION_VARIABLE_STRING_BYTES};

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
        assert!(Type::Float.accepts_value(&json!(1.0)));
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
        assert!(CompositionVariableType::Float.accepts_value(&json!(1)));
        assert!(!CompositionVariableType::Integer.accepts_value(&json!(1.0)));
    }

    #[test]
    fn typed_input_parser_enforces_the_pinned_lexical_contract() {
        use CompositionVariableInputValue as Parsed;
        use CompositionVariableType as Type;

        assert_eq!(Type::String.parse_input(""), Parsed::Unset);
        assert_eq!(
            Type::String.parse_input("preserve\ttext\n"),
            Parsed::Valid(json!("preserve\ttext\n"))
        );
        assert_eq!(Type::String.parse_input(&"é".repeat(2049)), Parsed::Invalid);
        assert_eq!(
            Type::Boolean.parse_input("true"),
            Parsed::Valid(json!(true))
        );
        assert_eq!(
            Type::Boolean.parse_input("false"),
            Parsed::Valid(json!(false))
        );
        for invalid in ["True", "FALSE", " true", "0", "yes"] {
            assert_eq!(Type::Boolean.parse_input(invalid), Parsed::Invalid);
        }

        assert_eq!(
            Type::Integer.parse_input("+00042"),
            Parsed::Valid(json!(42))
        );
        assert_eq!(
            Type::Integer.parse_input("+0000000000000001"),
            Parsed::Valid(json!(1))
        );
        assert_eq!(
            Type::Integer.parse_input("-9007199254740991"),
            Parsed::Valid(json!(-9_007_199_254_740_991_i64))
        );
        assert_eq!(
            Type::Integer.parse_input("9007199254740991"),
            Parsed::Valid(json!(9_007_199_254_740_991_i64))
        );
        for invalid in [
            "-",
            "+",
            " 1",
            "1 ",
            "1.0",
            "1e2",
            "9007199254740992",
            "+00000000000000001",
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
    fn empty_null_and_mistyped_variable_defaults_are_rejected() {
        let null_default = SQL_FIRST.replace(
            "      required: true\n      format: year-month",
            "      required: false\n      default: null\n      format: year-month",
        );
        assert_eq!(
            super::canonicalize_composition_yaml(&null_default),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let wrong_type = SQL_FIRST.replace(
            "      type: date\n      required: true",
            "      type: boolean\n      required: true",
        );
        assert_eq!(
            super::canonicalize_composition_yaml(&wrong_type),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }
}
