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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<CompositionSection>,
}

impl CompositionSpec {
    /// Return components in section order and each section's reference order.
    ///
    /// A component must have a unique ID and be referenced exactly once by the
    /// sections. Unknown, duplicate, or missing references make the layout
    /// invalid; the top-level component declaration order is never a fallback.
    pub fn components_in_render_order(
        &self,
    ) -> Result<Vec<&CompositionComponent>, CompositionDiagnosticCode> {
        let mut components_by_id = HashMap::with_capacity(self.components.len());
        for component in &self.components {
            if components_by_id.insert(component.id(), component).is_some() {
                return Err(CompositionDiagnosticCode::InvalidComposition);
            }
        }

        let mut rendered_ids = HashSet::with_capacity(self.components.len());
        let mut ordered = Vec::with_capacity(self.components.len());
        for section in &self.sections {
            for component_id in &section.components {
                if !rendered_ids.insert(component_id.as_str()) {
                    return Err(CompositionDiagnosticCode::InvalidComposition);
                }
                let component = components_by_id
                    .get(component_id.as_str())
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
            Self::Metric { id, .. } | Self::Table { id, .. } => id,
        }
    }

    pub fn source_id(&self) -> &str {
        match self {
            Self::Metric { source, .. } | Self::Table { source, .. } => source,
        }
    }

    pub fn label(&self) -> Option<&str> {
        match self {
            Self::Metric { label, .. } | Self::Table { label, .. } => label.as_deref(),
        }
    }

    pub fn value_field(&self) -> Option<&CompositionMetricValueField> {
        match self {
            Self::Metric { value_field, .. } => Some(value_field),
            Self::Table { .. } => None,
        }
    }
}

/// A named group of dashboard components.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSection {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<String>,
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
        CompositionSection, CompositionSource, CompositionSpec, CompositionValue,
        EntryQueryTemplate, DEFAULT_COMPOSITION_PAGE_LIMIT,
    };

    const MONTHLY_EXPENSE: &str =
        include_str!("../tests/fixtures/composition/monthly-expense.ugcomp.yaml");

    #[test]
    fn components_follow_section_and_reference_order() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.components.push(CompositionComponent::Table {
            id: "last".to_string(),
            label: None,
            source: "expense_rows".to_string(),
        });
        spec.components.reverse();
        spec.sections.reverse();
        spec.sections[0].components.push("total".to_string());
        spec.sections[1].components.clear();
        spec.sections.push(CompositionSection {
            id: "later".to_string(),
            components: vec!["last".to_string()],
        });

        let component_ids: Vec<_> = spec
            .components_in_render_order()
            .unwrap()
            .into_iter()
            .map(CompositionComponent::id)
            .collect();

        assert_eq!(component_ids, ["transactions", "total", "last"]);
    }

    #[test]
    fn empty_component_layout_is_valid() {
        let spec = CompositionSpec {
            parameters: vec![],
            sources: vec![],
            components: vec![],
            sections: vec![],
        };

        assert!(spec.components_in_render_order().unwrap().is_empty());
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
        spec.sections[0].components.push("total".to_string());

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );

        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.sections[1].components.push("total".to_string());

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn unknown_component_references_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.sections[0].components[0] = "unknown".to_string();

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn unreferenced_components_are_invalid() {
        let mut spec = parse_composition_yaml(MONTHLY_EXPENSE).unwrap().spec;
        spec.sections[1].components.clear();

        assert_eq!(
            spec.components_in_render_order(),
            Err(CompositionDiagnosticCode::InvalidComposition)
        );
    }

    #[test]
    fn document_uses_the_portable_envelope_field_names() {
        let document: CompositionDocument = serde_json::from_str(
            r#"{"format":"ugoite.composition","format_version":1,"kind":"dashboard","name":"Example","tags":["demo"],"spec":{"parameters":[],"sources":[],"components":[],"sections":[]}}"#,
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
                sections: vec![],
            }
        );
        assert_eq!(serde_json::to_value(document).unwrap()["kind"], "dashboard");
    }

    #[test]
    fn unknown_model_fields_are_rejected() {
        let result = serde_json::from_str::<CompositionDocument>(
            r#"{"format":"ugoite.composition","format_version":1,"kind":"dashboard","name":"Example","tags":[],"spec":{"parameters":[],"sources":[],"components":[],"sections":[]},"extra":true}"#,
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
                "sections": [{"id": "overview", "components": ["total", "transactions"]}]
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
}
