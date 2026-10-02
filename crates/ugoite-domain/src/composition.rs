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
use std::collections::BTreeMap;

mod canonical;
mod yaml;

pub use canonical::{
    canonicalize_composition, canonicalize_composition_yaml, CanonicalComposition,
};
pub use yaml::{
    parse_composition_yaml, MAX_COMPOSITION_COLLECTION_ITEMS, MAX_COMPOSITION_YAML_BYTES,
    MAX_COMPOSITION_YAML_DEPTH,
};

pub const COMPOSITION_FORMAT_VERSION: u32 = 1;
/// Default page size for an EntryQuery source when the document omits it.
/// The core resolver still validates the value against EntryQuery's maximum.
pub const DEFAULT_COMPOSITION_PAGE_LIMIT: usize = 100;

/// A complete portable Composition document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDocument {
    pub format_version: u32,
    pub name: String,
    pub kind: CompositionKind,
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

/// A value that can be supplied to a Composition source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionParameter {
    pub id: String,
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

/// A visual component supported by the dashboard renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionComponent {
    Metric {
        id: String,
        source: String,
        value_field: String,
    },
    Table {
        id: String,
        source: String,
    },
}

impl CompositionComponent {
    pub fn id(&self) -> &str {
        match self {
            Self::Metric { id, .. } | Self::Table { id, .. } => id,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CompositionDiagnosticCode, CompositionDocument, CompositionKind, CompositionSource,
        CompositionSpec, CompositionValue, EntryQueryTemplate, DEFAULT_COMPOSITION_PAGE_LIMIT,
    };

    #[test]
    fn document_uses_the_portable_envelope_field_names() {
        let document: CompositionDocument = serde_json::from_str(
            r#"{"format_version":1,"name":"Example","kind":"dashboard","spec":{"parameters":[],"sources":[],"components":[],"sections":[]}}"#,
        )
        .unwrap();

        assert_eq!(document.format_version, 1);
        assert_eq!(document.kind, CompositionKind::Dashboard);
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
            r#"{"format_version":1,"name":"Example","kind":"dashboard","spec":{"parameters":[],"sources":[],"components":[],"sections":[]},"extra":true}"#,
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
            "format_version": 1,
            "name": "Monthly expenses",
            "kind": "dashboard",
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
                        "variables": {"month": {"parameter": "month"}}
                    }
                ],
                "components": [
                    {"kind": "metric", "id": "total", "source": "monthly_total", "value_field": "total"},
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
        assert_eq!(
            CompositionDiagnosticCode::UnsupportedFormatVersion.as_str(),
            "unsupported_format_version"
        );
        assert_eq!(
            CompositionDiagnosticCode::InvalidComposition.as_str(),
            "invalid_composition"
        );
        assert_eq!(
            serde_json::to_string(&CompositionDiagnosticCode::SourceUnavailable).unwrap(),
            "\"source_unavailable\""
        );
    }
}
