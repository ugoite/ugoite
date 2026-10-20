//! Pure compilation from a validated variable graph to the existing stateless
//! SQL request contract.
//!
//! This module performs no storage reads or authorization. Callers resolve
//! the current values first and the ordinary SQL execution path still checks
//! access to the Space, Forms, and exact Saved SQL revision.

use crate::sql_query::{
    SavedSqlRevisionRef, SqlQueryPage, SqlQueryRequest, SqlResultColumnType, MAX_SQL_QUERY_BYTES,
    MAX_SQL_SCALAR_PROBE_ROWS,
};
use chrono::{DateTime, Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_domain::composition::{
    compile_variable_graph, validate_variable_graph_result, VariableGraph,
    VariableGraphArgumentBinding, VariableGraphColumn, VariableGraphDiagnostic,
    VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind, VariableGraphQueryDefinition,
    VariableGraphResultDiagnostic, VariableGraphResultType, VariableGraphScalarDescriptor,
    VariableGraphScalarType, VariableGraphValidatedResult,
};
use ugoite_domain::sql_document::{parse_sql_document, SqlDocumentStatus};
use uuid::Uuid;

/// The default bounded page size for one variable-graph table query.
pub const DEFAULT_VARIABLE_GRAPH_SQL_PAGE_LIMIT: usize = 100;

/// Stable diagnostics produced while binding one variable-graph query.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum VariableGraphQueryRequestDiagnosticCode {
    InvalidGraph,
    QueryVariableNotFound,
    QueryNodeRequired,
    InvalidInlineSql,
    InvalidSavedSqlIdentity,
    InvalidSavedSqlColumn,
    InvalidArgumentName,
    DuplicateArgumentName,
    MissingArgumentValue,
    ArgumentTypeMismatch,
    InvalidArgumentValue,
    UnsupportedArgumentType,
    RequestOutOfBounds,
}

impl VariableGraphQueryRequestDiagnosticCode {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidGraph => "invalid_graph",
            Self::QueryVariableNotFound => "query_variable_not_found",
            Self::QueryNodeRequired => "query_node_required",
            Self::InvalidInlineSql => "invalid_inline_sql",
            Self::InvalidSavedSqlIdentity => "invalid_saved_sql_identity",
            Self::InvalidSavedSqlColumn => "invalid_saved_sql_column",
            Self::InvalidArgumentName => "invalid_argument_name",
            Self::DuplicateArgumentName => "duplicate_argument_name",
            Self::MissingArgumentValue => "missing_argument_value",
            Self::ArgumentTypeMismatch => "argument_type_mismatch",
            Self::InvalidArgumentValue => "invalid_argument_value",
            Self::UnsupportedArgumentType => "unsupported_argument_type",
            Self::RequestOutOfBounds => "request_out_of_bounds",
        }
    }
}

/// Stable diagnostics raised while checking one typed SQL page against a
/// query variable's declared return contract.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableGraphQueryResultDiagnosticCode {
    QueryVariableNotFound,
    QueryNodeRequired,
    RequestSourceMismatch,
    ExecutionMismatch,
    ResultSchemaMissing,
    ResultSchemaMismatch,
    InvalidResultRow,
    SavedSqlColumnMissing,
    SavedSqlColumnAmbiguous,
    ScalarResultIncomplete,
    ResultContractMismatch,
}

impl VariableGraphQueryResultDiagnosticCode {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueryVariableNotFound => "query_variable_not_found",
            Self::QueryNodeRequired => "query_node_required",
            Self::RequestSourceMismatch => "request_source_mismatch",
            Self::ExecutionMismatch => "execution_mismatch",
            Self::ResultSchemaMissing => "result_schema_missing",
            Self::ResultSchemaMismatch => "result_schema_mismatch",
            Self::InvalidResultRow => "invalid_result_row",
            Self::SavedSqlColumnMissing => "saved_sql_column_missing",
            Self::SavedSqlColumnAmbiguous => "saved_sql_column_ambiguous",
            Self::ScalarResultIncomplete => "scalar_result_incomplete",
            Self::ResultContractMismatch => "result_contract_mismatch",
        }
    }
}

/// A query-result diagnostic scoped to its graph variable.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphQueryResultDiagnostic {
    pub code: VariableGraphQueryResultDiagnosticCode,
    pub variable_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column_name: Option<String>,
    /// Detailed stable Domain diagnostic for a declared descriptor mismatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_diagnostic: Option<VariableGraphResultDiagnostic>,
}

impl VariableGraphQueryResultDiagnostic {
    fn for_node(code: VariableGraphQueryResultDiagnosticCode, variable_id: &str) -> Self {
        Self {
            code,
            variable_id: variable_id.to_owned(),
            column_name: None,
            result_diagnostic: None,
        }
    }

    fn for_column(
        code: VariableGraphQueryResultDiagnosticCode,
        variable_id: &str,
        column_name: &str,
    ) -> Self {
        Self {
            code,
            variable_id: variable_id.to_owned(),
            column_name: Some(column_name.to_owned()),
            result_diagnostic: None,
        }
    }

    fn for_contract(variable_id: &str, result_diagnostic: VariableGraphResultDiagnostic) -> Self {
        Self {
            code: VariableGraphQueryResultDiagnosticCode::ResultContractMismatch,
            variable_id: variable_id.to_owned(),
            column_name: result_diagnostic.column_name.clone(),
            result_diagnostic: Some(result_diagnostic),
        }
    }
}

/// A query-binding diagnostic with only the relevant graph identities.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct VariableGraphQueryRequestDiagnostic {
    pub code: VariableGraphQueryRequestDiagnosticCode,
    /// The graph node relevant to this diagnostic.
    pub variable_id: String,
    pub argument_name: Option<String>,
    /// The resolved graph value required by this SQL argument.
    pub reference_id: Option<String>,
    /// Original stable Domain diagnostic code when graph validation fails.
    pub domain_code: Option<String>,
    /// Dependency cycle from Domain validation, when applicable.
    pub cycle: Vec<String>,
}

impl VariableGraphQueryRequestDiagnostic {
    fn for_node(code: VariableGraphQueryRequestDiagnosticCode, query_id: &str) -> Self {
        Self {
            code,
            variable_id: query_id.to_owned(),
            argument_name: None,
            reference_id: None,
            domain_code: None,
            cycle: Vec::new(),
        }
    }

    fn for_argument(
        code: VariableGraphQueryRequestDiagnosticCode,
        query_id: &str,
        argument: &VariableGraphArgumentBinding,
    ) -> Self {
        Self {
            code,
            variable_id: query_id.to_owned(),
            argument_name: Some(argument.name.clone()),
            reference_id: Some(argument.variable.clone()),
            domain_code: None,
            cycle: Vec::new(),
        }
    }

    fn from_domain(diagnostic: VariableGraphDiagnostic, query_id: &str) -> Self {
        Self {
            code: VariableGraphQueryRequestDiagnosticCode::InvalidGraph,
            variable_id: diagnostic
                .variable_id
                .unwrap_or_else(|| query_id.to_owned()),
            argument_name: diagnostic.argument_name,
            reference_id: diagnostic.reference_id,
            domain_code: Some(diagnostic.code.as_str().to_owned()),
            cycle: diagnostic.cycle,
        }
    }
}

/// One request in a deterministically ordered variable-graph query batch.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableGraphQueryRequestBatchItem {
    /// The query variable whose request is being prepared.
    pub variable_id: String,
    /// A ready stateless request, when all argument values are already resolved.
    ///
    /// This is `None` when a selected query variable supplies one of this
    /// query's arguments. Compile it again with
    /// [`compile_variable_graph_query_request`] after those dependencies have
    /// produced their values. The batch never embeds their previous values.
    pub request: Option<SqlQueryRequest>,
    /// Selected query results that must be resolved before this request can be
    /// compiled. These are listed in query argument binding order.
    pub deferred_arguments: Vec<VariableGraphQueryRequestDeferredArgument>,
}

/// One SQL argument that waits for a selected query variable to be evaluated.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphQueryRequestDeferredArgument {
    /// SQL parameter name declared by the dependent query.
    pub argument_name: String,
    /// Selected query variable whose fresh result supplies this argument.
    pub variable_id: String,
    /// Type expected by the dependent query's SQL parameter.
    pub value_type: VariableGraphScalarDescriptor,
}

/// Stable failures while compiling and accepting one Composition query result
/// into an evaluation snapshot.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
pub enum VariableGraphQueryEvaluationDiagnosticCode {
    InvalidGraph,
    UnexpectedInputValue,
    MissingInputValue,
    InvalidInputValue,
    QueryVariableNotFound,
    QueryNodeRequired,
    QueryDependencyNotReady,
    QueryAlreadyEvaluated,
    RequestCompilationFailed,
    RequestSnapshotMismatch,
    RequestDoesNotMatchGraph,
    MalformedPageContinuation,
    RepeatedPageContinuation,
    ResultValidationFailed,
    QueryResultMissing,
}

impl VariableGraphQueryEvaluationDiagnosticCode {
    /// Stable machine-readable spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidGraph => "invalid_graph",
            Self::UnexpectedInputValue => "unexpected_input_value",
            Self::MissingInputValue => "missing_input_value",
            Self::InvalidInputValue => "invalid_input_value",
            Self::QueryVariableNotFound => "query_variable_not_found",
            Self::QueryNodeRequired => "query_node_required",
            Self::QueryDependencyNotReady => "query_dependency_not_ready",
            Self::QueryAlreadyEvaluated => "query_already_evaluated",
            Self::RequestCompilationFailed => "request_compilation_failed",
            Self::RequestSnapshotMismatch => "request_snapshot_mismatch",
            Self::RequestDoesNotMatchGraph => "request_does_not_match_graph",
            Self::MalformedPageContinuation => "malformed_page_continuation",
            Self::RepeatedPageContinuation => "repeated_page_continuation",
            Self::ResultValidationFailed => "result_validation_failed",
            Self::QueryResultMissing => "query_result_missing",
        }
    }
}

/// A query-evaluation diagnostic with the originating request or result error
/// when one exists.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct VariableGraphQueryEvaluationDiagnostic {
    pub code: VariableGraphQueryEvaluationDiagnosticCode,
    pub variable_id: String,
    pub request_diagnostic: Option<VariableGraphQueryRequestDiagnostic>,
    pub result_diagnostic: Option<VariableGraphQueryResultDiagnostic>,
}

impl VariableGraphQueryEvaluationDiagnostic {
    fn for_variable(code: VariableGraphQueryEvaluationDiagnosticCode, variable_id: &str) -> Self {
        Self {
            code,
            variable_id: variable_id.to_owned(),
            request_diagnostic: None,
            result_diagnostic: None,
        }
    }

    fn from_request(diagnostic: VariableGraphQueryRequestDiagnostic) -> Self {
        Self {
            code: VariableGraphQueryEvaluationDiagnosticCode::RequestCompilationFailed,
            variable_id: diagnostic.variable_id.clone(),
            request_diagnostic: Some(diagnostic),
            result_diagnostic: None,
        }
    }

    fn from_result(diagnostic: VariableGraphQueryResultDiagnostic) -> Self {
        Self {
            code: VariableGraphQueryEvaluationDiagnosticCode::ResultValidationFailed,
            variable_id: diagnostic.variable_id.clone(),
            request_diagnostic: None,
            result_diagnostic: Some(diagnostic),
        }
    }
}

/// Transient Core state that accepts only query results validated against the
/// graph's return descriptors. Raw SQL pages are never retained or exposed as
/// preview values. The caller supplies one execution ID for the immutable
/// evaluation snapshot and passes only the request actually sent to SQL.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableGraphQueryEvaluation {
    graph: VariableGraph,
    execution_id: Uuid,
    topological_order: Vec<String>,
    dependencies: BTreeMap<String, Vec<String>>,
    resolved_scalar_values: BTreeMap<String, VariableGraphLiteral>,
    validated_results: BTreeMap<String, VariableGraphQueryPreviewValue>,
}

/// Complete, transient query values eligible for a Composition preview.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableGraphQueryPreview {
    pub execution_id: Uuid,
    pub query_results: BTreeMap<String, VariableGraphQueryPreviewValue>,
}

/// One descriptor-validated SQL result page eligible for rendering.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableGraphQueryPreviewValue {
    pub result: VariableGraphValidatedResult,
    pub has_more: bool,
    pub next: Option<String>,
}

impl VariableGraphQueryEvaluation {
    /// Start an evaluation with the snapshot's input values. Constants and
    /// declared input defaults are taken from the validated graph. Supplied
    /// values may name input variables only; query values can enter the state
    /// only after their SQL page has passed result validation.
    pub fn new(
        graph: &VariableGraph,
        execution_id: Uuid,
        input_values: &BTreeMap<String, VariableGraphLiteral>,
    ) -> Result<Self, Vec<VariableGraphQueryEvaluationDiagnostic>> {
        let compiled = compile_variable_graph(graph).map_err(|diagnostics| {
            map_graph_diagnostics(diagnostics, "")
                .into_iter()
                .map(|diagnostic| VariableGraphQueryEvaluationDiagnostic {
                    code: VariableGraphQueryEvaluationDiagnosticCode::InvalidGraph,
                    variable_id: diagnostic.variable_id.clone(),
                    request_diagnostic: Some(diagnostic),
                    result_diagnostic: None,
                })
                .collect::<Vec<_>>()
        })?;

        let nodes = graph
            .variables
            .iter()
            .map(|node| (node.id.as_str(), node))
            .collect::<BTreeMap<_, _>>();
        let mut diagnostics = Vec::new();
        for variable_id in input_values.keys() {
            if !matches!(
                nodes.get(variable_id.as_str()).map(|node| &node.kind),
                Some(VariableGraphNodeKind::Input { .. })
            ) {
                diagnostics.push(VariableGraphQueryEvaluationDiagnostic::for_variable(
                    VariableGraphQueryEvaluationDiagnosticCode::UnexpectedInputValue,
                    variable_id,
                ));
            }
        }

        let mut resolved_scalar_values = BTreeMap::new();
        for node in &graph.variables {
            let value = match &node.kind {
                VariableGraphNodeKind::Constant { value, .. } => Some(value.clone()),
                VariableGraphNodeKind::Input {
                    required, default, ..
                } => input_values
                    .get(&node.id)
                    .cloned()
                    .or_else(|| default.clone())
                    .or_else(|| {
                        if *required {
                            diagnostics.push(VariableGraphQueryEvaluationDiagnostic::for_variable(
                                VariableGraphQueryEvaluationDiagnosticCode::MissingInputValue,
                                &node.id,
                            ));
                        }
                        None
                    }),
                VariableGraphNodeKind::Query { .. } => None,
            };
            if let Some(value) = value {
                let descriptor = scalar_descriptor_of(&node.kind)
                    .expect("constant and input variables have scalar descriptors");
                if !literal_matches_descriptor(descriptor, &value) {
                    diagnostics.push(VariableGraphQueryEvaluationDiagnostic::for_variable(
                        VariableGraphQueryEvaluationDiagnosticCode::InvalidInputValue,
                        &node.id,
                    ));
                } else {
                    resolved_scalar_values.insert(node.id.clone(), value);
                }
            }
        }
        if !diagnostics.is_empty() {
            diagnostics.sort();
            diagnostics.dedup();
            return Err(diagnostics);
        }

        Ok(Self {
            graph: graph.clone(),
            execution_id,
            topological_order: compiled.topological_order,
            dependencies: compiled.dependencies,
            resolved_scalar_values,
            validated_results: BTreeMap::new(),
        })
    }

    /// Compile the next query request from this snapshot's validated inputs
    /// and already accepted scalar dependencies.
    pub fn compile_request(
        &self,
        query_variable_id: &str,
    ) -> Result<SqlQueryRequest, Vec<VariableGraphQueryEvaluationDiagnostic>> {
        let Some(node) = self
            .graph
            .variables
            .iter()
            .find(|node| node.id == query_variable_id)
        else {
            return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                VariableGraphQueryEvaluationDiagnosticCode::QueryVariableNotFound,
                query_variable_id,
            )]);
        };
        if !matches!(node.kind, VariableGraphNodeKind::Query { .. }) {
            return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                VariableGraphQueryEvaluationDiagnosticCode::QueryNodeRequired,
                query_variable_id,
            )]);
        }
        let continuation = match self.validated_results.get(query_variable_id) {
            Some(previous)
                if matches!(&previous.result, VariableGraphValidatedResult::Table { .. })
                    && previous.has_more =>
            {
                previous.next.clone()
            }
            Some(_) => {
                return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                    VariableGraphQueryEvaluationDiagnosticCode::QueryAlreadyEvaluated,
                    query_variable_id,
                )]);
            }
            None => {
                if self
                    .dependencies
                    .get(query_variable_id)
                    .into_iter()
                    .flatten()
                    .any(|dependency_id| {
                        matches!(
                            nodes_kind(&self.graph, dependency_id),
                            Some(VariableGraphNodeKind::Query { .. })
                        ) && !self.validated_results.contains_key(dependency_id)
                    })
                {
                    return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                        VariableGraphQueryEvaluationDiagnosticCode::QueryDependencyNotReady,
                        query_variable_id,
                    )]);
                }
                None
            }
        };

        let mut request = compile_variable_graph_query_request(
            &self.graph,
            query_variable_id,
            self.execution_id,
            &self.resolved_scalar_values,
        )
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(VariableGraphQueryEvaluationDiagnostic::from_request)
                .collect::<Vec<_>>()
        })?;
        request.continuation = continuation;
        request.validate().map_err(|_| {
            vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                VariableGraphQueryEvaluationDiagnosticCode::RequestCompilationFailed,
                query_variable_id,
            )]
        })?;
        Ok(request)
    }

    /// Accept one SQL page only when it was produced by this snapshot's exact
    /// compiled request and its schema, nullability, values, and cardinality
    /// satisfy the graph descriptor. A failed page leaves the state unchanged.
    pub fn accept_page(
        &mut self,
        query_variable_id: &str,
        request: &SqlQueryRequest,
        page: &SqlQueryPage,
    ) -> Result<(), Vec<VariableGraphQueryEvaluationDiagnostic>> {
        let expected_request = self.compile_request(query_variable_id)?;
        if request.execution_id != Some(self.execution_id) {
            return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                VariableGraphQueryEvaluationDiagnosticCode::RequestSnapshotMismatch,
                query_variable_id,
            )]);
        }
        if request != &expected_request {
            return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                VariableGraphQueryEvaluationDiagnosticCode::RequestDoesNotMatchGraph,
                query_variable_id,
            )]);
        }

        if page.has_more != page.next.is_some() {
            return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                VariableGraphQueryEvaluationDiagnosticCode::MalformedPageContinuation,
                query_variable_id,
            )]);
        }
        if request
            .continuation
            .as_ref()
            .is_some_and(|continuation| page.next.as_ref().is_some_and(|next| next == continuation))
        {
            return Err(vec![VariableGraphQueryEvaluationDiagnostic::for_variable(
                VariableGraphQueryEvaluationDiagnosticCode::RepeatedPageContinuation,
                query_variable_id,
            )]);
        }

        let result =
            validate_variable_graph_sql_page(&self.graph, query_variable_id, request, page)
                .map_err(|diagnostics| {
                    diagnostics
                        .into_iter()
                        .map(VariableGraphQueryEvaluationDiagnostic::from_result)
                        .collect::<Vec<_>>()
                })?;
        if let VariableGraphValidatedResult::Scalar { value } = &result {
            self.resolved_scalar_values
                .insert(query_variable_id.to_owned(), value.clone());
        }
        self.validated_results.insert(
            query_variable_id.to_owned(),
            VariableGraphQueryPreviewValue {
                result,
                has_more: page.has_more,
                next: page.next.clone(),
            },
        );
        Ok(())
    }

    /// Expose typed query values only after every query in the graph has
    /// produced a validated result for this snapshot.
    pub fn preview(
        &self,
    ) -> Result<VariableGraphQueryPreview, Vec<VariableGraphQueryEvaluationDiagnostic>> {
        let missing = self
            .topological_order
            .iter()
            .filter(|variable_id| {
                matches!(
                    nodes_kind(&self.graph, variable_id),
                    Some(VariableGraphNodeKind::Query { .. })
                ) && !self.validated_results.contains_key(variable_id.as_str())
            })
            .map(|variable_id| {
                VariableGraphQueryEvaluationDiagnostic::for_variable(
                    VariableGraphQueryEvaluationDiagnosticCode::QueryResultMissing,
                    variable_id,
                )
            })
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            return Err(missing);
        }
        Ok(VariableGraphQueryPreview {
            execution_id: self.execution_id,
            query_results: self.validated_results.clone(),
        })
    }
}

fn nodes_kind<'a>(
    graph: &'a VariableGraph,
    variable_id: &str,
) -> Option<&'a VariableGraphNodeKind> {
    graph
        .variables
        .iter()
        .find(|node| node.id == variable_id)
        .map(|node| &node.kind)
}

/// Validate a graph, then compile one query variable and its resolved values.
///
/// Argument names in `SqlQueryRequest` are the SQL parameter names from the
/// query definition; values are looked up by their explicitly bound graph
/// variable IDs. Unrelated resolved values are ignored. Missing values and
/// unsupported SQL parameter types fail closed. The request is always limited
/// to a bounded first page and carries no continuation token.
/// A Saved SQL output-column selection remains query-result metadata because
/// `SqlQueryRequest` carries the pinned revision and parameters only.
/// The caller owns the execution snapshot identity so this portable compiler
/// does not depend on a platform-specific random-number provider.
pub fn compile_variable_graph_query_request(
    graph: &VariableGraph,
    query_variable_id: &str,
    execution_id: Uuid,
    resolved_values: &BTreeMap<String, VariableGraphLiteral>,
) -> Result<SqlQueryRequest, Vec<VariableGraphQueryRequestDiagnostic>> {
    if let Err(domain_diagnostics) = compile_variable_graph(graph) {
        return Err(map_graph_diagnostics(domain_diagnostics, query_variable_id));
    }

    let Some(node) = graph
        .variables
        .iter()
        .find(|node| node.id == query_variable_id)
    else {
        return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
            VariableGraphQueryRequestDiagnosticCode::QueryVariableNotFound,
            query_variable_id,
        )]);
    };

    compile_validated_query_node(graph, node, execution_id, resolved_values)
}

fn compile_validated_query_node(
    graph: &VariableGraph,
    node: &VariableGraphNode,
    execution_id: Uuid,
    resolved_values: &BTreeMap<String, VariableGraphLiteral>,
) -> Result<SqlQueryRequest, Vec<VariableGraphQueryRequestDiagnostic>> {
    compile_validated_query_node_with_deferred_dependencies(
        graph,
        node,
        execution_id,
        resolved_values,
        &BTreeSet::new(),
    )
    .map(|(request, _)| request)
}

fn compile_validated_query_node_with_deferred_dependencies(
    graph: &VariableGraph,
    node: &VariableGraphNode,
    execution_id: Uuid,
    resolved_values: &BTreeMap<String, VariableGraphLiteral>,
    deferred_query_variable_ids: &BTreeSet<String>,
) -> Result<
    (
        SqlQueryRequest,
        Vec<VariableGraphQueryRequestDeferredArgument>,
    ),
    Vec<VariableGraphQueryRequestDiagnostic>,
> {
    let VariableGraphNodeKind::Query { query, returns } = &node.kind else {
        return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
            VariableGraphQueryRequestDiagnosticCode::QueryNodeRequired,
            &node.id,
        )]);
    };

    let (sql, saved_sql) = match &query.definition {
        VariableGraphQueryDefinition::InlineSql { sql } => {
            if sql.len() > MAX_SQL_QUERY_BYTES {
                return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::RequestOutOfBounds,
                    &node.id,
                )]);
            }
            if parse_sql_document(sql).status != SqlDocumentStatus::Parsed {
                return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::InvalidInlineSql,
                    &node.id,
                )]);
            }
            (sql.clone(), None)
        }
        VariableGraphQueryDefinition::SavedSql {
            entry_id,
            revision_id,
            column,
        } => {
            let (Some(entry_id), Some(revision_id)) = (entry_id.as_deref(), revision_id.as_deref())
            else {
                return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::InvalidSavedSqlIdentity,
                    &node.id,
                )]);
            };
            if Uuid::parse_str(entry_id).is_err() || Uuid::parse_str(revision_id).is_err() {
                return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::InvalidSavedSqlIdentity,
                    &node.id,
                )]);
            }
            if column.as_deref().is_some_and(|name| name.trim().is_empty())
                || (column.is_some() && !matches!(returns, VariableGraphResultType::Scalar { .. }))
            {
                return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::InvalidSavedSqlColumn,
                    &node.id,
                )]);
            }
            (
                String::new(),
                Some(SavedSqlRevisionRef {
                    id: entry_id.to_owned(),
                    revision_id: revision_id.to_owned(),
                }),
            )
        }
    };

    let mut diagnostics = Vec::new();
    let mut deferred_arguments = Vec::new();
    let mut seen_names = BTreeSet::new();
    let mut parameters = Map::new();
    let mut parameter_types = BTreeMap::new();
    let graph_nodes: BTreeMap<&str, &VariableGraphNode> = graph
        .variables
        .iter()
        .map(|graph_node| (graph_node.id.as_str(), graph_node))
        .collect();
    for argument in &query.arguments {
        if !valid_argument_name(&argument.name) {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::InvalidArgumentName,
                &node.id,
                argument,
            ));
            continue;
        }
        if !seen_names.insert(argument.name.as_str()) {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::DuplicateArgumentName,
                &node.id,
                argument,
            ));
            continue;
        }

        let source_type = graph_nodes
            .get(argument.variable.as_str())
            .and_then(|source| scalar_descriptor_of(&source.kind));
        let Some(source_type) = source_type else {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::ArgumentTypeMismatch,
                &node.id,
                argument,
            ));
            continue;
        };
        let Some(sql_type) = sql_type_for_graph_type(source_type.scalar_type) else {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::UnsupportedArgumentType,
                &node.id,
                argument,
            ));
            continue;
        };
        if deferred_query_variable_ids.contains(&argument.variable) {
            deferred_arguments.push(VariableGraphQueryRequestDeferredArgument {
                argument_name: argument.name.clone(),
                variable_id: argument.variable.clone(),
                value_type: source_type,
            });
            parameter_types.insert(argument.name.clone(), sql_type.to_owned());
            continue;
        }
        let Some(value) = resolved_values.get(&argument.variable) else {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::MissingArgumentValue,
                &node.id,
                argument,
            ));
            continue;
        };
        if !literal_kind_matches(source_type.scalar_type, value) {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::ArgumentTypeMismatch,
                &node.id,
                argument,
            ));
            continue;
        }
        if !literal_matches_descriptor(source_type, value) {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::InvalidArgumentValue,
                &node.id,
                argument,
            ));
            continue;
        }
        let Some(value) = bind_literal(source_type, value) else {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::InvalidArgumentValue,
                &node.id,
                argument,
            ));
            continue;
        };
        parameters.insert(argument.name.clone(), value);
        parameter_types.insert(argument.name.clone(), sql_type.to_owned());
    }
    if !diagnostics.is_empty() {
        diagnostics.sort();
        diagnostics.dedup();
        return Err(diagnostics);
    }

    let request = SqlQueryRequest {
        sql,
        parameters,
        parameter_types,
        include_exact_integer_values: false,
        limit: match returns {
            VariableGraphResultType::Scalar { .. } => MAX_SQL_SCALAR_PROBE_ROWS,
            VariableGraphResultType::Table { .. } => DEFAULT_VARIABLE_GRAPH_SQL_PAGE_LIMIT,
        },
        continuation: None,
        execution_id: Some(execution_id),
        saved_sql,
    };
    request.validate().map_err(|_| {
        vec![VariableGraphQueryRequestDiagnostic::for_node(
            VariableGraphQueryRequestDiagnosticCode::RequestOutOfBounds,
            &node.id,
        )]
    })?;
    Ok((request, deferred_arguments))
}

/// Compile a selected set of query variables into one atomic request batch.
///
/// Duplicate requested IDs are collapsed. Requests are emitted once in the
/// deterministic dependency order produced by [`compile_variable_graph`],
/// independent of graph authoring order or requested-ID order. If any ID is
/// unknown or not a query, or if any request cannot be compiled, no batch is
/// returned. Arguments sourced from other selected query variables are
/// returned as deferred dependencies with `request: None`; callers must wait
/// for those queries to produce fresh values and then compile the dependent
/// request with [`compile_variable_graph_query_request`]. This helper only
/// prepares requests; it does not authorize, execute, or validate query
/// results. `execution_id` is the immutable snapshot identity shared by the
/// requests in this batch; each request fingerprint still distinguishes its
/// own SQL and parameter subset.
pub fn compile_variable_graph_query_request_batch(
    graph: &VariableGraph,
    requested_query_variable_ids: &[String],
    execution_id: Uuid,
    resolved_values: &BTreeMap<String, VariableGraphLiteral>,
) -> Result<Vec<VariableGraphQueryRequestBatchItem>, Vec<VariableGraphQueryRequestDiagnostic>> {
    let compiled_graph = compile_variable_graph(graph).map_err(|diagnostics| {
        let fallback_query_id = requested_query_variable_ids
            .iter()
            .min()
            .map_or("", String::as_str);
        map_graph_diagnostics(diagnostics, fallback_query_id)
    })?;
    let requested_ids: BTreeSet<String> = requested_query_variable_ids.iter().cloned().collect();
    let nodes: BTreeMap<&str, &VariableGraphNode> = graph
        .variables
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();

    let mut diagnostics = Vec::new();
    for query_id in &requested_ids {
        match nodes.get(query_id.as_str()) {
            None => diagnostics.push(VariableGraphQueryRequestDiagnostic::for_node(
                VariableGraphQueryRequestDiagnosticCode::QueryVariableNotFound,
                query_id,
            )),
            Some(node) if !matches!(&node.kind, VariableGraphNodeKind::Query { .. }) => {
                diagnostics.push(VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::QueryNodeRequired,
                    query_id,
                ));
            }
            Some(_) => {}
        }
    }

    let mut batch = Vec::new();
    for query_id in &compiled_graph.topological_order {
        if !requested_ids.contains(query_id.as_str()) {
            continue;
        }
        let node = nodes[query_id.as_str()];
        match compile_validated_query_node_with_deferred_dependencies(
            graph,
            node,
            execution_id,
            resolved_values,
            &requested_ids,
        ) {
            Ok((request, deferred_arguments)) => batch.push(VariableGraphQueryRequestBatchItem {
                variable_id: query_id.clone(),
                request: deferred_arguments.is_empty().then_some(request),
                deferred_arguments,
            }),
            Err(mut request_diagnostics) => diagnostics.append(&mut request_diagnostics),
        }
    }

    if diagnostics.is_empty() {
        Ok(batch)
    } else {
        diagnostics.sort();
        diagnostics.dedup();
        Err(diagnostics)
    }
}

fn map_graph_diagnostics(
    diagnostics: Vec<VariableGraphDiagnostic>,
    query_id: &str,
) -> Vec<VariableGraphQueryRequestDiagnostic> {
    let mut mapped: Vec<_> = diagnostics
        .into_iter()
        .map(|diagnostic| VariableGraphQueryRequestDiagnostic::from_domain(diagnostic, query_id))
        .collect();
    mapped.sort();
    mapped.dedup();
    mapped
}

/// Validate a typed SQL page before exposing it as a query variable value.
///
/// The caller passes the exact request used to obtain the page so Saved SQL
/// results remain tied to the query variable's pinned entry and revision. The
/// page's ordered planner schema supplies column names and portable types;
/// values are converted without coercion and checked by the Domain result
/// contract. Older typed SQL schemas that omit nullability are conservatively
/// decoded as nullable.
///
/// A Saved SQL scalar may project a named output column only when that name is
/// present exactly once in the pinned revision's result schema. No first-row
/// or first-column selection is performed implicitly.
pub fn validate_variable_graph_sql_page(
    graph: &VariableGraph,
    query_variable_id: &str,
    request: &SqlQueryRequest,
    page: &SqlQueryPage,
) -> Result<VariableGraphValidatedResult, Vec<VariableGraphQueryResultDiagnostic>> {
    let Some(node) = graph
        .variables
        .iter()
        .find(|node| node.id == query_variable_id)
    else {
        return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
            VariableGraphQueryResultDiagnosticCode::QueryVariableNotFound,
            query_variable_id,
        )]);
    };
    let VariableGraphNodeKind::Query { returns, query, .. } = &node.kind else {
        return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
            VariableGraphQueryResultDiagnosticCode::QueryNodeRequired,
            &node.id,
        )]);
    };

    let selected_column = match &query.definition {
        VariableGraphQueryDefinition::InlineSql { sql }
            if request.saved_sql.is_none() && request.sql == *sql =>
        {
            None
        }
        VariableGraphQueryDefinition::SavedSql {
            entry_id: Some(entry_id),
            revision_id: Some(revision_id),
            column,
        } if request.saved_sql.as_ref().is_some_and(|saved_sql| {
            saved_sql.id == *entry_id && saved_sql.revision_id == *revision_id
        }) =>
        {
            column.as_deref()
        }
        _ => {
            return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
                VariableGraphQueryResultDiagnosticCode::RequestSourceMismatch,
                &node.id,
            )]);
        }
    };

    let request_fingerprint = request.execution_fingerprint().ok();
    if request.execution_id.is_none()
        || request.execution_id != page.execution_id
        || request_fingerprint.as_deref() != page.request_fingerprint.as_deref()
    {
        return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
            VariableGraphQueryResultDiagnosticCode::ExecutionMismatch,
            &node.id,
        )]);
    }

    let Some(result_schema) = page.result_schema.as_deref() else {
        return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
            VariableGraphQueryResultDiagnosticCode::ResultSchemaMissing,
            &node.id,
        )]);
    };
    if page.columns.len() != result_schema.len()
        || page
            .columns
            .iter()
            .zip(result_schema)
            .any(|(name, column)| name != &column.name)
    {
        return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
            VariableGraphQueryResultDiagnosticCode::ResultSchemaMismatch,
            &node.id,
        )]);
    }

    let selected_indices = if let Some(selected_column) = selected_column {
        let matches = result_schema
            .iter()
            .enumerate()
            .filter_map(|(index, column)| (column.name == selected_column).then_some(index))
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [] => {
                return Err(vec![VariableGraphQueryResultDiagnostic::for_column(
                    VariableGraphQueryResultDiagnosticCode::SavedSqlColumnMissing,
                    &node.id,
                    selected_column,
                )]);
            }
            [index] => vec![*index],
            _ => {
                return Err(vec![VariableGraphQueryResultDiagnostic::for_column(
                    VariableGraphQueryResultDiagnosticCode::SavedSqlColumnAmbiguous,
                    &node.id,
                    selected_column,
                )]);
            }
        }
    } else {
        (0..result_schema.len()).collect()
    };

    let selected_schema = selected_indices
        .iter()
        .map(|index| &result_schema[*index])
        .collect::<Vec<_>>();
    let expected_row_names = page.columns.iter().collect::<BTreeSet<_>>();
    let mut observed_rows = Vec::with_capacity(page.rows.len());
    for row in &page.rows {
        let Some(row) = row.as_object() else {
            return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
                VariableGraphQueryResultDiagnosticCode::InvalidResultRow,
                &node.id,
            )]);
        };
        if row.keys().any(|name| !expected_row_names.contains(name)) {
            return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
                VariableGraphQueryResultDiagnosticCode::InvalidResultRow,
                &node.id,
            )]);
        }
        let mut values = Vec::with_capacity(selected_schema.len());
        for column in &selected_schema {
            let Some(value) = row.get(&column.name) else {
                return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
                    VariableGraphQueryResultDiagnosticCode::InvalidResultRow,
                    &node.id,
                )]);
            };
            let Some(value) = sql_result_literal(column.column_type, value) else {
                return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
                    VariableGraphQueryResultDiagnosticCode::InvalidResultRow,
                    &node.id,
                )]);
            };
            values.push(value);
        }
        observed_rows.push(values);
    }

    let observed_columns = selected_schema
        .iter()
        .map(|column| VariableGraphColumn {
            name: column.name.clone(),
            value_type: VariableGraphScalarDescriptor {
                scalar_type: graph_scalar_type(column.column_type),
                nullable: column.nullable,
            },
        })
        .collect::<Vec<_>>();

    let result = validate_variable_graph_result(returns, &observed_columns, &observed_rows)
        .map_err(|diagnostics| {
            diagnostics
                .into_iter()
                .map(|diagnostic| {
                    VariableGraphQueryResultDiagnostic::for_contract(&node.id, diagnostic)
                })
                .collect::<Vec<_>>()
        })?;

    if matches!(returns, VariableGraphResultType::Scalar { .. })
        && (request.limit != MAX_SQL_SCALAR_PROBE_ROWS
            || request.continuation.is_some()
            || page.has_more
            || page.next.is_some())
    {
        return Err(vec![VariableGraphQueryResultDiagnostic::for_node(
            VariableGraphQueryResultDiagnosticCode::ScalarResultIncomplete,
            &node.id,
        )]);
    }

    Ok(result)
}

fn graph_scalar_type(column_type: SqlResultColumnType) -> VariableGraphScalarType {
    match column_type {
        SqlResultColumnType::String => VariableGraphScalarType::String,
        SqlResultColumnType::Boolean => VariableGraphScalarType::Boolean,
        SqlResultColumnType::Integer => VariableGraphScalarType::Integer,
        SqlResultColumnType::Float => VariableGraphScalarType::Float,
        SqlResultColumnType::Date => VariableGraphScalarType::Date,
        SqlResultColumnType::Timestamp => VariableGraphScalarType::Timestamp,
        SqlResultColumnType::Decimal => VariableGraphScalarType::Decimal,
        SqlResultColumnType::Json => VariableGraphScalarType::Json,
    }
}

fn sql_result_literal(
    column_type: SqlResultColumnType,
    value: &Value,
) -> Option<VariableGraphLiteral> {
    if value.is_null() {
        return Some(VariableGraphLiteral::Null);
    }
    match column_type {
        SqlResultColumnType::String => value
            .as_str()
            .map(|value| VariableGraphLiteral::String(value.to_owned())),
        SqlResultColumnType::Boolean => value.as_bool().map(VariableGraphLiteral::Boolean),
        SqlResultColumnType::Integer => value.as_i64().map(VariableGraphLiteral::Integer),
        SqlResultColumnType::Float => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(VariableGraphLiteral::Float),
        SqlResultColumnType::Date => value
            .as_str()
            .map(|value| VariableGraphLiteral::Date(value.to_owned())),
        SqlResultColumnType::Timestamp => value
            .as_str()
            .map(|value| VariableGraphLiteral::Timestamp(value.to_owned())),
        SqlResultColumnType::Decimal => value
            .as_str()
            .map(|value| VariableGraphLiteral::Decimal(value.to_owned())),
        SqlResultColumnType::Json => Some(VariableGraphLiteral::Json(value.clone())),
    }
}

fn scalar_descriptor_of(kind: &VariableGraphNodeKind) -> Option<VariableGraphScalarDescriptor> {
    match kind {
        VariableGraphNodeKind::Constant { value_type, .. }
        | VariableGraphNodeKind::Input { value_type, .. } => Some(*value_type),
        VariableGraphNodeKind::Query {
            returns: VariableGraphResultType::Scalar { value_type },
            ..
        } => Some(*value_type),
        VariableGraphNodeKind::Query {
            returns: VariableGraphResultType::Table { .. },
            ..
        } => None,
    }
}

fn valid_argument_name(name: &str) -> bool {
    let Some((first, rest)) = name.as_bytes().split_first() else {
        return false;
    };
    matches!(*first, b'A'..=b'Z' | b'a'..=b'z' | b'_')
        && rest
            .iter()
            .all(|byte| matches!(*byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

fn sql_type_for_graph_type(value_type: VariableGraphScalarType) -> Option<&'static str> {
    match value_type {
        VariableGraphScalarType::String => Some("string"),
        VariableGraphScalarType::Boolean => Some("boolean"),
        VariableGraphScalarType::Integer => Some("integer"),
        VariableGraphScalarType::Float => Some("float"),
        VariableGraphScalarType::Date => Some("date"),
        VariableGraphScalarType::Timestamp => Some("timestamp"),
        // The current SQL request binder does not accept these parameter types.
        VariableGraphScalarType::Decimal | VariableGraphScalarType::Json => None,
    }
}

fn literal_kind_matches(
    value_type: VariableGraphScalarType,
    literal: &VariableGraphLiteral,
) -> bool {
    if matches!(literal, VariableGraphLiteral::Null) {
        return true;
    }
    matches!(
        (value_type, literal),
        (
            VariableGraphScalarType::String,
            VariableGraphLiteral::String(_)
        ) | (
            VariableGraphScalarType::Boolean,
            VariableGraphLiteral::Boolean(_)
        ) | (
            VariableGraphScalarType::Integer,
            VariableGraphLiteral::Integer(_)
        ) | (
            VariableGraphScalarType::Float,
            VariableGraphLiteral::Float(_)
        ) | (VariableGraphScalarType::Date, VariableGraphLiteral::Date(_))
            | (
                VariableGraphScalarType::Timestamp,
                VariableGraphLiteral::Timestamp(_)
            )
            | (
                VariableGraphScalarType::Decimal,
                VariableGraphLiteral::Decimal(_)
            )
            | (VariableGraphScalarType::Json, VariableGraphLiteral::Json(_))
    )
}

fn literal_matches_descriptor(
    descriptor: VariableGraphScalarDescriptor,
    literal: &VariableGraphLiteral,
) -> bool {
    if matches!(literal, VariableGraphLiteral::Null) {
        return descriptor.nullable;
    }
    match (descriptor.scalar_type, literal) {
        (VariableGraphScalarType::String, VariableGraphLiteral::String(_))
        | (VariableGraphScalarType::Boolean, VariableGraphLiteral::Boolean(_))
        | (VariableGraphScalarType::Integer, VariableGraphLiteral::Integer(_)) => true,
        (VariableGraphScalarType::Float, VariableGraphLiteral::Float(value)) => value.is_finite(),
        (VariableGraphScalarType::Decimal, VariableGraphLiteral::Decimal(value)) => {
            // Decimal is intentionally unsupported by the current SQL binder,
            // but keep graph-value validation aligned with its domain contract.
            !value.trim().is_empty()
        }
        (VariableGraphScalarType::Date, VariableGraphLiteral::Date(value)) => {
            is_canonical_date(value)
        }
        (VariableGraphScalarType::Timestamp, VariableGraphLiteral::Timestamp(value)) => {
            is_valid_timestamp(value)
        }
        (VariableGraphScalarType::Json, VariableGraphLiteral::Json(value)) => !value.is_null(),
        _ => false,
    }
}

fn bind_literal(
    descriptor: VariableGraphScalarDescriptor,
    literal: &VariableGraphLiteral,
) -> Option<Value> {
    if matches!(literal, VariableGraphLiteral::Null) {
        return descriptor.nullable.then_some(Value::Null);
    }

    match (descriptor.scalar_type, literal) {
        (VariableGraphScalarType::String, VariableGraphLiteral::String(value)) => {
            Some(Value::String(value.clone()))
        }
        (VariableGraphScalarType::Boolean, VariableGraphLiteral::Boolean(value)) => {
            Some(Value::Bool(*value))
        }
        (VariableGraphScalarType::Integer, VariableGraphLiteral::Integer(value)) => {
            Some(Value::Number(Number::from(*value)))
        }
        (VariableGraphScalarType::Float, VariableGraphLiteral::Float(value))
            if value.is_finite() =>
        {
            Number::from_f64(*value).map(Value::Number)
        }
        (VariableGraphScalarType::Date, VariableGraphLiteral::Date(value))
            if is_canonical_date(value) =>
        {
            Some(Value::String(value.clone()))
        }
        (VariableGraphScalarType::Timestamp, VariableGraphLiteral::Timestamp(value))
            if DateTime::parse_from_rfc3339(value).is_ok() =>
        {
            Some(Value::String(value.clone()))
        }
        _ => None,
    }
}

fn is_canonical_date(value: &str) -> bool {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .is_some_and(|date| {
            (1..=9999).contains(&date.year()) && date.format("%Y-%m-%d").to_string() == value
        })
}

fn is_valid_timestamp(value: &str) -> bool {
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len())
        || !value.is_ascii()
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
        || !is_canonical_date(&value[..10])
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
    if local.len() < 19
        || ![11, 12, 14, 15, 17, 18]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_query::{SqlResultColumn, MAX_SQL_PARAMETER_BYTES};
    use serde_json::json;
    use std::ops::{Deref, DerefMut};
    use ugoite_domain::composition::{
        VariableGraphColumn, VariableGraphQuery, VariableGraphResultDiagnosticCode,
        VariableGraphResultType, VariableGraphScalarDescriptor,
    };

    #[derive(Clone)]
    struct TestArgument {
        name: String,
        variable: String,
        value_type: VariableGraphScalarDescriptor,
    }

    #[derive(Clone)]
    struct TestQueryNode {
        node: VariableGraphNode,
        inputs: Vec<VariableGraphNode>,
    }

    impl Deref for TestQueryNode {
        type Target = VariableGraphNode;

        fn deref(&self) -> &Self::Target {
            &self.node
        }
    }

    impl DerefMut for TestQueryNode {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.node
        }
    }

    impl From<VariableGraphNode> for TestQueryNode {
        fn from(node: VariableGraphNode) -> Self {
            Self {
                node,
                inputs: Vec::new(),
            }
        }
    }

    fn scalar(
        value_type: VariableGraphScalarType,
        nullable: bool,
    ) -> VariableGraphScalarDescriptor {
        VariableGraphScalarDescriptor {
            scalar_type: value_type,
            nullable,
        }
    }

    fn test_execution_id() -> Uuid {
        Uuid::from_u128(1)
    }

    fn argument(
        name: &str,
        variable: &str,
        value_type: VariableGraphScalarType,
        nullable: bool,
    ) -> TestArgument {
        TestArgument {
            name: name.to_owned(),
            variable: variable.to_owned(),
            value_type: scalar(value_type, nullable),
        }
    }

    fn inline_node(arguments: Vec<TestArgument>) -> TestQueryNode {
        let sql = if arguments.is_empty() {
            "SELECT 1".to_owned()
        } else {
            format!(
                "SELECT {}",
                arguments
                    .iter()
                    .map(|argument| format!("${}", argument.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        let returns = if arguments.len() > 1 {
            VariableGraphResultType::Table {
                columns: arguments
                    .iter()
                    .map(|argument| VariableGraphColumn {
                        name: argument.name.clone(),
                        value_type: argument.value_type,
                    })
                    .collect(),
            }
        } else {
            VariableGraphResultType::Scalar {
                value_type: arguments
                    .first()
                    .map(|argument| argument.value_type)
                    .unwrap_or_else(|| scalar(VariableGraphScalarType::Integer, false)),
            }
        };
        query_node(
            "rows",
            VariableGraphQueryDefinition::InlineSql { sql },
            returns,
            arguments,
        )
    }

    fn query_node(
        id: &str,
        definition: VariableGraphQueryDefinition,
        returns: VariableGraphResultType,
        arguments: Vec<TestArgument>,
    ) -> TestQueryNode {
        TestQueryNode {
            node: VariableGraphNode {
                id: id.to_owned(),
                kind: VariableGraphNodeKind::Query {
                    returns,
                    query: VariableGraphQuery {
                        definition,
                        arguments: arguments
                            .iter()
                            .map(|argument| VariableGraphArgumentBinding {
                                name: argument.name.clone(),
                                variable: argument.variable.clone(),
                            })
                            .collect(),
                        form_bindings: Vec::new(),
                        schema_fingerprint: String::new(),
                    },
                },
            },
            inputs: arguments
                .into_iter()
                .map(|argument| VariableGraphNode {
                    id: argument.variable,
                    kind: VariableGraphNodeKind::Input {
                        value_type: argument.value_type,
                        required: true,
                        default: None,
                    },
                })
                .collect(),
        }
    }

    fn scalar_query_node(id: &str, sql: &str, arguments: Vec<TestArgument>) -> TestQueryNode {
        query_node(
            id,
            VariableGraphQueryDefinition::InlineSql {
                sql: sql.to_owned(),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
            arguments,
        )
    }

    fn graph_for(node: impl Into<TestQueryNode>) -> VariableGraph {
        graph_from_nodes([node])
    }

    fn graph_from_nodes(
        nodes: impl IntoIterator<Item = impl Into<TestQueryNode>>,
    ) -> VariableGraph {
        let nodes: Vec<_> = nodes.into_iter().map(Into::into).collect();
        let mut variables = BTreeMap::new();
        for node in &nodes {
            for input in &node.inputs {
                variables
                    .entry(input.id.clone())
                    .or_insert_with(|| input.clone());
            }
        }
        for node in nodes {
            variables.insert(node.node.id.clone(), node.node);
        }
        VariableGraph {
            variables: variables.into_values().collect(),
        }
    }

    fn compile_node(
        node: &impl CloneIntoTestQueryNode,
        resolved_values: &BTreeMap<String, VariableGraphLiteral>,
    ) -> Result<SqlQueryRequest, Vec<VariableGraphQueryRequestDiagnostic>> {
        let node = node.clone_into_test_query_node();
        let graph = graph_for(node.clone());
        compile_variable_graph_query_request(
            &graph,
            &node.node.id,
            Uuid::from_u128(1),
            resolved_values,
        )
    }

    trait CloneIntoTestQueryNode {
        fn clone_into_test_query_node(&self) -> TestQueryNode;
    }

    impl CloneIntoTestQueryNode for TestQueryNode {
        fn clone_into_test_query_node(&self) -> TestQueryNode {
            self.clone()
        }
    }

    impl CloneIntoTestQueryNode for VariableGraphNode {
        fn clone_into_test_query_node(&self) -> TestQueryNode {
            self.clone().into()
        }
    }

    fn argument_diagnostic(
        code: VariableGraphQueryRequestDiagnosticCode,
        name: &str,
        variable: &str,
    ) -> VariableGraphQueryRequestDiagnostic {
        VariableGraphQueryRequestDiagnostic {
            code,
            variable_id: "rows".to_owned(),
            argument_name: Some(name.to_owned()),
            reference_id: Some(variable.to_owned()),
            domain_code: None,
            cycle: Vec::new(),
        }
    }

    fn graph_diagnostic(
        variable_id: &str,
        domain_code: &str,
    ) -> VariableGraphQueryRequestDiagnostic {
        VariableGraphQueryRequestDiagnostic {
            code: VariableGraphQueryRequestDiagnosticCode::InvalidGraph,
            variable_id: variable_id.to_owned(),
            argument_name: None,
            reference_id: None,
            domain_code: Some(domain_code.to_owned()),
            cycle: Vec::new(),
        }
    }

    fn result_node(
        query: VariableGraphQueryDefinition,
        returns: VariableGraphResultType,
    ) -> TestQueryNode {
        query_node("result", query, returns, Vec::new())
    }

    fn result_page(
        request: &SqlQueryRequest,
        columns: &[(&str, SqlResultColumnType)],
        rows: Vec<Value>,
    ) -> SqlQueryPage {
        SqlQueryPage {
            columns: columns.iter().map(|(name, _)| (*name).to_owned()).collect(),
            rows,
            has_more: false,
            next: None,
            execution_id: request.execution_id,
            request_fingerprint: request.execution_fingerprint().ok(),
            result_schema: Some(
                columns
                    .iter()
                    .map(|(name, column_type)| SqlResultColumn {
                        name: (*name).to_owned(),
                        column_type: *column_type,
                        nullable: false,
                    })
                    .collect(),
            ),
            exact_integer_rows: None,
        }
    }

    #[test]
    fn planned_nullable_column_cannot_satisfy_a_required_scalar_descriptor() {
        let node = result_node(
            VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT maybe_total".to_owned(),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
        );
        let graph = graph_for(node);
        let request = compile_variable_graph_query_request(
            &graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("query request compiles");
        let mut page = result_page(
            &request,
            &[("maybe_total", SqlResultColumnType::Integer)],
            vec![json!({"maybe_total": 8})],
        );
        page.result_schema.as_mut().expect("planner result schema")[0].nullable = true;

        let diagnostics = validate_variable_graph_sql_page(&graph, "result", &request, &page)
            .expect_err("planner nullability is part of the declared result contract");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.result_diagnostic.as_ref().is_some_and(|result| {
                result.code == VariableGraphResultDiagnosticCode::ColumnTypeMismatch
                    && result
                        .observed_value_type
                        .is_some_and(|value_type| value_type.nullable)
            })
        }));
    }

    #[test]
    fn validates_inline_planner_schema_before_exposing_scalar_values() {
        let node = result_node(
            VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT COUNT(*) AS total".to_owned(),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
        );
        let graph = graph_for(node);
        let request = compile_variable_graph_query_request(
            &graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("request compiles");
        let page = result_page(
            &request,
            &[("total", SqlResultColumnType::Integer)],
            vec![json!({"total": 3})],
        );

        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &request, &page)
                .expect("planner result matches declared scalar"),
            VariableGraphValidatedResult::Scalar {
                value: VariableGraphLiteral::Integer(3),
            }
        );

        let wrong_type_node = result_node(
            VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT COUNT(*) AS total".to_owned(),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::String, false),
            },
        );
        let wrong_type_graph = graph_for(wrong_type_node);
        let mut wrong_type_request = compile_variable_graph_query_request(
            &wrong_type_graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("query request compiles despite runtime result drift");
        wrong_type_request.execution_id = request.execution_id;
        let mismatch = validate_variable_graph_sql_page(
            &wrong_type_graph,
            "result",
            &wrong_type_request,
            &page,
        )
        .expect_err("planned integer must not be exposed as a string");
        assert_eq!(
            mismatch[0].code,
            VariableGraphQueryResultDiagnosticCode::ResultContractMismatch
        );
        assert_eq!(
            mismatch[0]
                .result_diagnostic
                .as_ref()
                .expect("Domain result diagnostic")
                .code,
            VariableGraphResultDiagnosticCode::ColumnTypeMismatch
        );
    }

    #[test]
    fn validates_saved_sql_output_column_against_the_pinned_result_schema() {
        let node = result_node(
            VariableGraphQueryDefinition::SavedSql {
                entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
                revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                column: Some("total".to_owned()),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
        );
        let graph = graph_for(node);
        let request = compile_variable_graph_query_request(
            &graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("request pins Saved SQL revision");
        let page = result_page(
            &request,
            &[
                ("label", SqlResultColumnType::String),
                ("total", SqlResultColumnType::Integer),
            ],
            vec![json!({"label": "March", "total": 17})],
        );
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &request, &page)
                .expect("explicitly selected output column is scalar"),
            VariableGraphValidatedResult::Scalar {
                value: VariableGraphLiteral::Integer(17),
            }
        );

        let missing = result_page(
            &request,
            &[("label", SqlResultColumnType::String)],
            vec![json!({"label": "March"})],
        );
        let missing_diagnostic =
            validate_variable_graph_sql_page(&graph, "result", &request, &missing)
                .expect_err("selected column must exist in the pinned revision schema");
        assert_eq!(
            missing_diagnostic[0].code,
            VariableGraphQueryResultDiagnosticCode::SavedSqlColumnMissing
        );
        assert_eq!(missing_diagnostic[0].column_name.as_deref(), Some("total"));

        let ambiguous = result_page(
            &request,
            &[
                ("total", SqlResultColumnType::Integer),
                ("total", SqlResultColumnType::Integer),
            ],
            vec![json!({"total": 17})],
        );
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &request, &ambiguous)
                .expect_err("duplicate selected names are ambiguous")[0]
                .code,
            VariableGraphQueryResultDiagnosticCode::SavedSqlColumnAmbiguous
        );

        let mut wrong_revision_request = request.clone();
        wrong_revision_request
            .saved_sql
            .as_mut()
            .expect("Saved SQL request")
            .revision_id = "00000000-0000-0000-0000-000000000003".to_owned();
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &wrong_revision_request, &page,)
                .expect_err("the response request must retain the exact pinned revision")[0]
                .code,
            VariableGraphQueryResultDiagnosticCode::RequestSourceMismatch
        );
    }

    #[test]
    fn scalar_results_never_choose_a_first_row_or_column() {
        let node = result_node(
            VariableGraphQueryDefinition::SavedSql {
                entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
                revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                column: None,
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
        );
        let graph = graph_for(node);
        let request = compile_variable_graph_query_request(
            &graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("query request compiles");
        let multiple_columns = result_page(
            &request,
            &[
                ("total", SqlResultColumnType::Integer),
                ("other", SqlResultColumnType::Integer),
            ],
            vec![json!({"total": 4, "other": 5})],
        );
        let diagnostics =
            validate_variable_graph_sql_page(&graph, "result", &request, &multiple_columns)
                .expect_err("no selected Saved SQL column means no first-column fallback");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.result_diagnostic.as_ref().is_some_and(|result| {
                result.code == VariableGraphResultDiagnosticCode::ScalarColumnCountMismatch
            })
        }));

        let selected_node = result_node(
            VariableGraphQueryDefinition::SavedSql {
                entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
                revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                column: Some("total".to_owned()),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
        );
        let selected_graph = graph_for(selected_node);
        let selected_request = compile_variable_graph_query_request(
            &selected_graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("query request compiles");
        let multiple_rows = result_page(
            &selected_request,
            &[("total", SqlResultColumnType::Integer)],
            vec![json!({"total": 4}), json!({"total": 5})],
        );
        let diagnostics = validate_variable_graph_sql_page(
            &selected_graph,
            "result",
            &selected_request,
            &multiple_rows,
        )
        .expect_err("an explicit column does not permit first-row fallback");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.result_diagnostic.as_ref().is_some_and(|result| {
                result.code == VariableGraphResultDiagnosticCode::ScalarMultipleRows
            })
        }));
    }

    #[test]
    fn scalar_probe_rejects_continuations_and_stale_execution_pages() {
        let node = result_node(
            VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT COUNT(*) AS total".to_owned(),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
        );
        let graph = graph_for(node);
        let request = compile_variable_graph_query_request(
            &graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("query request compiles");
        let page = result_page(
            &request,
            &[("total", SqlResultColumnType::Integer)],
            vec![json!({"total": 4})],
        );

        let mut stale_request = request.clone();
        stale_request.execution_id = Some(Uuid::from_u128(2));
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &stale_request, &page)
                .expect_err("a page from an older execution cannot bind to current inputs")[0]
                .code,
            VariableGraphQueryResultDiagnosticCode::ExecutionMismatch
        );

        let mut stale_continuation_request = request.clone();
        stale_continuation_request.continuation = Some("opaque-next-page".to_owned());
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &stale_continuation_request, &page)
                .expect_err("a page from a different continuation request cannot be reused")[0]
                .code,
            VariableGraphQueryResultDiagnosticCode::ExecutionMismatch
        );

        let mut continued_request = request;
        continued_request.continuation = Some("opaque-next-page".to_owned());
        let continued_page = result_page(
            &continued_request,
            &[("total", SqlResultColumnType::Integer)],
            vec![json!({"total": 4})],
        );
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &continued_request, &continued_page)
                .expect_err("a later page cannot be accepted as a complete scalar probe")[0]
                .code,
            VariableGraphQueryResultDiagnosticCode::ScalarResultIncomplete
        );
    }

    #[test]
    fn validates_table_result_columns_and_types_from_planner_schema() {
        let node = result_node(
            VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT day, amount FROM \"G\"".to_owned(),
            },
            VariableGraphResultType::Table {
                columns: vec![
                    VariableGraphColumn {
                        name: "day".to_owned(),
                        value_type: scalar(VariableGraphScalarType::Date, false),
                    },
                    VariableGraphColumn {
                        name: "amount".to_owned(),
                        value_type: scalar(VariableGraphScalarType::Decimal, true),
                    },
                ],
            },
        );
        let graph = graph_for(node);
        let request = compile_variable_graph_query_request(
            &graph,
            "result",
            Uuid::from_u128(1),
            &BTreeMap::new(),
        )
        .expect("table request compiles");
        let page = result_page(
            &request,
            &[
                ("day", SqlResultColumnType::Date),
                ("amount", SqlResultColumnType::Decimal),
            ],
            vec![json!({"day": "2026-10-18", "amount": "12.500"})],
        );
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &request, &page)
                .expect("matching typed table result"),
            VariableGraphValidatedResult::Table {
                columns: vec![
                    VariableGraphColumn {
                        name: "day".to_owned(),
                        value_type: scalar(VariableGraphScalarType::Date, false),
                    },
                    VariableGraphColumn {
                        name: "amount".to_owned(),
                        value_type: scalar(VariableGraphScalarType::Decimal, true),
                    },
                ],
                rows: vec![vec![
                    VariableGraphLiteral::Date("2026-10-18".to_owned()),
                    VariableGraphLiteral::Decimal("12.500".to_owned()),
                ]],
            }
        );

        let mismatched_schema = result_page(
            &request,
            &[
                ("day", SqlResultColumnType::Date),
                ("amount", SqlResultColumnType::Integer),
            ],
            vec![json!({"day": "2026-10-18", "amount": 12})],
        );
        let diagnostics =
            validate_variable_graph_sql_page(&graph, "result", &request, &mismatched_schema)
                .expect_err("table output type must match its declared column");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.result_diagnostic.as_ref().is_some_and(|result| {
                result.code == VariableGraphResultDiagnosticCode::ColumnTypeMismatch
                    && result.column_index == Some(1)
            })
        }));

        let missing_cell = result_page(
            &request,
            &[
                ("day", SqlResultColumnType::Date),
                ("amount", SqlResultColumnType::Decimal),
            ],
            vec![json!({"day": "2026-10-18"})],
        );
        assert_eq!(
            validate_variable_graph_sql_page(&graph, "result", &request, &missing_cell)
                .expect_err("missing cell is not SQL NULL")
                .first()
                .expect("one local diagnostic")
                .code,
            VariableGraphQueryResultDiagnosticCode::InvalidResultRow
        );
    }

    #[test]
    fn compiles_inline_sql_with_explicit_names_and_typed_values() {
        let node = inline_node(vec![
            argument(
                "threshold",
                "floor_value",
                VariableGraphScalarType::Integer,
                false,
            ),
            argument("day", "selected_day", VariableGraphScalarType::Date, false),
            argument(
                "active",
                "is_active",
                VariableGraphScalarType::Boolean,
                false,
            ),
            argument(
                "rate",
                "exchange_rate",
                VariableGraphScalarType::Float,
                false,
            ),
            argument(
                "created_at",
                "created_at_value",
                VariableGraphScalarType::Timestamp,
                false,
            ),
        ]);
        let values = BTreeMap::from([
            ("floor_value".to_owned(), VariableGraphLiteral::Integer(42)),
            (
                "selected_day".to_owned(),
                VariableGraphLiteral::Date("2026-10-18".to_owned()),
            ),
            ("is_active".to_owned(), VariableGraphLiteral::Boolean(true)),
            (
                "exchange_rate".to_owned(),
                VariableGraphLiteral::Float(1.25),
            ),
            (
                "created_at_value".to_owned(),
                VariableGraphLiteral::Timestamp("2026-10-18T01:02:03Z".to_owned()),
            ),
            (
                "unrelated".to_owned(),
                VariableGraphLiteral::String("x".to_owned()),
            ),
        ]);

        let request = compile_node(&node, &values).expect("request");
        assert_eq!(
            request.sql,
            "SELECT $threshold, $day, $active, $rate, $created_at"
        );
        assert_eq!(request.parameters["threshold"], Value::from(42));
        assert_eq!(request.parameters["day"], Value::from("2026-10-18"));
        assert_eq!(request.parameters["active"], Value::Bool(true));
        assert_eq!(request.parameters["rate"], Value::from(1.25));
        assert_eq!(
            request.parameters["created_at"],
            Value::from("2026-10-18T01:02:03Z")
        );
        assert_eq!(request.parameter_types["threshold"], "integer");
        assert_eq!(request.parameter_types["day"], "date");
        assert_eq!(request.parameter_types["active"], "boolean");
        assert_eq!(request.parameter_types["rate"], "float");
        assert_eq!(request.parameter_types["created_at"], "timestamp");
        assert_eq!(request.limit, DEFAULT_VARIABLE_GRAPH_SQL_PAGE_LIMIT);
        assert_eq!(request.continuation, None);
        assert_eq!(request.execution_id, Some(Uuid::from_u128(1)));
        assert_eq!(request.saved_sql, None);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn compiles_saved_sql_to_exact_revision_without_copying_sql_text() {
        let node = query_node(
            "total",
            VariableGraphQueryDefinition::SavedSql {
                entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
                revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                column: Some("total".to_owned()),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
            vec![argument(
                "day",
                "selected_day",
                VariableGraphScalarType::Date,
                false,
            )],
        );
        let values = BTreeMap::from([(
            "selected_day".to_owned(),
            VariableGraphLiteral::Date("2026-10-18".to_owned()),
        )]);

        let request = compile_node(&node, &values).expect("request");
        assert_eq!(request.sql, "");
        assert_eq!(request.parameters["day"], Value::from("2026-10-18"));
        assert_eq!(request.parameter_types["day"], "date");
        assert_eq!(
            request.saved_sql,
            Some(SavedSqlRevisionRef {
                id: "00000000-0000-0000-0000-000000000001".to_owned(),
                revision_id: "00000000-0000-0000-0000-000000000002".to_owned(),
            })
        );
        assert_eq!(request.limit, MAX_SQL_SCALAR_PROBE_ROWS);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn missing_values_and_type_mismatches_are_stable() {
        let node = inline_node(vec![argument(
            "threshold",
            "floor_value",
            VariableGraphScalarType::Integer,
            false,
        )]);
        assert_eq!(
            compile_node(&node, &BTreeMap::new()).expect_err("missing resolved value"),
            [argument_diagnostic(
                VariableGraphQueryRequestDiagnosticCode::MissingArgumentValue,
                "threshold",
                "floor_value",
            )]
        );
        assert_eq!(
            VariableGraphQueryRequestDiagnosticCode::MissingArgumentValue.as_str(),
            "missing_argument_value"
        );

        let values = BTreeMap::from([(
            "floor_value".to_owned(),
            VariableGraphLiteral::String("42".to_owned()),
        )]);
        assert_eq!(
            compile_node(&node, &values).expect_err("wrong scalar type"),
            [argument_diagnostic(
                VariableGraphQueryRequestDiagnosticCode::ArgumentTypeMismatch,
                "threshold",
                "floor_value",
            )]
        );
    }

    #[test]
    fn nullable_and_unsupported_parameter_types_fail_or_bind_explicitly() {
        let nullable = inline_node(vec![argument(
            "threshold",
            "floor_value",
            VariableGraphScalarType::Integer,
            true,
        )]);
        let values = BTreeMap::from([("floor_value".to_owned(), VariableGraphLiteral::Null)]);
        let request = compile_node(&nullable, &values).expect("nullable typed null");
        assert_eq!(request.parameters["threshold"], Value::Null);
        assert_eq!(request.parameter_types["threshold"], "integer");

        let required = inline_node(vec![argument(
            "threshold",
            "floor_value",
            VariableGraphScalarType::Integer,
            false,
        )]);
        assert_eq!(
            compile_node(&required, &values)
                .expect_err("untyped null cannot satisfy non-null argument"),
            [argument_diagnostic(
                VariableGraphQueryRequestDiagnosticCode::InvalidArgumentValue,
                "threshold",
                "floor_value",
            )]
        );

        for scalar_type in [
            VariableGraphScalarType::Decimal,
            VariableGraphScalarType::Json,
        ] {
            let unsupported = inline_node(vec![argument(
                "threshold",
                "floor_value",
                scalar_type,
                false,
            )]);
            assert_eq!(
                compile_node(&unsupported, &BTreeMap::new())
                    .expect_err("the current SQL parameter binder has no type support"),
                [argument_diagnostic(
                    VariableGraphQueryRequestDiagnosticCode::UnsupportedArgumentType,
                    "threshold",
                    "floor_value",
                )]
            );
        }
    }

    #[test]
    fn rejects_invalid_values_without_coercion() {
        for (scalar_type, value) in [
            (
                VariableGraphScalarType::Float,
                VariableGraphLiteral::Float(f64::INFINITY),
            ),
            (
                VariableGraphScalarType::Date,
                VariableGraphLiteral::Date("2026-02-30".to_owned()),
            ),
            (
                VariableGraphScalarType::Timestamp,
                VariableGraphLiteral::Timestamp("2026-10-18 01:02:03".to_owned()),
            ),
        ] {
            let node = inline_node(vec![argument("value", "resolved", scalar_type, false)]);
            let values = BTreeMap::from([("resolved".to_owned(), value)]);
            assert_eq!(
                compile_node(&node, &values).expect_err("invalid typed value"),
                [argument_diagnostic(
                    VariableGraphQueryRequestDiagnosticCode::InvalidArgumentValue,
                    "value",
                    "resolved",
                )]
            );
        }
        assert_eq!(
            VariableGraphQueryRequestDiagnosticCode::InvalidArgumentValue.as_str(),
            "invalid_argument_value"
        );
    }

    #[test]
    fn rejects_invalid_query_definition_identity_and_argument_duplicates() {
        let invalid_sql = query_node(
            "bad",
            VariableGraphQueryDefinition::InlineSql {
                sql: "DELETE FROM entries".to_owned(),
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
            vec![],
        );
        assert_eq!(
            compile_node(&invalid_sql, &BTreeMap::new()).expect_err("non-query inline SQL"),
            [graph_diagnostic("bad", "invalid_inline_sql")]
        );

        let missing_saved_identity = query_node(
            "saved",
            VariableGraphQueryDefinition::SavedSql {
                entry_id: None,
                revision_id: None,
                column: None,
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
            vec![],
        );
        assert_eq!(
            compile_node(&missing_saved_identity, &BTreeMap::new())
                .expect_err("exact Saved SQL identity required"),
            [graph_diagnostic("saved", "invalid_saved_sql_identity")]
        );

        let invalid_saved_identity = query_node(
            "invalid_saved",
            VariableGraphQueryDefinition::SavedSql {
                entry_id: Some("not-a-uuid".to_owned()),
                revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                column: None,
            },
            VariableGraphResultType::Scalar {
                value_type: scalar(VariableGraphScalarType::Integer, false),
            },
            vec![],
        );
        assert_eq!(
            compile_node(&invalid_saved_identity, &BTreeMap::new())
                .expect_err("invalid entry identity"),
            [graph_diagnostic(
                "invalid_saved",
                "invalid_saved_sql_identity"
            )]
        );

        let mut duplicate = inline_node(vec![
            argument("threshold", "one", VariableGraphScalarType::Integer, false),
            argument("threshold", "two", VariableGraphScalarType::Integer, false),
        ]);
        let VariableGraphNodeKind::Query { returns, .. } = &mut duplicate.kind else {
            unreachable!("helper constructs a query node")
        };
        *returns = VariableGraphResultType::Scalar {
            value_type: scalar(VariableGraphScalarType::Integer, false),
        };
        let duplicate_values = BTreeMap::from([
            ("one".to_owned(), VariableGraphLiteral::Integer(1)),
            ("two".to_owned(), VariableGraphLiteral::Integer(2)),
        ]);
        assert_eq!(
            compile_node(&duplicate, &duplicate_values)
                .expect_err("duplicate names must not overwrite parameter values"),
            [VariableGraphQueryRequestDiagnostic {
                code: VariableGraphQueryRequestDiagnosticCode::InvalidGraph,
                variable_id: "rows".to_owned(),
                argument_name: Some("threshold".to_owned()),
                reference_id: None,
                domain_code: Some("duplicate_argument_name".to_owned()),
                cycle: Vec::new(),
            }]
        );
    }

    #[test]
    fn enforces_query_and_parameter_byte_bounds() {
        let mut oversized_sql = inline_node(vec![]);
        let VariableGraphNodeKind::Query { query, .. } = &mut oversized_sql.kind else {
            unreachable!("helper constructs a query node")
        };
        query.definition = VariableGraphQueryDefinition::InlineSql {
            sql: format!("SELECT 1 /* {} */", "x".repeat(MAX_SQL_QUERY_BYTES + 1)),
        };
        assert_eq!(
            compile_node(&oversized_sql, &BTreeMap::new()).expect_err("query byte bound"),
            [graph_diagnostic("rows", "invalid_inline_sql")]
        );

        let node = inline_node(vec![argument(
            "threshold",
            "large",
            VariableGraphScalarType::String,
            false,
        )]);
        let values = BTreeMap::from([(
            "large".to_owned(),
            VariableGraphLiteral::String("x".repeat(MAX_SQL_PARAMETER_BYTES + 1)),
        )]);
        assert_eq!(
            compile_node(&node, &values).expect_err("parameter JSON byte bound"),
            [VariableGraphQueryRequestDiagnostic::for_node(
                VariableGraphQueryRequestDiagnosticCode::RequestOutOfBounds,
                "rows"
            )]
        );
    }

    #[test]
    fn rejects_non_query_graph_nodes() {
        let node = VariableGraphNode {
            id: "value".to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: scalar(VariableGraphScalarType::Integer, false),
                required: true,
                default: None,
            },
        };
        assert_eq!(
            compile_node(&node, &BTreeMap::new()).expect_err("input is not a query"),
            [VariableGraphQueryRequestDiagnostic::for_node(
                VariableGraphQueryRequestDiagnosticCode::QueryNodeRequired,
                "value"
            )]
        );
    }

    #[test]
    fn validates_the_graph_and_query_identity_at_the_public_boundary() {
        let valid_node = inline_node(vec![]);
        let valid_graph = graph_for(valid_node);
        assert_eq!(
            compile_variable_graph_query_request(
                &valid_graph,
                "absent",
                Uuid::from_u128(1),
                &BTreeMap::new(),
            )
            .expect_err("unknown query reference"),
            [VariableGraphQueryRequestDiagnostic::for_node(
                VariableGraphQueryRequestDiagnosticCode::QueryVariableNotFound,
                "absent",
            )]
        );

        let mut invalid_graph = graph_for(inline_node(vec![]));
        invalid_graph.variables.push(
            query_node(
                "bad",
                VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT $missing".to_owned(),
                },
                VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                vec![],
            )
            .node,
        );
        assert_eq!(
            compile_variable_graph_query_request(
                &invalid_graph,
                "rows",
                Uuid::from_u128(1),
                &BTreeMap::new(),
            )
            .expect_err("all graph query definitions must validate"),
            [VariableGraphQueryRequestDiagnostic {
                code: VariableGraphQueryRequestDiagnosticCode::InvalidGraph,
                variable_id: "bad".to_owned(),
                argument_name: Some("missing".to_owned()),
                reference_id: None,
                domain_code: Some("missing_argument_binding".to_owned()),
                cycle: Vec::new(),
            }]
        );
    }

    #[test]
    fn batch_order_is_dependency_deterministic_and_independent_of_input_order() {
        let upstream = scalar_query_node("upstream", "SELECT 1", vec![]);
        let downstream = scalar_query_node(
            "downstream",
            "SELECT $seed",
            vec![argument(
                "seed",
                "upstream",
                VariableGraphScalarType::Integer,
                false,
            )],
        );
        let first_graph = graph_from_nodes(vec![downstream.clone(), upstream.clone()]);
        let reordered_graph = graph_from_nodes(vec![upstream, downstream]);
        let values = BTreeMap::from([("upstream".to_owned(), VariableGraphLiteral::Integer(7))]);
        let requested = vec!["downstream".to_owned(), "upstream".to_owned()];

        let first = compile_variable_graph_query_request_batch(
            &first_graph,
            &requested,
            test_execution_id(),
            &values,
        )
        .expect("valid request batch");
        let reordered = compile_variable_graph_query_request_batch(
            &reordered_graph,
            &requested,
            test_execution_id(),
            &values,
        )
        .expect("reordered graph keeps deterministic batch order");
        let reversed_request = compile_variable_graph_query_request_batch(
            &first_graph,
            &["upstream".to_owned(), "downstream".to_owned()],
            test_execution_id(),
            &values,
        )
        .expect("reordered request IDs keep deterministic batch order");

        let batch_ids: Vec<_> = first.iter().map(|item| item.variable_id.as_str()).collect();
        assert_eq!(batch_ids, ["upstream", "downstream"]);
        assert!(first[0].deferred_arguments.is_empty());
        assert_eq!(
            first[0]
                .request
                .as_ref()
                .expect("upstream request is ready")
                .execution_id,
            Some(test_execution_id())
        );
        assert_eq!(
            first[1].deferred_arguments,
            [VariableGraphQueryRequestDeferredArgument {
                argument_name: "seed".to_owned(),
                variable_id: "upstream".to_owned(),
                value_type: scalar(VariableGraphScalarType::Integer, false),
            }]
        );
        assert!(first[1].request.is_none());
        assert_eq!(first, reordered);
        assert_eq!(first, reversed_request);

        let no_previous_upstream_value = compile_variable_graph_query_request_batch(
            &first_graph,
            &requested,
            test_execution_id(),
            &BTreeMap::new(),
        )
        .expect("a deferred dependency does not require its previous value");
        assert!(no_previous_upstream_value[1].request.is_none());
    }

    #[test]
    fn deferred_query_handoff_uses_fresh_value_and_preserves_snapshot_identity() {
        let execution_id = test_execution_id();
        let upstream = scalar_query_node("upstream", "SELECT 11", vec![]);
        let downstream = scalar_query_node(
            "downstream",
            "SELECT $seed",
            vec![argument(
                "seed",
                "upstream",
                VariableGraphScalarType::Integer,
                false,
            )],
        );
        let graph = graph_from_nodes(vec![downstream, upstream]);
        let requested = ["upstream".to_owned(), "downstream".to_owned()];
        let old_values =
            BTreeMap::from([("upstream".to_owned(), VariableGraphLiteral::Integer(7))]);

        let batch = compile_variable_graph_query_request_batch(
            &graph,
            &requested,
            execution_id,
            &old_values,
        )
        .expect("the selected dependency batch compiles");
        let upstream_request = batch[0]
            .request
            .as_ref()
            .expect("the upstream query is ready");
        assert_eq!(upstream_request.execution_id, Some(execution_id));
        assert_eq!(batch[1].variable_id, "downstream");
        assert!(batch[1].request.is_none());
        assert_eq!(
            batch[1].deferred_arguments[0].variable_id, "upstream",
            "a cached upstream value must not be embedded in the deferred request"
        );

        let old_downstream_request =
            compile_variable_graph_query_request(&graph, "downstream", execution_id, &old_values)
                .expect("the old request can be prepared for stale-page comparison");
        assert_eq!(old_downstream_request.parameters["seed"], 7);

        let upstream_page = result_page(
            upstream_request,
            &[("value", SqlResultColumnType::Integer)],
            vec![json!({"value": 11})],
        );
        let VariableGraphValidatedResult::Scalar { value: fresh_value } =
            validate_variable_graph_sql_page(&graph, "upstream", upstream_request, &upstream_page)
                .expect("the upstream result is tied to its request")
        else {
            panic!("upstream query must resolve to a scalar")
        };
        assert_eq!(fresh_value, VariableGraphLiteral::Integer(11));

        let fresh_values = BTreeMap::from([("upstream".to_owned(), fresh_value)]);
        let downstream_request =
            compile_variable_graph_query_request(&graph, "downstream", execution_id, &fresh_values)
                .expect("the deferred query is recompiled after its dependency resolves");
        assert_eq!(downstream_request.execution_id, Some(execution_id));
        assert_eq!(downstream_request.parameters["seed"], 11);
        assert_ne!(
            downstream_request.execution_fingerprint().unwrap(),
            old_downstream_request.execution_fingerprint().unwrap(),
            "a fresh parameter value produces a distinct request fingerprint"
        );

        let fresh_downstream_page = result_page(
            &downstream_request,
            &[("value", SqlResultColumnType::Integer)],
            vec![json!({"value": 11})],
        );
        assert_eq!(
            validate_variable_graph_sql_page(
                &graph,
                "downstream",
                &downstream_request,
                &fresh_downstream_page,
            )
            .expect("the downstream page belongs to its fresh request"),
            VariableGraphValidatedResult::Scalar {
                value: VariableGraphLiteral::Integer(11),
            }
        );

        let stale_downstream_page = result_page(
            &old_downstream_request,
            &[("value", SqlResultColumnType::Integer)],
            vec![json!({"value": 7})],
        );
        assert_eq!(
            validate_variable_graph_sql_page(
                &graph,
                "downstream",
                &downstream_request,
                &stale_downstream_page,
            )
            .expect_err("a page for the old parameter fingerprint is stale")[0]
                .code,
            VariableGraphQueryResultDiagnosticCode::ExecutionMismatch
        );
    }

    #[test]
    fn batch_deduplicates_requested_ids_and_can_select_only_invalidated_queries() {
        let upstream = scalar_query_node("upstream", "SELECT 1", vec![]);
        let downstream = scalar_query_node(
            "downstream",
            "SELECT $seed",
            vec![argument(
                "seed",
                "upstream",
                VariableGraphScalarType::Integer,
                false,
            )],
        );
        let graph = graph_from_nodes(vec![downstream, upstream]);
        let values = BTreeMap::from([("upstream".to_owned(), VariableGraphLiteral::Integer(7))]);

        let deduplicated = compile_variable_graph_query_request_batch(
            &graph,
            &[
                "downstream".to_owned(),
                "upstream".to_owned(),
                "downstream".to_owned(),
            ],
            test_execution_id(),
            &values,
        )
        .expect("duplicate IDs compile once");
        assert_eq!(
            deduplicated
                .iter()
                .map(|item| item.variable_id.as_str())
                .collect::<Vec<_>>(),
            ["upstream", "downstream"]
        );
        assert!(deduplicated[1].request.is_none());
        assert_eq!(
            deduplicated[1].deferred_arguments[0].variable_id,
            "upstream"
        );

        let selected = compile_variable_graph_query_request_batch(
            &graph,
            &["downstream".to_owned()],
            test_execution_id(),
            &values,
        )
        .expect("caller can compile only an invalidated query");
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].variable_id, "downstream");
        assert!(selected[0].deferred_arguments.is_empty());
        assert_eq!(
            selected[0]
                .request
                .as_ref()
                .expect("unselected query dependencies are already resolved")
                .parameters["seed"],
            7
        );
    }

    #[test]
    fn batch_rejects_unknown_and_non_query_ids_without_partial_results() {
        let graph = graph_from_nodes(vec![
            scalar_query_node("query", "SELECT 1", vec![]),
            VariableGraphNode {
                id: "input".to_owned(),
                kind: VariableGraphNodeKind::Input {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                    required: true,
                    default: None,
                },
            }
            .into(),
        ]);
        let diagnostics = compile_variable_graph_query_request_batch(
            &graph,
            &["query".to_owned(), "missing".to_owned(), "input".to_owned()],
            test_execution_id(),
            &BTreeMap::new(),
        )
        .expect_err("invalid requested IDs reject the entire batch");
        assert_eq!(
            diagnostics,
            [
                VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::QueryVariableNotFound,
                    "missing",
                ),
                VariableGraphQueryRequestDiagnostic::for_node(
                    VariableGraphQueryRequestDiagnosticCode::QueryNodeRequired,
                    "input",
                ),
            ]
        );
    }

    #[test]
    fn batch_request_failures_are_atomic_and_preserve_scalar_table_bounds() {
        let query = scalar_query_node(
            "query",
            "SELECT $amount",
            vec![argument(
                "amount",
                "amount_input",
                VariableGraphScalarType::Integer,
                false,
            )],
        );
        let input = VariableGraphNode {
            id: "amount_input".to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: scalar(VariableGraphScalarType::Integer, false),
                required: true,
                default: None,
            },
        };
        let good_query = scalar_query_node("good", "SELECT 1", vec![]);
        let invalid_batch_graph = graph_from_nodes(vec![query, input.into(), good_query]);
        assert_eq!(
            compile_variable_graph_query_request_batch(
                &invalid_batch_graph,
                &["query".to_owned(), "good".to_owned()],
                test_execution_id(),
                &BTreeMap::new(),
            )
            .expect_err("missing one request value rejects the batch"),
            [VariableGraphQueryRequestDiagnostic {
                code: VariableGraphQueryRequestDiagnosticCode::MissingArgumentValue,
                variable_id: "query".to_owned(),
                argument_name: Some("amount".to_owned()),
                reference_id: Some("amount_input".to_owned()),
                domain_code: None,
                cycle: Vec::new(),
            }]
        );

        let table = query_node(
            "table",
            VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT 1 AS value".to_owned(),
            },
            VariableGraphResultType::Table {
                columns: vec![VariableGraphColumn {
                    name: "value".to_owned(),
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                }],
            },
            vec![],
        );
        let scalar = scalar_query_node("scalar", "SELECT 1", vec![]);
        let valid_batch_graph = graph_from_nodes(vec![table, scalar]);
        let requests = compile_variable_graph_query_request_batch(
            &valid_batch_graph,
            &["table".to_owned(), "scalar".to_owned()],
            test_execution_id(),
            &BTreeMap::new(),
        )
        .expect("both requests fit their existing bounds");
        assert!(requests.iter().all(|item| {
            item.request
                .as_ref()
                .is_some_and(|request| request.execution_id == Some(test_execution_id()))
        }));
        assert_eq!(requests[0].variable_id, "scalar");
        assert_eq!(
            requests[0]
                .request
                .as_ref()
                .expect("scalar request has no deferred dependencies")
                .limit,
            MAX_SQL_SCALAR_PROBE_ROWS
        );
        assert_eq!(requests[1].variable_id, "table");
        assert_eq!(
            requests[1]
                .request
                .as_ref()
                .expect("table request has no deferred dependencies")
                .limit,
            DEFAULT_VARIABLE_GRAPH_SQL_PAGE_LIMIT
        );
    }
}
