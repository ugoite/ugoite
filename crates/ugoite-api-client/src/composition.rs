//! Portable Composition read response DTOs.
//!
//! These types describe the wire contract without depending on the storage
//! representation used by the Server.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Stable semantic diagnostic identifiers shared across Composition surfaces.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionDiagnosticCode {
    UnsupportedFormatVersion,
    InvalidComposition,
    ParameterUnknown,
    ParameterMissing,
    ParameterTypeMismatch,
    SourceUnavailable,
    MissingForm,
    MissingField,
    FieldTypeChanged,
    SourceSchemaChanged,
    MetricFieldNotProjected,
    SavedSqlRevisionMissing,
    NotAuthorized,
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
            Self::MissingForm => "missing_form",
            Self::MissingField => "missing_field",
            Self::FieldTypeChanged => "field_type_changed",
            Self::SourceSchemaChanged => "source_schema_changed",
            Self::MetricFieldNotProjected => "metric_field_not_projected",
            Self::SavedSqlRevisionMissing => "saved_sql_revision_missing",
            Self::NotAuthorized => "not_authorized",
        }
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "unsupported_format_version" => Some(Self::UnsupportedFormatVersion),
            "invalid_composition" => Some(Self::InvalidComposition),
            "parameter_unknown" => Some(Self::ParameterUnknown),
            "parameter_missing" => Some(Self::ParameterMissing),
            "parameter_type_mismatch" => Some(Self::ParameterTypeMismatch),
            "source_unavailable" => Some(Self::SourceUnavailable),
            "missing_form" => Some(Self::MissingForm),
            "missing_field" => Some(Self::MissingField),
            "field_type_changed" => Some(Self::FieldTypeChanged),
            "source_schema_changed" => Some(Self::SourceSchemaChanged),
            "metric_field_not_projected" => Some(Self::MetricFieldNotProjected),
            "saved_sql_revision_missing" => Some(Self::SavedSqlRevisionMissing),
            "not_authorized" => Some(Self::NotAuthorized),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionLintValue {
    pub document: Value,
    pub canonical_yaml: String,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionLintError {
    pub kind: String,
    pub code: CompositionDiagnosticCode,
}

/// Result of the side-effect-free `composition.lint` operation.
///
/// A successful response sets `ok` and `value`; a diagnostic response sets
/// `error`. The Server omits the inactive optional field in each shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionLintResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<CompositionLintValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<CompositionLintError>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionRawRevision {
    pub revision: CompositionRevisionMetadata,
    pub fields: BTreeMap<String, Value>,
    pub unmapped_field_values: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionRevisionMetadata {
    pub form_id: String,
    pub entry_id: String,
    pub revision_id: String,
    pub parent_revision_id: Option<String>,
    pub entry_version: u64,
    pub change_id: String,
    pub expected_version: Option<u64>,
    /// `upsert`, `delete`, or `restore`.
    pub operation: String,
    pub committed_at_micros: i64,
    pub author_id: String,
    pub form_version: u32,
    pub source_kind: String,
    pub source_id: Option<String>,
    pub entry: CompositionEntryMetadata,
    pub extra_attributes: BTreeMap<String, Value>,
    pub extension_metadata: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionEntryMetadata {
    pub external_id: String,
    pub tags: Vec<String>,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
    pub updated_by: String,
    pub integrity: CompositionEntryIntegrity,
    pub deleted: bool,
    pub deleted_at_micros: Option<i64>,
    pub deleted_by: Option<String>,
    pub restored_from: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct CompositionEntryIntegrity {
    pub checksum: String,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionHistoryPage {
    pub entry_id: String,
    pub revisions: Vec<CompositionRawRevision>,
    pub total: usize,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

/// One bounded current-index projection for a Composition.
///
/// The listing intentionally exposes only its stable entry/revision identity
/// and raw summary fields. It never transports the stored YAML `spec`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionListItem {
    pub composition_id: String,
    pub revision_id: String,
    pub updated_at: f64,
    pub name: Option<Value>,
    pub kind: Option<Value>,
    pub format_version: Option<Value>,
    pub tags: Vec<String>,
}

/// Bounded current Composition page. Authorization is evaluated by the
/// server for each request; `has_more` is the only continuation signal.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct CompositionListPage {
    pub items: Vec<CompositionListItem>,
    pub offset: usize,
    pub limit: usize,
    pub has_more: bool,
}

/// Request to resolve one exact Composition revision against current source
/// metadata. Resolution does not execute any source query or create state.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolveRequest {
    pub revision_id: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

/// Caller-visible result of a side-effect-free Composition resolution pass.
/// Parameter definitions are present whenever the exact revision parsed as a
/// supported Composition document, including responses with diagnostics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolveResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_definitions: Option<Vec<CompositionParameterDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CompositionResolvePlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<CompositionResolveDiagnostic>,
}

/// Typed, UI-neutral fields needed to collect caller parameter values.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionParameterDefinition {
    pub id: String,
    #[serde(rename = "type")]
    pub parameter_type: CompositionParameterType,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<CompositionParameterFormat>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionParameterType {
    String,
    Boolean,
    Integer,
    Float,
    Date,
    Timestamp,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompositionParameterFormat {
    YearMonth,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolvePlan {
    pub composition_revision: CompositionRevisionReference,
    pub sources: Vec<CompositionResolvedSource>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionRevisionReference {
    pub entry_id: String,
    pub revision_id: String,
}

/// A query request compiled by the Rust core resolver. The request value is
/// forwarded unchanged to the existing `entry.query` or `sql.query` operation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionResolvedSource {
    EntryQuery {
        source_id: String,
        request: Value,
        source_schema_fingerprint: String,
    },
    SavedSql {
        source_id: String,
        request: Value,
        source_schema_fingerprint: String,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolveDiagnostic {
    pub code: CompositionDiagnosticCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_id: Option<String>,
}
