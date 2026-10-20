//! Typed Composition v1 domain contract.
//!
//! This module intentionally contains no YAML parser, storage, or query
//! execution code. It defines the shared Rust model that native and WASM
//! adapters use when parsing and resolving a Composition.

use chrono::{DateTime, Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

mod canonical;
pub mod variable_binding;
pub mod variable_graph;
pub mod variable_graph_input;
pub mod variable_invalidation;
mod yaml;

use variable_binding::{compile_variable_graph_bindings, VariableGraphComponentBinding};

pub use variable_graph::{
    compile_variable_graph, validate_variable_graph_result, variable_graph_dependency_order,
    CompiledVariableGraph, VariableGraph, VariableGraphArgumentBinding, VariableGraphColumn,
    VariableGraphDiagnostic, VariableGraphDiagnosticCode, VariableGraphFormBinding,
    VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind, VariableGraphQuery,
    VariableGraphQueryDefinition, VariableGraphResultDiagnostic, VariableGraphResultDiagnosticCode,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
    VariableGraphValidatedResult,
};
pub use variable_graph_input::{
    parse_variable_graph_input, resolve_variable_graph_inputs, ResolvedVariableGraphInputs,
    VariableGraphInputDiagnostic, VariableGraphInputDiagnosticCode, VariableGraphInputError,
    VariableGraphInputValue,
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
    /// version. Structural and graph-reference checks are performed by the
    /// parser and resolver that consume this model.
    pub fn validate_format_version(&self) -> Result<(), CompositionDiagnosticCode> {
        if self.format_version == COMPOSITION_FORMAT_VERSION {
            Ok(())
        } else {
            Err(CompositionDiagnosticCode::UnsupportedFormatVersion)
        }
    }

    /// Whether every query has a server-confirmed result and query descriptor
    /// suitable for persistence. Query nodes carry the descriptor derived by Core.
    pub fn has_complete_query_descriptors(&self) -> bool {
        self.spec.variables.iter().all(|node| match &node.kind {
            VariableGraphNodeKind::Query { returns, query } => {
                variable_graph_result_is_complete(returns)
                    && !query.schema_fingerprint.is_empty()
                    && query.schema_fingerprint.len() == 64
                    && query
                        .schema_fingerprint
                        .bytes()
                        .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                    && query
                        .form_bindings
                        .iter()
                        .all(|binding| !binding.relation.trim().is_empty())
                    && query
                        .form_bindings
                        .iter()
                        .map(|binding| binding.relation.as_str())
                        .collect::<HashSet<_>>()
                        .len()
                        == query.form_bindings.len()
            }
            VariableGraphNodeKind::Constant { .. } | VariableGraphNodeKind::Input { .. } => true,
        })
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
    pub variables: Vec<VariableGraphNode>,
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
        compile_variable_graph(&VariableGraph {
            variables: self.variables.clone(),
        })
        .map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;

        let graph_components = self
            .components
            .iter()
            .map(|component| match component {
                CompositionComponent::Input { id, variable } => {
                    VariableGraphComponentBinding::Input {
                        id: id.clone(),
                        variable: variable.clone(),
                    }
                }
                CompositionComponent::Text { id, content, .. } => {
                    VariableGraphComponentBinding::Text {
                        id: id.clone(),
                        variable: match content {
                            CompositionTextContent::Literal { .. } => None,
                            CompositionTextContent::Variable { variable } => Some(variable.clone()),
                        },
                    }
                }
                CompositionComponent::Metric { id, variable, .. } => {
                    VariableGraphComponentBinding::Metric {
                        id: id.clone(),
                        variable: variable.clone(),
                    }
                }
                CompositionComponent::Table { id, variable } => {
                    VariableGraphComponentBinding::Table {
                        id: id.clone(),
                        variable: variable.clone(),
                    }
                }
            })
            .collect::<Vec<_>>();

        let mut components_by_id = HashMap::with_capacity(self.components.len());
        for component in &self.components {
            if component.id().trim().is_empty()
                || components_by_id.insert(component.id(), component).is_some()
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
            match component {
                CompositionComponent::Input { .. }
                | CompositionComponent::Text { .. }
                | CompositionComponent::Table { .. }
                | CompositionComponent::Metric { .. } => {}
            }
        }

        compile_variable_graph_bindings(
            &VariableGraph {
                variables: self.variables.clone(),
            },
            &graph_components,
            &self
                .layout
                .rows
                .iter()
                .flat_map(|row| row.items.iter())
                .map(|item| match item {
                    FlowItem::Component { component } => component.clone(),
                })
                .collect::<Vec<_>>(),
        )
        .map_err(|_| CompositionDiagnosticCode::InvalidComposition)?;

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
/// Maximum UTF-8 byte length accepted for a numeric Composition input token.
///
/// This bounds lexical parsing consistently across local CLI, browser, and
/// server supplied typed values, including finite floating-point values that
/// would otherwise accept arbitrarily long zero-padded tokens.
pub const MAX_COMPOSITION_VARIABLE_NUMERIC_TOKEN_BYTES: usize = 64;

fn valid_variable_string(value: &str) -> bool {
    value.len() <= MAX_COMPOSITION_VARIABLE_STRING_BYTES
        && value
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
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

fn variable_graph_result_is_complete(result: &VariableGraphResultType) -> bool {
    match result {
        VariableGraphResultType::Scalar { .. } => true,
        VariableGraphResultType::Table { columns } => {
            !columns.is_empty()
                && columns.len() <= MAX_COMPOSITION_COLLECTION_ITEMS
                && columns.iter().all(|column| !column.name.trim().is_empty())
                && unique_by(columns, |column| column.name.clone())
        }
    }
}

fn unique_by<T, K: Eq + std::hash::Hash>(items: &[T], key: impl Fn(&T) -> K) -> bool {
    let mut seen = HashSet::with_capacity(items.len());
    items.iter().all(|item| seen.insert(key(item)))
}

/// Portable logical type used when returning one metric value.
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
        /// Scalar value supplied by a constant, input, or query variable.
        variable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presentation: Option<CompositionMetricPresentation>,
    },
    Table {
        id: String,
        /// Table value supplied by a query variable.
        variable: String,
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

    /// The referenced variable, if the component reads one.
    pub fn variable_id(&self) -> Option<&str> {
        match self {
            Self::Input { variable, .. }
            | Self::Metric { variable, .. }
            | Self::Table { variable, .. } => Some(variable),
            Self::Text {
                content: CompositionTextContent::Variable { variable },
                ..
            } => Some(variable),
            Self::Text {
                content: CompositionTextContent::Literal { .. },
                ..
            } => None,
        }
    }

    pub(crate) fn metric_shape_is_well_formed(&self) -> bool {
        let Self::Metric { presentation, .. } = self else {
            return true;
        };

        presentation
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
/// Missing, denied, or mismatched query descriptors are reported as
/// `QueryUnavailable` without query metadata; the vocabulary carries no
/// existence-revealing codes.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionDiagnosticCode {
    UnsupportedFormatVersion,
    InvalidComposition,
    VariableUnknown,
    VariableMissing,
    VariableTypeMismatch,
    QueryUnavailable,
    MissingField,
    FieldTypeChanged,
    QuerySchemaChanged,
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
            Self::QueryUnavailable => "query_unavailable",
            Self::MissingField => "missing_field",
            Self::FieldTypeChanged => "field_type_changed",
            Self::QuerySchemaChanged => "query_schema_changed",
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
        CompositionMetricPresentation, VariableGraphNodeKind, VariableGraphResultType,
        VariableGraphScalarDescriptor, VariableGraphScalarType,
    };

    const GRAPH: &str = include_str!("../tests/fixtures/composition/sql-first.ugcomp.yaml");

    #[test]
    fn variable_graph_fixture_has_deterministic_component_order() {
        let document = parse_composition_yaml(GRAPH).unwrap();
        assert_eq!(document.format, super::CompositionFormat::UgoiteComposition);
        assert_eq!(document.format_version, 1);
        assert_eq!(document.name, "Monthly expenses");
        assert_eq!(document.spec.variables.len(), 4);
        assert_eq!(document.spec.components.len(), 4);
        assert_eq!(
            document
                .spec
                .components_in_render_order()
                .unwrap()
                .iter()
                .map(|component| component.id())
                .collect::<Vec<_>>(),
            [
                "month_start_input",
                "month_end_input",
                "total",
                "transactions"
            ]
        );
    }

    #[test]
    fn replaced_composition_shape_is_rejected_without_hidden_fields() {
        for (field, invalid) in [
            (
                "spec.sources",
                GRAPH.replace("  variables:\n", "  sources: []\n  variables:\n"),
            ),
            (
                "component.source",
                GRAPH.replace(
                    "      variable: expense_rows\n",
                    "      source: expense_rows\n",
                ),
            ),
            (
                "root tags",
                GRAPH.replace(
                    "name: Monthly expenses\n",
                    "name: Monthly expenses\ntags: []\n",
                ),
            ),
            (
                "variable label",
                GRAPH.replace(
                    "    - id: month_start\n",
                    "    - id: month_start\n      label: Start month\n",
                ),
            ),
            (
                "component label",
                GRAPH.replace(
                    "    - id: month_start_input\n",
                    "    - id: month_start_input\n      label: Start month\n",
                ),
            ),
        ] {
            assert_eq!(
                parse_composition_yaml(&invalid),
                Err(CompositionDiagnosticCode::InvalidComposition),
                "unexpectedly accepted {field}"
            );
        }
    }

    #[test]
    fn components_reference_only_graph_variables_of_compatible_shape() {
        let mut document = parse_composition_yaml(GRAPH).unwrap();
        let CompositionComponent::Metric { variable, .. } = &mut document.spec.components[2] else {
            panic!("fixture component should be a Metric");
        };
        *variable = "expense_rows".to_owned();
        assert_eq!(
            document.spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut document = parse_composition_yaml(GRAPH).unwrap();
        let CompositionComponent::Table { variable, .. } = &mut document.spec.components[3] else {
            panic!("fixture component should be a Table");
        };
        *variable = "month_total".to_owned();
        assert_eq!(
            document.spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn numeric_metric_presentation_is_checked_against_scalar_type() {
        let mut document = parse_composition_yaml(GRAPH).unwrap();
        let VariableGraphNodeKind::Query { returns, .. } = &mut document.spec.variables[3].kind
        else {
            panic!("fixture node should be a query");
        };
        *returns = VariableGraphResultType::Scalar {
            value_type: VariableGraphScalarDescriptor {
                scalar_type: VariableGraphScalarType::Date,
                nullable: false,
            },
        };
        let CompositionComponent::Metric { presentation, .. } = &mut document.spec.components[2]
        else {
            panic!("fixture component should be a Metric");
        };
        *presentation = Some(CompositionMetricPresentation::Number {
            unit: None,
            fraction_digits: None,
        });
        assert_eq!(
            super::canonicalize_composition(&document),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }
}
