//! Pure compilation from a validated variable graph to the existing stateless
//! SQL request contract.
//!
//! This module performs no storage reads or authorization. Callers resolve
//! the current values first and the ordinary SQL execution path still checks
//! access to the Space, Forms, and exact Saved SQL revision.

use crate::sql_query::{
    SavedSqlRevisionRef, SqlQueryRequest, MAX_SQL_QUERY_BYTES, MAX_SQL_SCALAR_PROBE_ROWS,
};
use chrono::{DateTime, Datelike, NaiveDate};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, BTreeSet};
use ugoite_domain::composition::{
    compile_variable_graph, VariableGraph, VariableGraphArgumentBinding, VariableGraphDiagnostic,
    VariableGraphLiteral, VariableGraphNode, VariableGraphNodeKind, VariableGraphQueryDefinition,
    VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
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
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VariableGraphQueryRequestDeferredArgument {
    /// SQL parameter name declared by the dependent query.
    pub argument_name: String,
    /// Selected query variable whose fresh result supplies this argument.
    pub variable_id: String,
    /// Type expected by the dependent query's SQL parameter.
    pub value_type: VariableGraphScalarDescriptor,
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
pub fn compile_variable_graph_query_request(
    graph: &VariableGraph,
    query_variable_id: &str,
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

    compile_validated_query_node(graph, node, resolved_values)
}

fn compile_validated_query_node(
    graph: &VariableGraph,
    node: &VariableGraphNode,
    resolved_values: &BTreeMap<String, VariableGraphLiteral>,
) -> Result<SqlQueryRequest, Vec<VariableGraphQueryRequestDiagnostic>> {
    compile_validated_query_node_with_deferred_dependencies(
        graph,
        node,
        resolved_values,
        &BTreeSet::new(),
    )
    .map(|(request, _)| request)
}

fn compile_validated_query_node_with_deferred_dependencies(
    graph: &VariableGraph,
    node: &VariableGraphNode,
    resolved_values: &BTreeMap<String, VariableGraphLiteral>,
    deferred_query_variable_ids: &BTreeSet<String>,
) -> Result<
    (
        SqlQueryRequest,
        Vec<VariableGraphQueryRequestDeferredArgument>,
    ),
    Vec<VariableGraphQueryRequestDiagnostic>,
> {
    let VariableGraphNodeKind::Query {
        query,
        returns,
        arguments,
    } = &node.kind
    else {
        return Err(vec![VariableGraphQueryRequestDiagnostic::for_node(
            VariableGraphQueryRequestDiagnosticCode::QueryNodeRequired,
            &node.id,
        )]);
    };

    let (sql, saved_sql) = match query {
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
    for argument in arguments {
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

        let Some(sql_type) = sql_type_for_graph_type(argument.value_type.scalar_type) else {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::UnsupportedArgumentType,
                &node.id,
                argument,
            ));
            continue;
        };
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
        if source_type.scalar_type != argument.value_type.scalar_type
            || (source_type.nullable && !argument.value_type.nullable)
        {
            diagnostics.push(VariableGraphQueryRequestDiagnostic::for_argument(
                VariableGraphQueryRequestDiagnosticCode::ArgumentTypeMismatch,
                &node.id,
                argument,
            ));
            continue;
        }
        if deferred_query_variable_ids.contains(&argument.variable) {
            deferred_arguments.push(VariableGraphQueryRequestDeferredArgument {
                argument_name: argument.name.clone(),
                variable_id: argument.variable.clone(),
                value_type: argument.value_type,
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
        let Some(value) = bind_literal(argument, value) else {
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
/// results.
pub fn compile_variable_graph_query_request_batch(
    graph: &VariableGraph,
    requested_query_variable_ids: &[String],
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
    argument: &VariableGraphArgumentBinding,
    literal: &VariableGraphLiteral,
) -> Option<Value> {
    if matches!(literal, VariableGraphLiteral::Null) {
        return argument.value_type.nullable.then_some(Value::Null);
    }

    match (argument.value_type.scalar_type, literal) {
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
    use crate::sql_query::MAX_SQL_PARAMETER_BYTES;
    use ugoite_domain::composition::{
        VariableGraphColumn, VariableGraphResultType, VariableGraphScalarDescriptor,
    };

    fn scalar(
        value_type: VariableGraphScalarType,
        nullable: bool,
    ) -> VariableGraphScalarDescriptor {
        VariableGraphScalarDescriptor {
            scalar_type: value_type,
            nullable,
        }
    }

    fn argument(
        name: &str,
        variable: &str,
        value_type: VariableGraphScalarType,
        nullable: bool,
    ) -> VariableGraphArgumentBinding {
        VariableGraphArgumentBinding {
            name: name.to_owned(),
            value_type: scalar(value_type, nullable),
            variable: variable.to_owned(),
        }
    }

    fn inline_node(arguments: Vec<VariableGraphArgumentBinding>) -> VariableGraphNode {
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
        VariableGraphNode {
            id: "rows".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns,
                query: VariableGraphQueryDefinition::InlineSql { sql },
                arguments,
            },
        }
    }

    fn scalar_query_node(
        id: &str,
        sql: &str,
        arguments: Vec<VariableGraphArgumentBinding>,
    ) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: sql.to_owned(),
                },
                arguments,
            },
        }
    }

    fn graph_for(node: VariableGraphNode) -> VariableGraph {
        let mut variables = BTreeMap::new();
        if let VariableGraphNodeKind::Query { arguments, .. } = &node.kind {
            for argument in arguments {
                variables
                    .entry(argument.variable.clone())
                    .or_insert_with(|| VariableGraphNode {
                        id: argument.variable.clone(),
                        kind: VariableGraphNodeKind::Input {
                            value_type: argument.value_type,
                            required: true,
                            default: None,
                        },
                    });
            }
        }
        variables.insert(node.id.clone(), node);
        VariableGraph {
            variables: variables.into_values().collect(),
        }
    }

    fn compile_node(
        node: &VariableGraphNode,
        resolved_values: &BTreeMap<String, VariableGraphLiteral>,
    ) -> Result<SqlQueryRequest, Vec<VariableGraphQueryRequestDiagnostic>> {
        let graph = graph_for(node.clone());
        compile_variable_graph_query_request(&graph, &node.id, resolved_values)
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
        assert_eq!(request.saved_sql, None);
        assert!(request.validate().is_ok());
    }

    #[test]
    fn compiles_saved_sql_to_exact_revision_without_copying_sql_text() {
        let node = VariableGraphNode {
            id: "total".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::SavedSql {
                    entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
                    revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                    column: Some("total".to_owned()),
                },
                arguments: vec![argument(
                    "day",
                    "selected_day",
                    VariableGraphScalarType::Date,
                    false,
                )],
            },
        };
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
        let invalid_sql = VariableGraphNode {
            id: "bad".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: "DELETE FROM entries".to_owned(),
                },
                arguments: vec![],
            },
        };
        assert_eq!(
            compile_node(&invalid_sql, &BTreeMap::new()).expect_err("non-query inline SQL"),
            [graph_diagnostic("bad", "invalid_inline_sql")]
        );

        let missing_saved_identity = VariableGraphNode {
            id: "saved".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::SavedSql {
                    entry_id: None,
                    revision_id: None,
                    column: None,
                },
                arguments: vec![],
            },
        };
        assert_eq!(
            compile_node(&missing_saved_identity, &BTreeMap::new())
                .expect_err("exact Saved SQL identity required"),
            [graph_diagnostic("saved", "invalid_saved_sql_identity")]
        );

        let invalid_saved_identity = VariableGraphNode {
            id: "invalid_saved".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::SavedSql {
                    entry_id: Some("not-a-uuid".to_owned()),
                    revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
                    column: None,
                },
                arguments: vec![],
            },
        };
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
        *query = VariableGraphQueryDefinition::InlineSql {
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
            compile_variable_graph_query_request(&valid_graph, "absent", &BTreeMap::new())
                .expect_err("unknown query reference"),
            [VariableGraphQueryRequestDiagnostic::for_node(
                VariableGraphQueryRequestDiagnosticCode::QueryVariableNotFound,
                "absent",
            )]
        );

        let mut invalid_graph = graph_for(inline_node(vec![]));
        invalid_graph.variables.push(VariableGraphNode {
            id: "bad".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT $missing".to_owned(),
                },
                arguments: vec![],
            },
        });
        assert_eq!(
            compile_variable_graph_query_request(&invalid_graph, "rows", &BTreeMap::new())
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
        let first_graph = VariableGraph {
            variables: vec![downstream.clone(), upstream.clone()],
        };
        let reordered_graph = VariableGraph {
            variables: vec![upstream, downstream],
        };
        let values = BTreeMap::from([("upstream".to_owned(), VariableGraphLiteral::Integer(7))]);
        let requested = vec!["downstream".to_owned(), "upstream".to_owned()];

        let first = compile_variable_graph_query_request_batch(&first_graph, &requested, &values)
            .expect("valid request batch");
        let reordered =
            compile_variable_graph_query_request_batch(&reordered_graph, &requested, &values)
                .expect("reordered graph keeps deterministic batch order");
        let reversed_request = compile_variable_graph_query_request_batch(
            &first_graph,
            &["upstream".to_owned(), "downstream".to_owned()],
            &values,
        )
        .expect("reordered request IDs keep deterministic batch order");

        let batch_ids: Vec<_> = first.iter().map(|item| item.variable_id.as_str()).collect();
        assert_eq!(batch_ids, ["upstream", "downstream"]);
        assert!(first[0].deferred_arguments.is_empty());
        assert!(first[0].request.is_some());
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

        let no_previous_upstream_value =
            compile_variable_graph_query_request_batch(&first_graph, &requested, &BTreeMap::new())
                .expect("a deferred dependency does not require its previous value");
        assert!(no_previous_upstream_value[1].request.is_none());
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
        let graph = VariableGraph {
            variables: vec![downstream, upstream],
        };
        let values = BTreeMap::from([("upstream".to_owned(), VariableGraphLiteral::Integer(7))]);

        let deduplicated = compile_variable_graph_query_request_batch(
            &graph,
            &[
                "downstream".to_owned(),
                "upstream".to_owned(),
                "downstream".to_owned(),
            ],
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

        let selected =
            compile_variable_graph_query_request_batch(&graph, &["downstream".to_owned()], &values)
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
        let graph = VariableGraph {
            variables: vec![
                scalar_query_node("query", "SELECT 1", vec![]),
                VariableGraphNode {
                    id: "input".to_owned(),
                    kind: VariableGraphNodeKind::Input {
                        value_type: scalar(VariableGraphScalarType::Integer, false),
                        required: true,
                        default: None,
                    },
                },
            ],
        };
        let diagnostics = compile_variable_graph_query_request_batch(
            &graph,
            &["query".to_owned(), "missing".to_owned(), "input".to_owned()],
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
        let invalid_batch_graph = VariableGraph {
            variables: vec![query, input, good_query],
        };
        assert_eq!(
            compile_variable_graph_query_request_batch(
                &invalid_batch_graph,
                &["query".to_owned(), "good".to_owned()],
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

        let table = VariableGraphNode {
            id: "table".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Table {
                    columns: vec![VariableGraphColumn {
                        name: "value".to_owned(),
                        value_type: scalar(VariableGraphScalarType::Integer, false),
                    }],
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT 1 AS value".to_owned(),
                },
                arguments: vec![],
            },
        };
        let scalar = scalar_query_node("scalar", "SELECT 1", vec![]);
        let valid_batch_graph = VariableGraph {
            variables: vec![table, scalar],
        };
        let requests = compile_variable_graph_query_request_batch(
            &valid_batch_graph,
            &["table".to_owned(), "scalar".to_owned()],
            &BTreeMap::new(),
        )
        .expect("both requests fit their existing bounds");
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
