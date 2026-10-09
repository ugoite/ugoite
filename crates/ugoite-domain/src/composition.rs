//! Typed Composition v1 domain contract.
//!
//! This module intentionally contains no YAML parser, storage, or query
//! execution code. It defines the shared Rust model that native and WASM
//! adapters use when parsing and resolving a Composition.

use crate::form::{FieldType, ListItemDefinition};
use crate::id::{EntryId, FieldId, FormId, RevisionId};
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
    pub parameters: Vec<CompositionParameter>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<CompositionSource>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<CompositionComponent>,
    pub layout: DashboardFlowLayout,
}

impl CompositionSpec {
    /// Return components in layout row order and each row's item order.
    ///
    /// A component must have a unique ID and be placed exactly once by the
    /// layout; a parameter control must reference a declared parameter
    /// exactly once. Unknown, duplicate, or missing component references,
    /// unknown or duplicate parameter references, empty layouts, empty rows,
    /// duplicate row IDs, and required parameters without a default and
    /// without a layout control make the layout invalid. The top-level
    /// component declaration order is never a fallback.
    pub fn components_in_render_order(
        &self,
    ) -> Result<Vec<&CompositionComponent>, CompositionDiagnosticCode> {
        let mut components_by_id = HashMap::with_capacity(self.components.len());
        for component in &self.components {
            if components_by_id.insert(component.id(), component).is_some() {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
        }

        let mut parameters_by_id = HashMap::with_capacity(self.parameters.len());
        for parameter in &self.parameters {
            if parameters_by_id
                .insert(parameter.id.as_str(), parameter)
                .is_some()
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
        }

        if self.layout.rows.is_empty() {
            return Err(CompositionDiagnosticCode::InvalidComposition);
        }

        let mut row_ids = HashSet::with_capacity(self.layout.rows.len());
        let mut rendered_ids = HashSet::with_capacity(self.components.len());
        let mut placed_parameters = HashSet::with_capacity(self.parameters.len());
        let mut ordered = Vec::with_capacity(self.components.len());
        for row in &self.layout.rows {
            if !row_ids.insert(row.id.as_str()) {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
            if row.items.is_empty() {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
            for item in &row.items {
                match item {
                    FlowItem::Component { component } => {
                        if !rendered_ids.insert(component.as_str()) {
                            return Err(CompositionDiagnosticCode::InvalidComposition);
                        }
                        let component = components_by_id
                            .get(component.as_str())
                            .copied()
                            .ok_or(CompositionDiagnosticCode::InvalidComposition)?;
                        ordered.push(component);
                    }
                    FlowItem::Parameter { parameter } => {
                        if !parameters_by_id.contains_key(parameter.as_str()) {
                            return Err(CompositionDiagnosticCode::InvalidComposition);
                        }
                        if !placed_parameters.insert(parameter.as_str()) {
                            return Err(CompositionDiagnosticCode::InvalidComposition);
                        }
                    }
                }
            }
        }

        if rendered_ids.len() != components_by_id.len() {
            return Err(CompositionDiagnosticCode::InvalidComposition);
        }

        for parameter in &self.parameters {
            if parameter.required
                && parameter.default.is_none()
                && !placed_parameters.contains(parameter.id.as_str())
            {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
        }

        Ok(ordered)
    }

    /// Return declared parameters with layout-placed controls first in layout
    /// row and item order, then unplaced parameters in declaration order.
    ///
    /// Unknown or duplicate parameter references make the placement invalid.
    /// Row structure and component placement stay owned by
    /// [`components_in_render_order`](Self::components_in_render_order); this
    /// helper only orders the existing semantic definitions for callers that
    /// expose placed parameters first.
    pub fn parameters_in_placement_order(
        &self,
    ) -> Result<Vec<&CompositionParameter>, CompositionDiagnosticCode> {
        let mut declared = HashMap::with_capacity(self.parameters.len());
        for parameter in &self.parameters {
            if declared.insert(parameter.id.as_str(), parameter).is_some() {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
        }

        let mut seen = HashSet::with_capacity(self.parameters.len());
        let mut ordered = Vec::with_capacity(self.parameters.len());
        for row in &self.layout.rows {
            for item in &row.items {
                if let FlowItem::Parameter { parameter } = item {
                    let Some(definition) = declared.get(parameter.as_str()).copied() else {
                        return Err(CompositionDiagnosticCode::InvalidComposition);
                    };
                    if !seen.insert(parameter.as_str()) {
                        return Err(CompositionDiagnosticCode::InvalidComposition);
                    }
                    ordered.push(definition);
                }
            }
        }
        for parameter in &self.parameters {
            if !seen.contains(parameter.id.as_str()) {
                ordered.push(parameter);
            }
        }
        Ok(ordered)
    }
}

/// A value that can be supplied to a Composition source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionParameter {
    pub id: String,
    /// Optional display text; parameter binding continues to use `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(rename = "type")]
    pub parameter_type: CompositionParameterType,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<CompositionLiteral>,
    /// A display/input hint. It does not define an expression or conversion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<CompositionParameterFormat>,
}

/// Supported Composition parameter value types.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionParameterType {
    String,
    Boolean,
    Integer,
    Float,
    Date,
    Timestamp,
}

/// Supported presentation hints for typed parameter values.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionParameterFormat {
    YearMonth,
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
    Date,
    Timestamp,
    Json,
}

impl CompositionResultFieldType {
    /// Whether this result type can be selected by a single-value metric.
    pub const fn supports_metric(self) -> bool {
        !matches!(self, Self::Json)
    }
}

/// The logical schema snapshot of one query-used Form field, including typed
/// List item and RowReference target metadata when applicable.
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
/// bounded `EntryQuery` type after binding parameters.
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

/// A filter whose value may be a literal or a named parameter reference.
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

/// A source value expressed directly or as a named parameter reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompositionValue {
    Parameter(CompositionParameterReference),
    Literal(CompositionLiteral),
}

/// A JSON scalar used as a parameter default or source literal.
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
pub struct CompositionParameterReference {
    pub parameter: String,
}

/// The stable identity of the scalar read by a metric component.
///
/// EntryQuery properties use a stable `FieldId`; Saved SQL results use the
/// exact output column name returned by the selected query revision. The
/// resolver validates that this variant matches the component's source.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionMetricValueField {
    EntryField { field_id: FieldId },
    SqlColumn { name: String },
}

/// A visual component supported by the dashboard renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionComponent {
    Text {
        id: String,
        /// Optional display text; component identity continues to use IDs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        text: String,
        style: TextStyle,
    },
    Metric {
        id: String,
        /// Optional display text; component and source bindings continue to use IDs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        source: String,
        value_field: CompositionMetricValueField,
    },
    Table {
        id: String,
        /// Optional display text; component and source bindings continue to use IDs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        source: String,
    },
}

impl CompositionComponent {
    pub fn id(&self) -> &str {
        match self {
            Self::Text { id, .. } | Self::Metric { id, .. } | Self::Table { id, .. } => id,
        }
    }

    /// The referenced source, if the component reads from one. Text carries
    /// no source binding; the core resolver skips it when compiling requests.
    pub fn source_id(&self) -> Option<&str> {
        match self {
            Self::Text { .. } => None,
            Self::Metric { source, .. } | Self::Table { source, .. } => Some(source),
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Text { label, .. } | Self::Metric { label, .. } | Self::Table { label, .. } => {
                label.as_deref()
            }
        }
    }

    pub fn value_field(&self) -> Option<&CompositionMetricValueField> {
        match self {
            Self::Metric { value_field, .. } => Some(value_field),
            Self::Text { .. } | Self::Table { .. } => None,
        }
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

/// One stable layout row carrying ordered component and parameter items.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowRow {
    pub id: String,
    pub items: Vec<FlowItem>,
}

/// One layout placement: a component reference or a parameter control.
///
/// Parameters keep their semantic definition under `spec.parameters`; the
/// layout only places a control bound to the parameter ID.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum FlowItem {
    Parameter { parameter: String },
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
    ParameterUnknown,
    ParameterMissing,
    ParameterTypeMismatch,
    SourceUnavailable,
    MissingField,
    FieldTypeChanged,
    SourceSchemaChanged,
    MetricFieldNotProjected,
    MetricResultNotScalar,
    MetricResultTypeMismatch,
    MetricResultEmpty,
    MetricResultMultipleRows,
    MetricResultColumnMissing,
    MetricResultColumnAmbiguous,
    MetricResultPageIncomplete,
}

impl CompositionDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedFormatVersion => "unsupported_format_version",
            Self::InvalidComposition => "invalid_composition",
            Self::ParameterUnknown => "parameter_unknown",
            Self::ParameterMissing => "parameter_missing",
            Self::ParameterTypeMismatch => "parameter_type_mismatch",
            Self::SourceUnavailable => "source_unavailable",
            Self::MissingField => "missing_field",
            Self::FieldTypeChanged => "field_type_changed",
            Self::SourceSchemaChanged => "source_schema_changed",
            Self::MetricFieldNotProjected => "metric_field_not_projected",
            Self::MetricResultNotScalar => "metric_result_not_scalar",
            Self::MetricResultTypeMismatch => "metric_result_type_mismatch",
            Self::MetricResultEmpty => "metric_result_empty",
            Self::MetricResultMultipleRows => "metric_result_multiple_rows",
            Self::MetricResultColumnMissing => "metric_result_column_missing",
            Self::MetricResultColumnAmbiguous => "metric_result_column_ambiguous",
            Self::MetricResultPageIncomplete => "metric_result_page_incomplete",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_composition_yaml, CompositionComponent, CompositionDiagnosticCode,
        CompositionDocument, CompositionFormat, CompositionKind, CompositionMetricValueField,
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

    fn parameter_item(id: &str) -> FlowItem {
        FlowItem::Parameter {
            parameter: id.to_owned(),
        }
    }

    fn flow_layout(rows: Vec<FlowRow>) -> DashboardFlowLayout {
        DashboardFlowLayout {
            kind: FlowLayoutKind::Flow,
            rows,
        }
    }

    #[test]
    fn components_follow_layout_row_and_item_order() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.components.push(CompositionComponent::Table {
            id: "last".to_string(),
            label: None,
            source: "expense_rows".to_string(),
        });
        spec.components.reverse();
        spec.layout = flow_layout(vec![
            layout_row(
                "controls",
                vec![parameter_item("month_start"), parameter_item("month_end")],
            ),
            layout_row("detail", vec![component_item("transactions")]),
            layout_row("summary", vec![component_item("total")]),
            layout_row("later", vec![component_item("last")]),
        ]);

        let component_ids: Vec<_> = spec
            .components_in_render_order()
            .unwrap()
            .into_iter()
            .map(CompositionComponent::id)
            .collect();

        assert_eq!(component_ids, ["transactions", "total", "last"]);
    }

    #[test]
    fn placed_parameters_come_first_in_layout_order() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        // Declare an unplaced optional parameter after the placed ones.
        spec.parameters[1].required = false;
        spec.parameters.push(
            serde_json::from_value(serde_json::json!({
                "id": "region",
                "type": "string",
                "required": false,
            }))
            .unwrap(),
        );
        // Placement order is the reverse of declaration order.
        spec.layout.rows[0].items = vec![
            parameter_item("month_end"),
            parameter_item("month_start"),
            spec.layout.rows[0].items[2].clone(),
            spec.layout.rows[0].items[3].clone(),
        ];

        let ordered_ids: Vec<_> = spec
            .parameters_in_placement_order()
            .expect("placed parameters order")
            .into_iter()
            .map(|parameter| parameter.id.as_str())
            .collect();

        assert_eq!(ordered_ids, ["month_end", "month_start", "region"]);
    }

    #[test]
    fn unknown_and_duplicate_parameter_placements_have_no_order() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0].items.push(parameter_item("unknown"));
        assert_eq!(
            spec.parameters_in_placement_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout
            .rows
            .push(layout_row("repeated", vec![parameter_item("month_start")]));
        assert_eq!(
            spec.parameters_in_placement_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
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
    fn unknown_parameter_references_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0].items.push(parameter_item("unknown"));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_parameter_placements_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout
            .rows
            .push(layout_row("repeated", vec![parameter_item("month_start")]));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn duplicate_parameter_ids_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.parameters.push(spec.parameters[0].clone());

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn required_parameter_without_default_needs_a_layout_control() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.layout.rows[0]
            .items
            .retain(|item| *item != parameter_item("month_end"));

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn optional_and_defaulted_parameters_do_not_need_a_layout_control() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.parameters[0].required = false;
        spec.layout.rows[0]
            .items
            .retain(|item| *item != parameter_item("month_start"));

        assert!(spec.components_in_render_order().is_ok());

        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.parameters[1].default =
            Some(serde_json::from_value(serde_json::json!("2026-11-01")).unwrap());
        spec.layout.rows[0]
            .items
            .retain(|item| *item != parameter_item("month_end"));

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
            text: "Summary".to_string(),
            style: TextStyle::Heading,
        };
        assert_eq!(component.id(), "heading");
        assert_eq!(component.source_id(), None);
        assert_eq!(
            serde_json::to_value(&component).unwrap(),
            serde_json::json!({
                "kind": "text",
                "id": "heading",
                "text": "Summary",
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
            r#"{"format":"ugoite.composition","format_version":1,"kind":"dashboard","name":"Example","tags":["demo"],"spec":{"parameters":[],"sources":[],"components":[],"layout":{"kind":"flow","rows":[]}}}"#,
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
                parameters: vec![],
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
            r#"{"format":"ugoite.composition","format_version":1,"kind":"dashboard","name":"Example","tags":[],"spec":{"parameters":[],"sources":[],"components":[],"layout":{"kind":"flow","rows":[]}},"extra":true}"#,
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
    fn dashboard_model_carries_exact_sources_and_query_templates() {
        let document: CompositionDocument = serde_json::from_value(serde_json::json!({
            "format": "ugoite.composition",
            "format_version": 1,
            "kind": "dashboard",
            "name": "Monthly expenses",
            "tags": [],
            "spec": {
                "parameters": [
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
                            "text": {"parameter": "search"},
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
                        "variables": {"month": {"parameter": "month"}}
                    }
                ],
                "components": [
                    {"kind": "metric", "id": "total", "source": "monthly_total", "value_field": {"kind": "sql_column", "name": "total"}},
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
            Some(CompositionValue::Parameter(reference)) if reference.parameter == "search"
        ));
        assert_eq!(document.spec.sources[1].id(), "monthly_total");
        assert!(matches!(
            &document.spec.components[0],
            CompositionComponent::Metric {
                value_field: CompositionMetricValueField::SqlColumn { name },
                ..
            } if name == "total"
        ));
    }

    #[test]
    fn parameter_reference_and_literal_values_reject_unknown_mapping_keys() {
        assert!(
            serde_json::from_str::<CompositionValue>(r#"{"parameter":"month","extra":true}"#)
                .is_err()
        );
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
                CompositionDiagnosticCode::ParameterUnknown,
                "parameter_unknown",
            ),
            (
                CompositionDiagnosticCode::ParameterMissing,
                "parameter_missing",
            ),
            (
                CompositionDiagnosticCode::ParameterTypeMismatch,
                "parameter_type_mismatch",
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
                CompositionDiagnosticCode::MetricFieldNotProjected,
                "metric_field_not_projected",
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
            "parameter_unknown",
            "parameter_missing",
            "parameter_type_mismatch",
            "source_unavailable",
            "missing_field",
            "field_type_changed",
            "source_schema_changed",
            "metric_field_not_projected",
            "metric_result_not_scalar",
            "metric_result_type_mismatch",
            "metric_result_empty",
            "metric_result_multiple_rows",
            "metric_result_column_missing",
            "metric_result_column_ambiguous",
            "metric_result_page_incomplete",
        ];
        assert_eq!(frozen_codes.len(), 17);
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

        // Metric field identity: stable FieldId versus exact SQL column name.
        let entry_field = CompositionMetricValueField::EntryField {
            field_id: crate::id::FieldId::new(100).unwrap(),
        };
        assert_eq!(
            serde_json::to_value(&entry_field).unwrap()["kind"],
            serde_json::json!("entry_field")
        );
        let sql_column = CompositionMetricValueField::SqlColumn {
            name: "total".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&sql_column).unwrap()["kind"],
            serde_json::json!("sql_column")
        );

        // Dashboard flow layout (ADR-019 re-freeze): flow is the only layout
        // kind; rows carry stable IDs with ordered component and parameter
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
        assert_eq!(
            serde_json::to_value(FlowItem::Parameter {
                parameter: "month".to_string(),
            })
            .unwrap(),
            serde_json::json!({"kind": "parameter", "parameter": "month"})
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
            text: "Summary".to_string(),
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
