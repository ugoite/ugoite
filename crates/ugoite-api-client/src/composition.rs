//! Portable Composition operation DTOs.
//!
//! These types describe the wire contract without depending on the storage
//! representation used by the Server.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Stable semantic diagnostic identifiers shared across Composition surfaces.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "unsupported_format_version" => Some(Self::UnsupportedFormatVersion),
            "invalid_composition" => Some(Self::InvalidComposition),
            "variable_unknown" => Some(Self::VariableUnknown),
            "variable_missing" => Some(Self::VariableMissing),
            "variable_type_mismatch" => Some(Self::VariableTypeMismatch),
            "query_unavailable" => Some(Self::QueryUnavailable),
            "missing_field" => Some(Self::MissingField),
            "field_type_changed" => Some(Self::FieldTypeChanged),
            "query_schema_changed" => Some(Self::QuerySchemaChanged),
            "metric_result_not_scalar" => Some(Self::MetricResultNotScalar),
            "metric_result_type_mismatch" => Some(Self::MetricResultTypeMismatch),
            "metric_scalar_no_rows" => Some(Self::MetricScalarNoRows),
            "metric_scalar_multiple_rows" => Some(Self::MetricScalarMultipleRows),
            "metric_result_column_missing" => Some(Self::MetricResultColumnMissing),
            "metric_result_column_ambiguous" => Some(Self::MetricResultColumnAmbiguous),
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

/// Create or update one saved Composition through the portable API.
///
/// A create omits both `composition_id` and `base_revision_id`. An update
/// supplies both, and the base revision is checked exactly by the Server.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSaveRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision_id: Option<String>,
    pub yaml: String,
}

/// Portable commit receipt for one Composition publication.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionPublicationReceipt {
    pub command_id: String,
    pub catalog_generation: u64,
    pub snapshot_id: i64,
    pub committed_revision_ids: Vec<String>,
    pub committed_at_micros: i64,
    pub data_file_count: usize,
}

/// Successful create or update response for `composition.save`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionSaveResponse {
    pub composition_id: String,
    pub revision_id: String,
    pub canonical_yaml: String,
    pub receipt: CompositionPublicationReceipt,
}

/// Restore one exact historical revision as a new append-only Composition
/// revision. `base_revision_id` must still be the current revision when the
/// Server publishes the restore.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionRestoreRequest {
    pub source_revision_id: String,
    pub base_revision_id: String,
}

/// Successful exact-revision Composition restore response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionRestoreResponse {
    pub composition_id: String,
    pub revision_id: String,
    pub restored_from_revision_id: String,
    pub canonical_yaml: String,
    pub receipt: CompositionPublicationReceipt,
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

/// Request to preview one unsaved candidate Composition document.
///
/// Resolution uses the same semantics as a saved revision but is keyed by
/// the draft fingerprint and creates no registry, history, or publication
/// state.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionPreviewRequest {
    pub yaml: String,
    /// Exact Composition revision this draft edits. Both identity fields must
    /// be supplied together; the server uses this revision only to preserve
    /// stable Form bindings for unchanged SQL relations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision_id: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
    /// Variables explicitly left unset; omission from both fields uses the
    /// declared default when one exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unset_variables: Vec<String>,
}

/// Caller-visible result of a side-effect-free Composition preview pass.
/// Variable definitions and the draft fingerprint are present whenever the
/// candidate parsed as a supported Composition document, including responses
/// with diagnostics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionPreviewResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_definitions: Option<Vec<CompositionVariableDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CompositionPreviewPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<CompositionResolveDiagnostic>,
}

/// Stored revision or unsaved draft evaluated as one immutable graph
/// snapshot. Draft bases are optional and always identify an exact revision.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionEvaluationTarget {
    SavedRevision {
        composition_id: String,
        revision_id: String,
    },
    Draft {
        yaml: String,
    },
}

/// Selects a stateless continuation for one table query variable.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionEvaluationPage {
    pub variable_id: String,
    pub cursor: String,
}

/// Request to resolve and evaluate a typed Composition query graph.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionEvaluationRequest {
    /// Caller-owned generation ID. Responses and page cursors echo/bind it so
    /// clients can discard stale results without depending on cancellation.
    pub evaluation_id: String,
    pub target: CompositionEvaluationTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision_id: Option<String>,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unset_variables: Vec<String>,
    /// Query variable IDs whose results should be recomputed. Core expands
    /// this list to include all query dependencies. Omission evaluates the
    /// full graph; an empty list evaluates no query nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_queries: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<CompositionEvaluationPage>,
}

/// One graph query result. `value` is the tagged, descriptor-validated
/// `scalar` or `table` result; it is keyed once by query variable ID even
/// when multiple display components reference that variable.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionGraphQueryResult {
    pub value: Value,
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// Query-graph execution failure scoped to one variable. `code` is the
/// stable Core evaluation code; nested result codes preserve the Domain's
/// scalar-cardinality and result-schema diagnostics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionEvaluationQueryDiagnostic {
    pub code: String,
    pub variable_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_diagnostic: Option<CompositionEvaluationRequestDiagnostic>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_diagnostic: Option<CompositionEvaluationResultDiagnostic>,
}

/// Query request compilation details, when an evaluation failure originated
/// in a graph-bound SQL argument or request.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionEvaluationRequestDiagnostic {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain_code: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycle: Vec<String>,
}

/// Stable query-result validation failure, including the Domain result code
/// that distinguishes scalar cardinality, column shape, and typed values.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionEvaluationResultDiagnostic {
    pub code: String,
    pub variable_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_diagnostic: Option<CompositionEvaluationResultContractDiagnostic>,
}

/// Exact Domain detail for an observed query-result mismatch.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionEvaluationResultContractDiagnostic {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_column_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_value_type: Option<CompositionScalarDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_value_type: Option<CompositionScalarDescriptor>,
}

/// Stateless graph evaluation response. `evaluation_id` and both fingerprints
/// identify the input, authorization, query, and Publication snapshot used.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionEvaluationResponse {
    pub evaluation_id: String,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_fingerprint: Option<String>,
    /// Stable identity for the authorization state rechecked after execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition_revision: Option<CompositionRevisionReference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_fingerprint: Option<String>,
    #[serde(default)]
    pub variable_values: BTreeMap<String, Value>,
    #[serde(default)]
    pub queries: Vec<CompositionResolvedQuery>,
    #[serde(default)]
    pub component_bindings: Vec<CompositionResolvedComponentBinding>,
    #[serde(default)]
    pub dependencies: CompositionDependencyIndex,
    #[serde(default)]
    pub query_results: BTreeMap<String, CompositionGraphQueryResult>,
    /// Query variables whose results were computed in this response, in
    /// dependency order. Failed evaluations return an empty list.
    pub computed_query_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<CompositionResolveDiagnostic>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub query_diagnostics: Vec<CompositionEvaluationQueryDiagnostic>,
}

/// Target of a bounded, read-only Composition Metric query.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionMetricQueryTarget {
    SavedRevision {
        composition_id: String,
        revision_id: String,
    },
    Draft {
        yaml: String,
    },
}

/// Request to evaluate one metric from a stored exact revision or unsaved
/// Composition draft.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryRequest {
    pub target: CompositionMetricQueryTarget,
    /// Exact Composition revision a draft edits. Both identity fields must
    /// be supplied together; saved-revision queries omit them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision_id: Option<String>,
    pub component_id: String,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
    /// Variables explicitly left unset; omission from both fields uses the
    /// declared default when one exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unset_variables: Vec<String>,
}

/// One successful display value selected from a Saved SQL result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryValue {
    pub component_id: String,
    pub result_type: CompositionResultFieldType,
    /// JSON encoding follows `result_type`: Integer results are canonical
    /// base-10 strings, Decimal results are strings preserving scale, Float
    /// results are JSON numbers, Boolean results are JSON booleans, and
    /// String/Date/Timestamp results are JSON strings. Successful SQL NULL is
    /// represented by a present JSON `null`.
    pub value: Value,
}

/// Stable metric-query diagnostic without backend error detail.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryDiagnostic {
    pub code: CompositionDiagnosticCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
}

/// Result of one bounded metric query. A present `value: null` is distinct
/// from a query that returns zero rows.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<CompositionMetricQueryValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<CompositionMetricQueryDiagnostic>,
}

/// Side-effect-free preview plan keyed by draft fingerprint instead of a
/// stored revision reference. Query descriptors and component references are
/// derived from the strict variable graph and current authorized metadata.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionPreviewPlan {
    pub draft_fingerprint: String,
    #[serde(default)]
    pub variable_values: BTreeMap<String, Value>,
    pub queries: Vec<CompositionResolvedQuery>,
    /// Stable display bindings in the resolver's layout row and item order.
    #[serde(default)]
    pub component_bindings: Vec<CompositionResolvedComponentBinding>,
    /// Semantic graph dependency edges derived by Core. Additive to the
    /// existing portable plan so adapters can schedule reactive work without
    /// re-parsing the Composition document.
    #[serde(default, skip_serializing_if = "CompositionDependencyIndex::is_empty")]
    pub dependencies: CompositionDependencyIndex,
}

/// Request to resolve one exact Composition revision against current query
/// metadata. Resolution does not execute any query or create state.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolveRequest {
    pub revision_id: String,
    #[serde(default)]
    pub variables: BTreeMap<String, Value>,
    /// Variables explicitly left unset; omission from both fields uses the
    /// declared default when one exists.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unset_variables: Vec<String>,
}

/// Caller-visible result of a side-effect-free Composition resolution pass.
/// Variable definitions are present whenever the exact revision parsed as a
/// supported Composition document, including responses with diagnostics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolveResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_definitions: Option<Vec<CompositionVariableDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<CompositionResolvePlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<CompositionResolveDiagnostic>,
}

/// Typed, UI-neutral fields needed to collect caller variable values.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionVariableDefinition {
    pub id: String,
    pub value_type: CompositionScalarDescriptor,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionScalarType {
    String,
    Boolean,
    Integer,
    Float,
    Decimal,
    Date,
    Timestamp,
    Json,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionScalarDescriptor {
    pub scalar_type: CompositionScalarType,
    pub nullable: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolvePlan {
    pub composition_revision: CompositionRevisionReference,
    #[serde(default)]
    pub variable_values: BTreeMap<String, Value>,
    pub queries: Vec<CompositionResolvedQuery>,
    /// Stable display bindings in the resolver's layout row and item order.
    #[serde(default)]
    pub component_bindings: Vec<CompositionResolvedComponentBinding>,
    #[serde(default, skip_serializing_if = "CompositionDependencyIndex::is_empty")]
    pub dependencies: CompositionDependencyIndex,
}

/// Variable and component consumers derived by Core independently of layout
/// order. The public variable input vocabulary remains unchanged in this
/// adapter revision; the index is an additive plan capability.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDependencyIndex {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variable_consumers: BTreeMap<String, CompositionVariableConsumers>,
}

impl CompositionDependencyIndex {
    fn is_empty(&self) -> bool {
        self.variable_consumers.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionVariableConsumers {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub input_component_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub text_component_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub query_variable_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub metric_component_ids: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub table_component_ids: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionResolvedComponentKind {
    Input,
    Metric,
    Table,
    Text,
}

/// Explicit display-only formatting for numeric Composition metrics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionMetricPresentation {
    Number {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fraction_digits: Option<u8>,
    },
}

/// Portable logical result type selected by the resolver for a metric.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolvedComponentBinding {
    pub component_id: String,
    pub kind: CompositionResolvedComponentKind,
    /// The single typed graph value bound to this component.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_result_type: Option<CompositionResultFieldType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<CompositionMetricPresentation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionRevisionReference {
    pub entry_id: String,
    pub revision_id: String,
}

/// A query request compiled by the Rust Core resolver. The descriptor is
/// checked against the current planner schema; `request: None` indicates a
/// query that waits for scalar query-variable dependencies.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolvedQuery {
    pub variable_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
    /// Exact SQL source for an authorized pinned Saved SQL revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql_text: Option<String>,
    #[serde(default)]
    pub deferred_arguments: Vec<CompositionDeferredArgument>,
    pub returns: CompositionQueryResultType,
    pub result_columns: Vec<CompositionResolvedResultColumn>,
    pub form_bindings: Vec<CompositionResolvedFormBinding>,
    pub schema_fingerprint: String,
    pub query_semantic_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scalar_cardinality: Option<CompositionScalarCardinality>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDeferredArgument {
    pub argument_name: String,
    pub variable_id: String,
    pub value_type: CompositionScalarDescriptor,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionQueryResultType {
    Scalar {
        value_type: CompositionScalarDescriptor,
    },
    Table {
        columns: Vec<CompositionResolvedResultColumn>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionScalarCardinality {
    ExactlyOneRow,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolvedFormBinding {
    pub relation: String,
    pub form_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolvedResultColumn {
    pub name: String,
    pub value_type: CompositionScalarDescriptor,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionResolveDiagnostic {
    pub code: CompositionDiagnosticCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
}
