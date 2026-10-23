//! Pure Composition variable graph planning.
//!
//! Core compiles the strict typed graph into bounded SQL requests. Current
//! query metadata is supplied by the storage-backed authorization boundary;
//! this module performs no storage reads and grants no authorization.

use crate::sql_query::{SqlQueryRequest, MAX_SQL_COLUMN_NAME_BYTES, MAX_SQL_OUTPUT_COLUMNS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_domain::composition::{
    variable_graph_dependency_order, variable_graph_literal_matches, CompositionComponent,
    CompositionDiagnosticCode, CompositionMetricPresentation, CompositionResultFieldType,
    CompositionSpec, CompositionTextContent, VariableGraph, VariableGraphColumn,
    VariableGraphFormBinding, VariableGraphInputDiagnosticCode, VariableGraphLiteral,
    VariableGraphNode, VariableGraphNodeKind, VariableGraphQueryDefinition,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
};
use ugoite_domain::id::{EntryId, RevisionId};
use uuid::Uuid;

/// Bounded page size for query variables returning a table.
pub const DEFAULT_COMPOSITION_QUERY_PAGE_LIMIT: usize = 100;

/// One stable diagnostic from graph input or query planning.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDiagnostic {
    pub code: CompositionDiagnosticCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
}

impl CompositionDiagnostic {
    fn for_variable(code: CompositionDiagnosticCode, variable_id: &str) -> Self {
        Self {
            code,
            variable_id: Some(variable_id.to_owned()),
        }
    }

    fn without_variable(code: CompositionDiagnosticCode) -> Self {
        Self {
            code,
            variable_id: None,
        }
    }
}

/// Resolved scalar constants and caller inputs for one graph snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VariableGraphBindings {
    pub values: BTreeMap<String, VariableGraphLiteral>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

/// Bind typed constants and input values. Query variables are never accepted
/// from the caller; they enter the graph only through validated SQL results.
pub fn bind_variables(
    variables: &[VariableGraphNode],
    supplied: &BTreeMap<String, Value>,
) -> VariableGraphBindings {
    bind_variables_with_unset(variables, supplied, &[])
}

/// Bind typed values while allowing an Input to be explicitly unset, even if
/// its document has a default. Unknown and non-Input values fail closed.
pub fn bind_variables_with_unset(
    variables: &[VariableGraphNode],
    supplied: &BTreeMap<String, Value>,
    unset_variables: &[String],
) -> VariableGraphBindings {
    let mut graph = VariableGraph {
        variables: variables.to_vec(),
    };
    let unset = unset_variables
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    let mut diagnostics = Vec::new();
    for id in &unset {
        match graph.variables.iter_mut().find(|node| node.id == *id) {
            Some(VariableGraphNode {
                kind: VariableGraphNodeKind::Input { default, .. },
                ..
            }) => *default = None,
            Some(_) => diagnostics.push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::InvalidComposition,
                id,
            )),
            None => diagnostics.push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::VariableUnknown,
                id,
            )),
        }
    }
    if !diagnostics.is_empty() {
        return VariableGraphBindings {
            values: BTreeMap::new(),
            diagnostics,
        };
    }

    let mut typed_supplied = BTreeMap::new();
    let nodes = graph
        .variables
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    for (id, value) in supplied {
        let Some(node) = nodes.get(id.as_str()) else {
            diagnostics.push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::VariableUnknown,
                id,
            ));
            continue;
        };
        let VariableGraphNodeKind::Input { value_type, .. } = node.kind else {
            diagnostics.push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::InvalidComposition,
                id,
            ));
            continue;
        };
        match literal_from_json(value_type, value) {
            Some(literal) => {
                typed_supplied.insert(id.clone(), literal);
            }
            None => diagnostics.push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::VariableTypeMismatch,
                id,
            )),
        }
    }
    if !diagnostics.is_empty() {
        return VariableGraphBindings {
            values: BTreeMap::new(),
            diagnostics,
        };
    }
    match ugoite_domain::composition::resolve_variable_graph_inputs(&graph, &typed_supplied) {
        Ok(resolved) => VariableGraphBindings {
            values: resolved.values,
            diagnostics: Vec::new(),
        },
        Err(error) => {
            let mut mapped = error
                .inputs
                .into_iter()
                .map(|diagnostic| {
                    let code = match diagnostic.code {
                        VariableGraphInputDiagnosticCode::UnknownInput => {
                            CompositionDiagnosticCode::VariableUnknown
                        }
                        VariableGraphInputDiagnosticCode::MissingRequiredInput => {
                            CompositionDiagnosticCode::VariableMissing
                        }
                        VariableGraphInputDiagnosticCode::InputTypeMismatch => {
                            CompositionDiagnosticCode::VariableTypeMismatch
                        }
                        VariableGraphInputDiagnosticCode::ValueForNonInput => {
                            CompositionDiagnosticCode::InvalidComposition
                        }
                    };
                    CompositionDiagnostic::for_variable(code, &diagnostic.variable_id)
                })
                .collect::<Vec<_>>();
            if !error.graph.is_empty() {
                mapped.push(CompositionDiagnostic::without_variable(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            }
            mapped.sort_by(|left, right| {
                (left.code.as_str(), &left.variable_id)
                    .cmp(&(right.code.as_str(), &right.variable_id))
            });
            mapped.dedup();
            VariableGraphBindings {
                values: BTreeMap::new(),
                diagnostics: mapped,
            }
        }
    }
}

/// Bind inputs for an explicit query selection. Graph structure and caller
/// identities are always validated; unrelated known inputs cannot prevent a
/// selected dependency closure from running. Full evaluation remains strict.
pub fn bind_variables_for_queries(
    variables: &[VariableGraphNode],
    supplied: &BTreeMap<String, Value>,
    unset_variables: &[String],
    requested_queries: Option<&[String]>,
) -> VariableGraphBindings {
    let Some(requested) = requested_queries else {
        return bind_variables_with_unset(variables, supplied, unset_variables);
    };
    let graph = VariableGraph {
        variables: variables.to_vec(),
    };
    let selected = match crate::composition_query::selected_query_dependency_closure_for_graph(
        &graph,
        Some(requested),
    ) {
        Ok(selected) => selected,
        Err(_) => {
            return VariableGraphBindings {
                values: BTreeMap::new(),
                diagnostics: vec![CompositionDiagnostic::without_variable(
                    CompositionDiagnosticCode::InvalidComposition,
                )],
            }
        }
    };
    let required = crate::composition_query::input_dependencies_for_queries(&graph, &selected);
    let mut scoped_variables = variables.to_vec();
    let mut scoped_values = supplied.clone();
    for node in &mut scoped_variables {
        if required.contains(&node.id) {
            continue;
        }
        if let VariableGraphNodeKind::Input {
            value_type,
            required,
            default,
        } = &mut node.kind
        {
            *required = false;
            if supplied.get(&node.id).is_some_and(|value| {
                literal_from_json(*value_type, value)
                    .is_none_or(|literal| !variable_graph_literal_matches(value_type, &literal))
            }) {
                scoped_values.remove(&node.id);
                *default = None;
            }
        }
    }
    bind_variables_with_unset(&scoped_variables, &scoped_values, unset_variables)
}

fn literal_from_json(
    descriptor: VariableGraphScalarDescriptor,
    value: &Value,
) -> Option<VariableGraphLiteral> {
    if value.is_null() {
        return descriptor.nullable.then_some(VariableGraphLiteral::Null);
    }
    Some(match descriptor.scalar_type {
        VariableGraphScalarType::String => VariableGraphLiteral::String(value.as_str()?.to_owned()),
        VariableGraphScalarType::Boolean => VariableGraphLiteral::Boolean(value.as_bool()?),
        VariableGraphScalarType::Integer => VariableGraphLiteral::Integer(value.as_i64()?),
        VariableGraphScalarType::Float => VariableGraphLiteral::Float(value.as_f64()?),
        VariableGraphScalarType::Decimal => {
            VariableGraphLiteral::Decimal(value.as_str()?.to_owned())
        }
        VariableGraphScalarType::Date => VariableGraphLiteral::Date(value.as_str()?.to_owned()),
        VariableGraphScalarType::Timestamp => {
            VariableGraphLiteral::Timestamp(value.as_str()?.to_owned())
        }
        VariableGraphScalarType::Json => VariableGraphLiteral::Json(value.clone()),
    })
}

/// Planner-derived schema for an inline query variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InlineSqlQueryMetadata {
    pub bound_sql: String,
    pub form_bindings: Vec<VariableGraphFormBinding>,
    pub parameter_names: BTreeSet<String>,
    pub result_columns: Vec<VariableGraphColumn>,
    pub schema_fingerprint: String,
}

/// Planner-derived schema for one exact Saved SQL revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedSqlRevisionMetadata {
    pub entry_id: String,
    pub revision_id: String,
    /// SQL source read from this exact immutable revision after authorization.
    pub sql_text: String,
    pub parameter_types: BTreeMap<String, VariableGraphScalarDescriptor>,
    pub form_bindings: Vec<VariableGraphFormBinding>,
    pub result_columns: Vec<VariableGraphColumn>,
    pub schema_fingerprint: String,
}

/// Current query metadata read through the caller's authorization boundary.
#[derive(Clone, Copy)]
pub enum CurrentQueryDescriptor<'a> {
    InlineSql {
        variable_id: &'a str,
        metadata: Option<&'a InlineSqlQueryMetadata>,
    },
    SavedSql {
        variable_id: &'a str,
        metadata: Option<&'a SavedSqlRevisionMetadata>,
    },
}

impl CurrentQueryDescriptor<'_> {
    fn variable_id(&self) -> &str {
        match self {
            Self::InlineSql { variable_id, .. } | Self::SavedSql { variable_id, .. } => variable_id,
        }
    }
}

/// Identity of the exact Composition revision being resolved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionRevisionRef {
    pub entry_id: EntryId,
    pub revision_id: RevisionId,
}

/// Inputs for one pure graph resolution pass.
pub struct ResolveInput<'a> {
    pub composition_revision: CompositionRevisionRef,
    /// The caller-owned identity for this immutable query plan snapshot.
    pub execution_id: Uuid,
    pub spec: &'a CompositionSpec,
    pub variables: &'a BTreeMap<String, Value>,
    pub current_queries: &'a [CurrentQueryDescriptor<'a>],
}

/// One bounded query request keyed by its graph variable ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedQueryRequest {
    pub variable_id: String,
    pub request: Option<SqlQueryRequest>,
    /// Exact source text for Saved SQL; absent for inline SQL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql_text: Option<String>,
    /// Authorized source rebound to immutable Form relation IDs by the service.
    /// It may be used as a new inline definition; it grants no read authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub portable_sql: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deferred_arguments:
        Vec<crate::composition_query::VariableGraphQueryRequestDeferredArgument>,
    /// Planner-confirmed output contract. In draft preview this may replace
    /// an explicitly incomplete return descriptor; persistence always checks
    /// the same contract before accepting the document.
    pub returns: VariableGraphResultType,
    /// Scalar query variables have an explicit one-row result contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scalar_cardinality: Option<CompositionScalarCardinality>,
    pub result_columns: Vec<VariableGraphColumn>,
    pub form_bindings: Vec<VariableGraphFormBinding>,
    pub schema_fingerprint: String,
    /// Stable identity for authoring-time reuse. Inline SQL hashes its exact
    /// bound SQL and all bindings; Saved SQL also pins exact entry/revision.
    pub query_semantic_fingerprint: String,
}

/// Cardinality promised by a scalar query variable's output contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionScalarCardinality {
    ExactlyOneRow,
}

/// One variable's input, text, query, or visual consumers.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableConsumers {
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

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionDependencyIndex {
    #[serde(default)]
    pub variable_consumers: BTreeMap<String, VariableConsumers>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedComponentKind {
    Input,
    Metric,
    #[serde(rename = "table")]
    Tabular,
    Text,
}

/// Component reference resolved against the typed graph; every data component
/// carries only its variable identity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedComponentBinding {
    pub component_id: String,
    pub kind: ResolvedComponentKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_result_type: Option<CompositionResultFieldType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<CompositionMetricPresentation>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCompositionPlan {
    pub composition_revision: CompositionRevisionRef,
    pub variable_values: BTreeMap<String, VariableGraphLiteral>,
    pub queries: Vec<ResolvedQueryRequest>,
    #[serde(default)]
    pub component_bindings: Vec<ResolvedComponentBinding>,
    #[serde(default)]
    pub dependencies: CompositionDependencyIndex,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewCompositionPlan {
    pub draft_fingerprint: String,
    pub variable_values: BTreeMap<String, VariableGraphLiteral>,
    pub queries: Vec<ResolvedQueryRequest>,
    #[serde(default)]
    pub component_bindings: Vec<ResolvedComponentBinding>,
    #[serde(default)]
    pub dependencies: CompositionDependencyIndex,
}

struct ResolvePartsOptions<'a> {
    allow_draft_descriptors: bool,
    allow_unset_inputs: bool,
    selected_query_ids: Option<&'a BTreeSet<String>>,
}

pub fn resolve_composition(
    input: ResolveInput<'_>,
) -> Result<ResolvedCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_with_unset(input, &[])
}

pub fn resolve_composition_with_unset(
    input: ResolveInput<'_>,
    unset: &[String],
) -> Result<ResolvedCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_with_query_selection(input, unset, None)
}

/// Resolve a Composition plan for the selected query variables and their
/// transitive query dependencies. Unselected query metadata is deliberately
/// omitted from the plan; graph structure and component bindings are still
/// validated against the complete graph.
pub fn resolve_composition_with_query_selection(
    input: ResolveInput<'_>,
    unset: &[String],
    selected_query_ids: Option<&BTreeSet<String>>,
) -> Result<ResolvedCompositionPlan, Vec<CompositionDiagnostic>> {
    let (variable_values, queries, component_bindings, dependencies) = resolve_parts(
        input.execution_id,
        input.spec,
        input.variables,
        unset,
        input.current_queries,
        ResolvePartsOptions {
            allow_draft_descriptors: false,
            allow_unset_inputs: false,
            selected_query_ids,
        },
    )?;
    Ok(ResolvedCompositionPlan {
        composition_revision: input.composition_revision,
        variable_values,
        queries,
        component_bindings,
        dependencies,
    })
}

pub fn resolve_composition_preview(
    execution_id: Uuid,
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    current_queries: &[CurrentQueryDescriptor<'_>],
    draft_fingerprint: String,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_preview_with_unset(
        execution_id,
        spec,
        variables,
        &[],
        current_queries,
        draft_fingerprint,
    )
}

pub fn resolve_composition_preview_with_unset(
    execution_id: Uuid,
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    unset: &[String],
    current_queries: &[CurrentQueryDescriptor<'_>],
    draft_fingerprint: String,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_preview_with_query_selection(
        execution_id,
        spec,
        variables,
        unset,
        current_queries,
        draft_fingerprint,
        None,
    )
}

/// Preview a selected query dependency subgraph while validating the full
/// Composition graph and component bindings.
pub fn resolve_composition_preview_with_query_selection(
    execution_id: Uuid,
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    unset: &[String],
    current_queries: &[CurrentQueryDescriptor<'_>],
    draft_fingerprint: String,
    selected_query_ids: Option<&BTreeSet<String>>,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_preview_with_query_selection_and_unset_inputs(
        execution_id,
        spec,
        variables,
        unset,
        current_queries,
        draft_fingerprint,
        ResolvePartsOptions {
            allow_draft_descriptors: true,
            allow_unset_inputs: false,
            selected_query_ids,
        },
    )
}

/// Authoring-only preview that resolves query metadata while Inputs remain
/// unset. Such Inputs stay absent from returned values, and requests that
/// would consume preview placeholders remain unexecutable (`None`).
pub fn resolve_composition_authoring_preview_with_query_selection(
    execution_id: Uuid,
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    unset: &[String],
    current_queries: &[CurrentQueryDescriptor<'_>],
    draft_fingerprint: String,
    selected_query_ids: Option<&BTreeSet<String>>,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    resolve_composition_preview_with_query_selection_and_unset_inputs(
        execution_id,
        spec,
        variables,
        unset,
        current_queries,
        draft_fingerprint,
        ResolvePartsOptions {
            allow_draft_descriptors: true,
            allow_unset_inputs: true,
            selected_query_ids,
        },
    )
}

fn resolve_composition_preview_with_query_selection_and_unset_inputs(
    execution_id: Uuid,
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    unset: &[String],
    current_queries: &[CurrentQueryDescriptor<'_>],
    draft_fingerprint: String,
    options: ResolvePartsOptions<'_>,
) -> Result<PreviewCompositionPlan, Vec<CompositionDiagnostic>> {
    let (variable_values, queries, component_bindings, dependencies) = resolve_parts(
        execution_id,
        spec,
        variables,
        unset,
        current_queries,
        options,
    )?;
    Ok(PreviewCompositionPlan {
        draft_fingerprint,
        variable_values,
        queries,
        component_bindings,
        dependencies,
    })
}

type ResolvedCompositionParts = (
    BTreeMap<String, VariableGraphLiteral>,
    Vec<ResolvedQueryRequest>,
    Vec<ResolvedComponentBinding>,
    CompositionDependencyIndex,
);

fn bind_authoring_preview_inputs(
    variables: &[VariableGraphNode],
    supplied: &BTreeMap<String, Value>,
    unset: &[String],
) -> Result<(VariableGraphBindings, BTreeSet<String>), Vec<CompositionDiagnostic>> {
    let mut preview_variables = variables.to_vec();
    for node in &mut preview_variables {
        if let VariableGraphNodeKind::Input { required, .. } = &mut node.kind {
            *required = false;
        }
    }
    let bindings = bind_variables_with_unset(&preview_variables, supplied, unset);
    if !bindings.diagnostics.is_empty() {
        return Err(bindings.diagnostics);
    }
    let missing_inputs = preview_variables
        .iter()
        .filter(|node| {
            matches!(node.kind, VariableGraphNodeKind::Input { .. })
                && !bindings.values.contains_key(&node.id)
        })
        .map(|node| node.id.clone())
        .collect();
    Ok((bindings, missing_inputs))
}

fn authoring_preview_placeholder(scalar_type: VariableGraphScalarType) -> VariableGraphLiteral {
    match scalar_type {
        VariableGraphScalarType::String => VariableGraphLiteral::String(String::new()),
        VariableGraphScalarType::Boolean => VariableGraphLiteral::Boolean(false),
        VariableGraphScalarType::Integer => VariableGraphLiteral::Integer(0),
        VariableGraphScalarType::Float => VariableGraphLiteral::Float(0.0),
        VariableGraphScalarType::Decimal => VariableGraphLiteral::Decimal("0".to_owned()),
        VariableGraphScalarType::Date => VariableGraphLiteral::Date("1970-01-01".to_owned()),
        VariableGraphScalarType::Timestamp => {
            VariableGraphLiteral::Timestamp("1970-01-01T00:00:00Z".to_owned())
        }
        VariableGraphScalarType::Json => VariableGraphLiteral::Json(Value::Bool(false)),
    }
}

fn queries_blocked_by_unset_inputs(
    graph: &VariableGraph,
    query_order: &[String],
    missing_inputs: &BTreeSet<String>,
) -> BTreeSet<String> {
    let nodes = graph
        .variables
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let mut blocked = BTreeSet::new();
    for variable_id in query_order {
        let Some(node) = nodes.get(variable_id.as_str()) else {
            continue;
        };
        let VariableGraphNodeKind::Query { query, .. } = &node.kind else {
            continue;
        };
        if query.arguments.iter().any(|argument| {
            missing_inputs.contains(&argument.variable) || blocked.contains(&argument.variable)
        }) {
            blocked.insert(variable_id.clone());
        }
    }
    blocked
}

fn resolve_parts(
    execution_id: Uuid,
    spec: &CompositionSpec,
    variables: &BTreeMap<String, Value>,
    unset: &[String],
    current_queries: &[CurrentQueryDescriptor<'_>],
    options: ResolvePartsOptions<'_>,
) -> Result<ResolvedCompositionParts, Vec<CompositionDiagnostic>> {
    let graph = VariableGraph {
        variables: spec.variables.clone(),
    };
    let (bindings, missing_inputs) = if options.allow_unset_inputs {
        bind_authoring_preview_inputs(&spec.variables, variables, unset)?
    } else {
        let requested = options
            .selected_query_ids
            .map(|ids| ids.iter().cloned().collect::<Vec<_>>());
        let bindings =
            bind_variables_for_queries(&spec.variables, variables, unset, requested.as_deref());
        if !bindings.diagnostics.is_empty() {
            return Err(bindings.diagnostics);
        }
        (bindings, BTreeSet::new())
    };
    let render_components = spec.components_in_render_order().map_err(|_| {
        vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]
    })?;
    let query_nodes = spec
        .variables
        .iter()
        .filter(|node| matches!(node.kind, VariableGraphNodeKind::Query { .. }))
        .collect::<Vec<_>>();
    let query_ids = query_nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    if options.selected_query_ids.is_some_and(|selected| {
        selected
            .iter()
            .any(|variable_id| !query_ids.contains(variable_id))
    }) {
        return Err(vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]);
    }
    let query_order = variable_graph_dependency_order(&graph).map_err(|_| {
        vec![CompositionDiagnostic::without_variable(
            CompositionDiagnosticCode::InvalidComposition,
        )]
    })?;
    let blocked_by_unset_inputs =
        queries_blocked_by_unset_inputs(&graph, &query_order, &missing_inputs);
    let mut query_binding_values = bindings.values.clone();
    if options.allow_unset_inputs {
        for node in &spec.variables {
            if missing_inputs.contains(&node.id) {
                if let VariableGraphNodeKind::Input { value_type, .. } = &node.kind {
                    query_binding_values.insert(
                        node.id.clone(),
                        authoring_preview_placeholder(value_type.scalar_type),
                    );
                }
            }
        }
    }
    let mut current = BTreeMap::new();
    for descriptor in current_queries {
        if !query_ids.contains(descriptor.variable_id())
            || current
                .insert(descriptor.variable_id(), descriptor)
                .is_some()
        {
            return Err(vec![CompositionDiagnostic::without_variable(
                CompositionDiagnosticCode::InvalidComposition,
            )]);
        }
    }

    let mut execution_graph = graph.clone();
    let mut derived = BTreeMap::<
        String,
        (
            VariableGraphResultType,
            Vec<VariableGraphFormBinding>,
            Vec<VariableGraphColumn>,
            String,
            String,
            Option<String>,
        ),
    >::new();
    let mut diagnostics = Vec::new();
    for variable_id in query_order {
        // Earlier query nodes may have had their draft return descriptor
        // replaced with planner-derived metadata. Snapshot the current scalar
        // types before mutably borrowing the node being resolved so downstream
        // Saved SQL argument bindings can be checked against the actual graph.
        let variable_scalar_types = execution_graph
            .variables
            .iter()
            .filter_map(|variable| {
                scalar_descriptor(&variable.kind)
                    .or_else(|| query_scalar_descriptor(&variable.kind))
                    .map(|descriptor| (variable.id.clone(), descriptor))
            })
            .collect::<BTreeMap<_, _>>();
        let Some(node) = execution_graph
            .variables
            .iter_mut()
            .find(|node| node.id == variable_id)
        else {
            continue;
        };
        let VariableGraphNodeKind::Query { query, returns } = &mut node.kind else {
            continue;
        };
        if options
            .selected_query_ids
            .is_some_and(|selected| !selected.contains(&node.id))
        {
            continue;
        }
        let Some(descriptor) = current.get(node.id.as_str()).copied() else {
            diagnostics.push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::QueryUnavailable,
                &node.id,
            ));
            continue;
        };
        match (&query.definition, descriptor) {
            (
                VariableGraphQueryDefinition::InlineSql { .. },
                CurrentQueryDescriptor::InlineSql {
                    metadata: Some(metadata),
                    ..
                },
            ) => {
                if !schema_is_well_formed(&metadata.result_columns, &metadata.schema_fingerprint)
                    || metadata.bound_sql.trim().is_empty()
                {
                    diagnostics.push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::QueryUnavailable,
                        &node.id,
                    ));
                    continue;
                }
                // Preview is an authorized planner boundary. Treat every
                // client-supplied descriptor as untrusted, including nonempty
                // or forged values, and replace it with planner output.
                if !options.allow_draft_descriptors
                    && (query.form_bindings != metadata.form_bindings
                        || query.schema_fingerprint != metadata.schema_fingerprint)
                {
                    diagnostics.push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::QuerySchemaChanged,
                        &node.id,
                    ));
                    continue;
                }
                let draft_returns = options.allow_draft_descriptors;
                if !returns_match_schema(returns, &metadata.result_columns) && !draft_returns {
                    diagnostics.push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::QuerySchemaChanged,
                        &node.id,
                    ));
                    continue;
                }
                if draft_returns {
                    match returns_from_schema(returns, &metadata.result_columns) {
                        Some(inferred) => *returns = inferred,
                        None => {
                            diagnostics.push(CompositionDiagnostic::for_variable(
                                CompositionDiagnosticCode::QuerySchemaChanged,
                                &node.id,
                            ));
                            continue;
                        }
                    }
                }
                if let VariableGraphQueryDefinition::InlineSql { sql } = &mut query.definition {
                    *sql = metadata.bound_sql.clone();
                }
                query.form_bindings = metadata.form_bindings.clone();
                query.schema_fingerprint = metadata.schema_fingerprint.clone();
                let returns = returns.clone();
                let fingerprint = query_semantic_fingerprint(
                    &query.definition,
                    &query.arguments,
                    &metadata.form_bindings,
                    &returns,
                    &metadata.result_columns,
                    &metadata.schema_fingerprint,
                )
                .map_err(|_| {
                    vec![CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                        &node.id,
                    )]
                })?;
                derived.insert(
                    node.id.clone(),
                    (
                        returns,
                        metadata.form_bindings.clone(),
                        metadata.result_columns.clone(),
                        metadata.schema_fingerprint.clone(),
                        fingerprint,
                        None,
                    ),
                );
            }
            (
                VariableGraphQueryDefinition::SavedSql {
                    entry_id: Some(entry_id),
                    revision_id: Some(revision_id),
                    ..
                },
                CurrentQueryDescriptor::SavedSql {
                    metadata: Some(metadata),
                    ..
                },
            ) => {
                if &metadata.entry_id != entry_id
                    || &metadata.revision_id != revision_id
                    || !schema_is_well_formed(
                        &metadata.result_columns,
                        &metadata.schema_fingerprint,
                    )
                {
                    diagnostics.push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::QueryUnavailable,
                        &node.id,
                    ));
                    continue;
                }
                // Saved SQL metadata is also re-derived for each preview;
                // client descriptors never establish a result contract.
                if !options.allow_draft_descriptors
                    && (query.form_bindings != metadata.form_bindings
                        || query.schema_fingerprint != metadata.schema_fingerprint)
                {
                    diagnostics.push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::QuerySchemaChanged,
                        &node.id,
                    ));
                    continue;
                }
                let draft_returns = options.allow_draft_descriptors;
                if (!returns_match_schema(returns, &metadata.result_columns) && !draft_returns)
                    || !saved_arguments_match(
                        &query.arguments,
                        &metadata.parameter_types,
                        &variable_scalar_types,
                    )
                {
                    diagnostics.push(CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::QuerySchemaChanged,
                        &node.id,
                    ));
                    continue;
                }
                if draft_returns {
                    match returns_from_schema(returns, &metadata.result_columns) {
                        Some(inferred) => *returns = inferred,
                        None => {
                            diagnostics.push(CompositionDiagnostic::for_variable(
                                CompositionDiagnosticCode::QuerySchemaChanged,
                                &node.id,
                            ));
                            continue;
                        }
                    }
                }
                query.form_bindings = metadata.form_bindings.clone();
                query.schema_fingerprint = metadata.schema_fingerprint.clone();
                let returns = returns.clone();
                let fingerprint = query_semantic_fingerprint(
                    &query.definition,
                    &query.arguments,
                    &metadata.form_bindings,
                    &returns,
                    &metadata.result_columns,
                    &metadata.schema_fingerprint,
                )
                .map_err(|_| {
                    vec![CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                        &node.id,
                    )]
                })?;
                derived.insert(
                    node.id.clone(),
                    (
                        returns,
                        metadata.form_bindings.clone(),
                        metadata.result_columns.clone(),
                        metadata.schema_fingerprint.clone(),
                        fingerprint,
                        Some(metadata.sql_text.clone()),
                    ),
                );
            }
            _ => diagnostics.push(CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::QueryUnavailable,
                &node.id,
            )),
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let requested = query_nodes
        .iter()
        .filter(|node| {
            options
                .selected_query_ids
                .is_none_or(|selected| selected.contains(&node.id))
        })
        .map(|node| node.id.clone())
        .collect::<Vec<_>>();
    let request_batch = crate::composition_query::compile_variable_graph_query_request_batch(
        &execution_graph,
        &requested,
        execution_id,
        &query_binding_values,
    )
    .map_err(|errors| {
        errors
            .into_iter()
            .map(|error| match error.code {
                crate::composition_query::VariableGraphQueryRequestDiagnosticCode::MissingArgumentValue => {
                    CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::VariableMissing,
                        error.reference_id.as_deref().unwrap_or(&error.variable_id),
                    )
                }
                _ => CompositionDiagnostic::for_variable(
                    CompositionDiagnosticCode::VariableTypeMismatch,
                    &error.variable_id,
                ),
            })
            .collect::<Vec<_>>()
    })?;
    let queries = request_batch
        .into_iter()
        .map(|item| {
            let (
                returns,
                form_bindings,
                result_columns,
                schema_fingerprint,
                query_semantic_fingerprint,
                sql_text,
            ) = derived
                .remove(&item.variable_id)
                .expect("every query descriptor was derived");
            let request = if blocked_by_unset_inputs.contains(&item.variable_id) {
                None
            } else {
                item.request
            };
            ResolvedQueryRequest {
                variable_id: item.variable_id,
                request,
                sql_text,
                portable_sql: None,
                deferred_arguments: item.deferred_arguments,
                scalar_cardinality: matches!(returns, VariableGraphResultType::Scalar { .. })
                    .then_some(CompositionScalarCardinality::ExactlyOneRow),
                returns,
                result_columns,
                form_bindings,
                schema_fingerprint,
                query_semantic_fingerprint,
            }
        })
        .collect::<Vec<_>>();

    let mut dependencies = CompositionDependencyIndex {
        variable_consumers: spec
            .variables
            .iter()
            .map(|node| (node.id.clone(), VariableConsumers::default()))
            .collect(),
    };
    for node in &spec.variables {
        if let VariableGraphNodeKind::Query { query, .. } = &node.kind {
            for argument in &query.arguments {
                if let Some(consumers) = dependencies.variable_consumers.get_mut(&argument.variable)
                {
                    consumers.query_variable_ids.insert(node.id.clone());
                }
            }
        }
    }
    let mut component_bindings = Vec::with_capacity(render_components.len());
    let nodes = execution_graph
        .variables
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    for component in render_components {
        let component_id = component.id();
        let variable_id = component.variable_id();
        let binding = match component {
            CompositionComponent::Text {
                content: CompositionTextContent::Literal { .. },
                ..
            } => ResolvedComponentBinding {
                component_id: component_id.to_owned(),
                kind: ResolvedComponentKind::Text,
                variable_id: None,
                expected_result_type: None,
                presentation: None,
            },
            CompositionComponent::Text {
                content: CompositionTextContent::Variable { variable },
                ..
            } => {
                if let Some(consumers) = dependencies.variable_consumers.get_mut(variable) {
                    consumers.text_component_ids.insert(component_id.to_owned());
                }
                ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Text,
                    variable_id: Some(variable.clone()),
                    expected_result_type: None,
                    presentation: None,
                }
            }
            CompositionComponent::Input { variable, .. } => {
                if let Some(consumers) = dependencies.variable_consumers.get_mut(variable) {
                    consumers
                        .input_component_ids
                        .insert(component_id.to_owned());
                }
                ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Input,
                    variable_id: Some(variable.clone()),
                    expected_result_type: None,
                    presentation: None,
                }
            }
            CompositionComponent::Metric {
                variable,
                presentation,
                ..
            } => {
                let node = nodes.get(variable.as_str()).copied().ok_or_else(|| {
                    vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]
                })?;
                let descriptor = scalar_descriptor(&node.kind)
                    .or_else(|| query_scalar_descriptor(&node.kind))
                    .ok_or_else(|| {
                        vec![CompositionDiagnostic::for_variable(
                            CompositionDiagnosticCode::MetricResultNotScalar,
                            variable,
                        )]
                    })?;
                if descriptor.scalar_type == VariableGraphScalarType::Json
                    || (presentation.is_some()
                        && !matches!(
                            descriptor.scalar_type,
                            VariableGraphScalarType::Integer
                                | VariableGraphScalarType::Float
                                | VariableGraphScalarType::Decimal
                        ))
                {
                    return Err(vec![CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::MetricResultNotScalar,
                        variable,
                    )]);
                }
                if let Some(consumers) = dependencies.variable_consumers.get_mut(variable) {
                    consumers
                        .metric_component_ids
                        .insert(component_id.to_owned());
                }
                ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Metric,
                    variable_id: Some(variable.clone()),
                    expected_result_type: Some(result_field_type(descriptor.scalar_type)),
                    presentation: presentation.clone(),
                }
            }
            CompositionComponent::Table { variable, .. } => {
                let node = nodes.get(variable.as_str()).copied().ok_or_else(|| {
                    vec![CompositionDiagnostic::without_variable(
                        CompositionDiagnosticCode::InvalidComposition,
                    )]
                })?;
                if !matches!(
                    node.kind,
                    VariableGraphNodeKind::Query {
                        returns: VariableGraphResultType::Table { .. },
                        ..
                    }
                ) {
                    return Err(vec![CompositionDiagnostic::for_variable(
                        CompositionDiagnosticCode::MetricResultNotScalar,
                        variable,
                    )]);
                }
                if let Some(consumers) = dependencies.variable_consumers.get_mut(variable) {
                    consumers
                        .table_component_ids
                        .insert(component_id.to_owned());
                }
                ResolvedComponentBinding {
                    component_id: component_id.to_owned(),
                    kind: ResolvedComponentKind::Tabular,
                    variable_id: variable_id.map(str::to_owned),
                    expected_result_type: None,
                    presentation: None,
                }
            }
        };
        component_bindings.push(binding);
    }
    Ok((bindings.values, queries, component_bindings, dependencies))
}

fn scalar_descriptor(kind: &VariableGraphNodeKind) -> Option<VariableGraphScalarDescriptor> {
    match kind {
        VariableGraphNodeKind::Constant { value_type, .. }
        | VariableGraphNodeKind::Input { value_type, .. } => Some(*value_type),
        _ => None,
    }
}

fn query_scalar_descriptor(kind: &VariableGraphNodeKind) -> Option<VariableGraphScalarDescriptor> {
    match kind {
        VariableGraphNodeKind::Query {
            returns: VariableGraphResultType::Scalar { value_type },
            ..
        } => Some(*value_type),
        _ => None,
    }
}

fn result_field_type(value_type: VariableGraphScalarType) -> CompositionResultFieldType {
    match value_type {
        VariableGraphScalarType::String => CompositionResultFieldType::String,
        VariableGraphScalarType::Boolean => CompositionResultFieldType::Boolean,
        VariableGraphScalarType::Integer => CompositionResultFieldType::Integer,
        VariableGraphScalarType::Float => CompositionResultFieldType::Float,
        VariableGraphScalarType::Decimal => CompositionResultFieldType::Decimal,
        VariableGraphScalarType::Date => CompositionResultFieldType::Date,
        VariableGraphScalarType::Timestamp => CompositionResultFieldType::Timestamp,
        VariableGraphScalarType::Json => CompositionResultFieldType::Json,
    }
}

fn saved_arguments_match(
    arguments: &[ugoite_domain::composition::VariableGraphArgumentBinding],
    parameter_types: &BTreeMap<String, VariableGraphScalarDescriptor>,
    variable_types: &BTreeMap<String, VariableGraphScalarDescriptor>,
) -> bool {
    if arguments.len() != parameter_types.len() {
        return false;
    }

    let mut seen = BTreeSet::new();
    arguments.iter().all(|argument| {
        if !seen.insert(argument.name.as_str()) {
            return false;
        }
        let Some(expected_type) = parameter_types.get(&argument.name) else {
            return false;
        };
        let Some(actual_type) = variable_types.get(&argument.variable) else {
            return false;
        };
        actual_type.scalar_type == expected_type.scalar_type
    })
}

/// Check whether a declared query result contract exactly matches planner
/// output columns, including scalar nullability and table column order.
pub fn returns_match_schema(
    returns: &VariableGraphResultType,
    columns: &[VariableGraphColumn],
) -> bool {
    match returns {
        VariableGraphResultType::Scalar { value_type } => {
            columns.len() == 1 && columns[0].value_type == *value_type
        }
        VariableGraphResultType::Table { columns: expected } => expected == columns,
    }
}

/// Derive a query result contract from planner output while preserving the
/// selected scalar or table result shape. Scalar queries require exactly one
/// planner column.
pub fn returns_from_schema(
    intent: &VariableGraphResultType,
    columns: &[VariableGraphColumn],
) -> Option<VariableGraphResultType> {
    match intent {
        VariableGraphResultType::Scalar { .. } if columns.len() == 1 => {
            Some(VariableGraphResultType::Scalar {
                value_type: columns[0].value_type,
            })
        }
        VariableGraphResultType::Table { .. } => Some(VariableGraphResultType::Table {
            columns: columns.to_vec(),
        }),
        _ => None,
    }
}

fn query_semantic_fingerprint(
    definition: &VariableGraphQueryDefinition,
    arguments: &[ugoite_domain::composition::VariableGraphArgumentBinding],
    form_bindings: &[VariableGraphFormBinding],
    returns: &VariableGraphResultType,
    result_columns: &[VariableGraphColumn],
    schema_fingerprint: &str,
) -> Result<String, serde_json::Error> {
    let mut arguments = arguments.to_vec();
    arguments.sort_by(|left, right| left.name.cmp(&right.name));
    let descriptor = serde_json::to_vec(&(
        definition,
        arguments,
        form_bindings,
        returns,
        result_columns,
        schema_fingerprint,
    ))?;
    Ok(hex::encode(Sha256::digest(descriptor)))
}

fn schema_is_well_formed(columns: &[VariableGraphColumn], fingerprint: &str) -> bool {
    !columns.is_empty()
        && columns.len() <= MAX_SQL_OUTPUT_COLUMNS
        && columns.iter().all(|column| {
            !column.name.trim().is_empty() && column.name.len() <= MAX_SQL_COLUMN_NAME_BYTES
        })
        && columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            == columns.len()
        && fingerprint.len() == 64
        && fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_domain::composition::{
        CompositionComponent, DashboardFlowLayout, FlowItem, FlowLayoutKind, FlowRow,
        VariableGraphArgumentBinding, VariableGraphQuery,
    };

    #[test]
    fn query_semantic_fingerprint_includes_scalar_or_table_return_shape() {
        let definition = VariableGraphQueryDefinition::InlineSql {
            sql: "SELECT 1 AS value".to_owned(),
        };
        let descriptor = VariableGraphScalarDescriptor {
            scalar_type: VariableGraphScalarType::Integer,
            nullable: false,
        };
        let columns = vec![VariableGraphColumn {
            name: "value".to_owned(),
            value_type: descriptor,
        }];
        let scalar = VariableGraphResultType::Scalar {
            value_type: descriptor,
        };
        let table = VariableGraphResultType::Table {
            columns: columns.clone(),
        };
        let args = [];
        let bindings = [];
        let schema_fingerprint = "0".repeat(64);

        let scalar_fingerprint = query_semantic_fingerprint(
            &definition,
            &args,
            &bindings,
            &scalar,
            &columns,
            &schema_fingerprint,
        )
        .expect("scalar semantic fingerprint");
        let table_fingerprint = query_semantic_fingerprint(
            &definition,
            &args,
            &bindings,
            &table,
            &columns,
            &schema_fingerprint,
        )
        .expect("table semantic fingerprint");

        assert_ne!(scalar_fingerprint, table_fingerprint);
    }

    #[test]
    fn saved_sql_preview_rejects_mismatched_argument_type_and_accepts_matching_type() {
        let integer = VariableGraphScalarDescriptor {
            scalar_type: VariableGraphScalarType::Integer,
            nullable: false,
        };
        let string = VariableGraphScalarDescriptor {
            scalar_type: VariableGraphScalarType::String,
            nullable: false,
        };
        let spec = CompositionSpec {
            variables: vec![
                VariableGraphNode {
                    id: "minimum".to_owned(),
                    kind: VariableGraphNodeKind::Constant {
                        value_type: string,
                        value: VariableGraphLiteral::String("10".to_owned()),
                    },
                },
                VariableGraphNode {
                    id: "count".to_owned(),
                    kind: VariableGraphNodeKind::Query {
                        returns: VariableGraphResultType::Scalar {
                            value_type: integer,
                        },
                        query: VariableGraphQuery {
                            definition: VariableGraphQueryDefinition::SavedSql {
                                entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
                                revision_id: Some(
                                    "00000000-0000-0000-0000-000000000002".to_owned(),
                                ),
                                column: None,
                            },
                            arguments: vec![VariableGraphArgumentBinding {
                                name: "minimum".to_owned(),
                                variable: "minimum".to_owned(),
                            }],
                            form_bindings: Vec::new(),
                            schema_fingerprint: String::new(),
                        },
                    },
                },
            ],
            components: vec![CompositionComponent::Metric {
                id: "count_metric".to_owned(),
                variable: "count".to_owned(),
                presentation: None,
            }],
            layout: DashboardFlowLayout {
                kind: FlowLayoutKind::Flow,
                rows: vec![FlowRow {
                    id: "row".to_owned(),
                    items: vec![FlowItem::Component {
                        component: "count_metric".to_owned(),
                    }],
                }],
            },
        };
        let metadata = SavedSqlRevisionMetadata {
            entry_id: "00000000-0000-0000-0000-000000000001".to_owned(),
            revision_id: "00000000-0000-0000-0000-000000000002".to_owned(),
            sql_text: "SELECT {{minimum}} AS value".to_owned(),
            parameter_types: BTreeMap::from([("minimum".to_owned(), integer)]),
            form_bindings: Vec::new(),
            result_columns: vec![VariableGraphColumn {
                name: "value".to_owned(),
                value_type: integer,
            }],
            schema_fingerprint: "0".repeat(64),
        };
        let current_queries = [CurrentQueryDescriptor::SavedSql {
            variable_id: "count",
            metadata: Some(&metadata),
        }];

        let mismatched = resolve_composition_preview(
            Uuid::nil(),
            &spec,
            &BTreeMap::new(),
            &current_queries,
            "draft".to_owned(),
        )
        .expect_err("a string binding must not satisfy the pinned integer parameter");
        assert_eq!(
            mismatched,
            vec![CompositionDiagnostic::for_variable(
                CompositionDiagnosticCode::QuerySchemaChanged,
                "count",
            )]
        );

        let mut matching_spec = spec.clone();
        matching_spec.variables[0].kind = VariableGraphNodeKind::Constant {
            value_type: integer,
            value: VariableGraphLiteral::Integer(10),
        };
        assert!(resolve_composition_preview(
            Uuid::nil(),
            &matching_spec,
            &BTreeMap::new(),
            &current_queries,
            "draft".to_owned(),
        )
        .is_ok());

        // A downstream Saved SQL query must see the planner-derived type of
        // an upstream scalar query, even when that query's draft descriptor
        // was stale.
        let mut planner_derived_spec = spec;
        planner_derived_spec.variables[0].kind = VariableGraphNodeKind::Query {
            returns: VariableGraphResultType::Scalar { value_type: string },
            query: VariableGraphQuery {
                definition: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT 10 AS value".to_owned(),
                },
                arguments: Vec::new(),
                form_bindings: Vec::new(),
                schema_fingerprint: String::new(),
            },
        };
        // The document order must not replace dependency order during preview.
        planner_derived_spec.variables.swap(0, 1);
        let inline_metadata = InlineSqlQueryMetadata {
            bound_sql: "SELECT 10 AS value".to_owned(),
            form_bindings: Vec::new(),
            parameter_names: BTreeSet::new(),
            result_columns: vec![VariableGraphColumn {
                name: "value".to_owned(),
                value_type: integer,
            }],
            schema_fingerprint: "a".repeat(64),
        };
        let planner_derived_queries = [
            CurrentQueryDescriptor::InlineSql {
                variable_id: "minimum",
                metadata: Some(&inline_metadata),
            },
            CurrentQueryDescriptor::SavedSql {
                variable_id: "count",
                metadata: Some(&metadata),
            },
        ];
        assert!(resolve_composition_preview(
            Uuid::nil(),
            &planner_derived_spec,
            &BTreeMap::new(),
            &planner_derived_queries,
            "draft".to_owned(),
        )
        .is_ok());
    }
}
