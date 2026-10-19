//! Typed variable graph definitions and deterministic dependency compilation.
//!
//! This module is a pure-domain foundation for a future Composition graph.
//! It deliberately has no adapter to the current Composition document, SQL
//! executor, or persistence behavior. Inline SQL syntax is checked with the
//! shared domain SQL-document parser before a graph can compile.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// A graph of shared Composition values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraph {
    /// Variables in authoring order. Compilation sorts identities where order
    /// is semantically irrelevant.
    pub variables: Vec<VariableGraphNode>,
}

/// One named value in a variable graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphNode {
    /// Stable graph-local identity.
    pub id: String,
    /// The node's definition and, for queries, its explicit dependencies.
    pub kind: VariableGraphNodeKind,
}

/// A value's definition in the variable graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableGraphNodeKind {
    /// A typed value owned by the Composition definition.
    Constant {
        value_type: VariableGraphScalarDescriptor,
        value: VariableGraphLiteral,
    },
    /// A scalar supplied by an explicitly placed input control.
    Input {
        value_type: VariableGraphScalarDescriptor,
        required: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<VariableGraphLiteral>,
    },
    /// A computed value with explicitly bound SQL arguments.
    Query {
        returns: VariableGraphResultType,
        query: VariableGraphQueryDefinition,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        arguments: Vec<VariableGraphArgumentBinding>,
    },
}

/// The durable definition that produces a query variable's value.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableGraphQueryDefinition {
    /// SQL text stored with the Composition query variable.
    InlineSql { sql: String },
    /// A Saved SQL Entry pinned to one exact revision.
    SavedSql {
        /// UUID of the Saved SQL Entry. Missing and invalid IDs are rejected
        /// by graph compilation with a stable diagnostic.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        entry_id: Option<String>,
        /// UUID of the exact Saved SQL revision. This never follows latest.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision_id: Option<String>,
        /// Optional output column selected for scalar query results.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        column: Option<String>,
    },
}

/// A scalar's domain type and nullability.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphScalarDescriptor {
    /// Portable scalar kind.
    pub scalar_type: VariableGraphScalarType,
    /// Whether this value may be SQL NULL.
    pub nullable: bool,
}

/// Scalar kinds shared by constants, inputs, query arguments, and results.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableGraphScalarType {
    String,
    Boolean,
    Integer,
    Float,
    Decimal,
    Date,
    Timestamp,
    Json,
}

/// A typed constant or input default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum VariableGraphLiteral {
    String(String),
    Boolean(bool),
    Integer(i64),
    /// A finite IEEE-754 value. Non-finite values are rejected by the compiler.
    Float(f64),
    /// Decimal text preserves exact precision.
    Decimal(String),
    /// ISO `YYYY-MM-DD` date text.
    Date(String),
    /// RFC 3339 timestamp text with an explicit offset.
    Timestamp(String),
    Json(Value),
    Null,
}

/// The declared result contract of a query variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableGraphResultType {
    Scalar {
        value_type: VariableGraphScalarDescriptor,
    },
    Table {
        columns: Vec<VariableGraphColumn>,
    },
}

/// One named column in a table result.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphColumn {
    pub name: String,
    pub value_type: VariableGraphScalarDescriptor,
}

/// A declared SQL argument explicitly connected to one scalar graph value.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphArgumentBinding {
    /// Parameter name declared by the query.
    pub name: String,
    /// Type expected by the query's parameter declaration.
    pub value_type: VariableGraphScalarDescriptor,
    /// Graph variable supplying this argument.
    pub variable: String,
}

/// Stable machine-readable diagnostics produced by graph compilation.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableGraphDiagnosticCode {
    InvalidVariableId,
    DuplicateVariableId,
    InvalidArgumentName,
    DuplicateArgumentName,
    InvalidConstantValue,
    InvalidInputDefault,
    InvalidResultDescriptor,
    InvalidInlineSql,
    InvalidSavedSqlIdentity,
    InvalidSavedSqlColumn,
    MissingArgumentBinding,
    UnusedArgumentBinding,
    MissingVariableReference,
    VariableTypeMismatch,
    DependencyCycle,
}

impl VariableGraphDiagnosticCode {
    /// Return the stable wire spelling for this diagnostic.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidVariableId => "invalid_variable_id",
            Self::DuplicateVariableId => "duplicate_variable_id",
            Self::InvalidArgumentName => "invalid_argument_name",
            Self::DuplicateArgumentName => "duplicate_argument_name",
            Self::InvalidConstantValue => "invalid_constant_value",
            Self::InvalidInputDefault => "invalid_input_default",
            Self::InvalidResultDescriptor => "invalid_result_descriptor",
            Self::InvalidInlineSql => "invalid_inline_sql",
            Self::InvalidSavedSqlIdentity => "invalid_saved_sql_identity",
            Self::InvalidSavedSqlColumn => "invalid_saved_sql_column",
            Self::MissingArgumentBinding => "missing_argument_binding",
            Self::UnusedArgumentBinding => "unused_argument_binding",
            Self::MissingVariableReference => "missing_variable_reference",
            Self::VariableTypeMismatch => "variable_type_mismatch",
            Self::DependencyCycle => "dependency_cycle",
        }
    }
}

/// A stable graph diagnostic with only the relevant identity context.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphDiagnostic {
    pub code: VariableGraphDiagnosticCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycle: Vec<String>,
}

impl VariableGraphDiagnostic {
    fn new(code: VariableGraphDiagnosticCode) -> Self {
        Self {
            code,
            variable_id: None,
            argument_name: None,
            reference_id: None,
            cycle: Vec::new(),
        }
    }

    fn for_variable(code: VariableGraphDiagnosticCode, variable_id: &str) -> Self {
        let mut diagnostic = Self::new(code);
        diagnostic.variable_id = Some(variable_id.to_owned());
        diagnostic
    }

    fn for_argument(
        code: VariableGraphDiagnosticCode,
        variable_id: &str,
        argument_name: &str,
        reference_id: Option<&str>,
    ) -> Self {
        let mut diagnostic = Self::for_variable(code, variable_id);
        diagnostic.argument_name = Some(argument_name.to_owned());
        diagnostic.reference_id = reference_id.map(str::to_owned);
        diagnostic
    }
}

/// A validated graph with deterministic execution dependencies.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CompiledVariableGraph {
    /// A deterministic topological order. Independent nodes are ordered by ID.
    pub topological_order: Vec<String>,
    /// Query variable ID to its unique, sorted dependency IDs.
    pub dependencies: BTreeMap<String, Vec<String>>,
}

/// Validate a graph and derive its deterministic dependency order.
pub fn compile_variable_graph(
    graph: &VariableGraph,
) -> Result<CompiledVariableGraph, Vec<VariableGraphDiagnostic>> {
    let mut id_counts = BTreeMap::<String, usize>::new();
    let mut diagnostics = Vec::new();
    for node in &graph.variables {
        if node.id.trim().is_empty() {
            diagnostics.push(VariableGraphDiagnostic::for_variable(
                VariableGraphDiagnosticCode::InvalidVariableId,
                &node.id,
            ));
        } else {
            *id_counts.entry(node.id.clone()).or_default() += 1;
        }
    }
    diagnostics.extend(
        id_counts
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(id, _)| {
                VariableGraphDiagnostic::for_variable(
                    VariableGraphDiagnosticCode::DuplicateVariableId,
                    id,
                )
            }),
    );
    if !diagnostics.is_empty() {
        diagnostics.sort();
        diagnostics.dedup();
        return Err(diagnostics);
    }

    let nodes: BTreeMap<&str, &VariableGraphNode> = graph
        .variables
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();

    for node in nodes.values().copied() {
        match &node.kind {
            VariableGraphNodeKind::Constant { value_type, value } => {
                if !literal_matches(value_type, value) {
                    diagnostics.push(VariableGraphDiagnostic::for_variable(
                        VariableGraphDiagnosticCode::InvalidConstantValue,
                        &node.id,
                    ));
                }
            }
            VariableGraphNodeKind::Input {
                value_type,
                default,
                ..
            } => {
                if default
                    .as_ref()
                    .is_some_and(|value| !literal_matches(value_type, value))
                {
                    diagnostics.push(VariableGraphDiagnostic::for_variable(
                        VariableGraphDiagnosticCode::InvalidInputDefault,
                        &node.id,
                    ));
                }
            }
            VariableGraphNodeKind::Query {
                returns,
                query,
                arguments,
            } => {
                if !result_descriptor_is_valid(returns) {
                    diagnostics.push(VariableGraphDiagnostic::for_variable(
                        VariableGraphDiagnosticCode::InvalidResultDescriptor,
                        &node.id,
                    ));
                }
                let inline_parameters = match query {
                    VariableGraphQueryDefinition::InlineSql { sql } => {
                        let document = crate::sql_document::parse_sql_document(sql);
                        if document.status != crate::sql_document::SqlDocumentStatus::Parsed {
                            diagnostics.push(VariableGraphDiagnostic::for_variable(
                                VariableGraphDiagnosticCode::InvalidInlineSql,
                                &node.id,
                            ));
                            None
                        } else {
                            match inline_sql_parameter_names(&document) {
                                Some(parameters) => Some(parameters),
                                None => {
                                    diagnostics.push(VariableGraphDiagnostic::for_variable(
                                        VariableGraphDiagnosticCode::InvalidInlineSql,
                                        &node.id,
                                    ));
                                    None
                                }
                            }
                        }
                    }
                    VariableGraphQueryDefinition::SavedSql {
                        entry_id,
                        revision_id,
                        column,
                    } => {
                        if !entry_id.as_deref().is_some_and(valid_saved_sql_identity)
                            || !revision_id.as_deref().is_some_and(valid_saved_sql_identity)
                        {
                            diagnostics.push(VariableGraphDiagnostic::for_variable(
                                VariableGraphDiagnosticCode::InvalidSavedSqlIdentity,
                                &node.id,
                            ));
                        }
                        if column.as_deref().is_some_and(|name| name.trim().is_empty())
                            || (column.is_some()
                                && !matches!(returns, VariableGraphResultType::Scalar { .. }))
                        {
                            diagnostics.push(VariableGraphDiagnostic::for_variable(
                                VariableGraphDiagnosticCode::InvalidSavedSqlColumn,
                                &node.id,
                            ));
                        }
                        None
                    }
                };
                let mut argument_counts = BTreeMap::<&str, usize>::new();
                for argument in arguments {
                    if !valid_argument_name(&argument.name) {
                        diagnostics.push(VariableGraphDiagnostic::for_argument(
                            VariableGraphDiagnosticCode::InvalidArgumentName,
                            &node.id,
                            &argument.name,
                            Some(&argument.variable),
                        ));
                    } else {
                        *argument_counts.entry(&argument.name).or_default() += 1;
                    }
                }
                diagnostics.extend(argument_counts.iter().filter(|(_, count)| **count > 1).map(
                    |(name, _)| {
                        VariableGraphDiagnostic::for_argument(
                            VariableGraphDiagnosticCode::DuplicateArgumentName,
                            &node.id,
                            name,
                            None,
                        )
                    },
                ));
                if let Some(parameters) = inline_parameters {
                    for name in parameters
                        .iter()
                        .filter(|name| !argument_counts.contains_key(name.as_str()))
                    {
                        diagnostics.push(VariableGraphDiagnostic::for_argument(
                            VariableGraphDiagnosticCode::MissingArgumentBinding,
                            &node.id,
                            name,
                            None,
                        ));
                    }
                    for name in argument_counts
                        .keys()
                        .filter(|name| !parameters.contains(**name))
                    {
                        diagnostics.push(VariableGraphDiagnostic::for_argument(
                            VariableGraphDiagnosticCode::UnusedArgumentBinding,
                            &node.id,
                            name,
                            None,
                        ));
                    }
                }
            }
        }
    }
    if !diagnostics.is_empty() {
        diagnostics.sort();
        diagnostics.dedup();
        return Err(diagnostics);
    }

    let mut dependencies = BTreeMap::<String, Vec<String>>::new();
    for node in nodes.values().copied() {
        let mut node_dependencies = BTreeSet::new();
        if let VariableGraphNodeKind::Query { arguments, .. } = &node.kind {
            for argument in arguments {
                let Some(target) = nodes.get(argument.variable.as_str()).copied() else {
                    diagnostics.push(VariableGraphDiagnostic::for_argument(
                        VariableGraphDiagnosticCode::MissingVariableReference,
                        &node.id,
                        &argument.name,
                        Some(&argument.variable),
                    ));
                    continue;
                };
                let Some(actual_type) = scalar_type_of(&target.kind) else {
                    diagnostics.push(VariableGraphDiagnostic::for_argument(
                        VariableGraphDiagnosticCode::VariableTypeMismatch,
                        &node.id,
                        &argument.name,
                        Some(&argument.variable),
                    ));
                    continue;
                };
                if !scalar_type_assignable(argument.value_type, actual_type) {
                    diagnostics.push(VariableGraphDiagnostic::for_argument(
                        VariableGraphDiagnosticCode::VariableTypeMismatch,
                        &node.id,
                        &argument.name,
                        Some(&argument.variable),
                    ));
                    continue;
                }
                node_dependencies.insert(argument.variable.clone());
            }
        }
        dependencies.insert(node.id.clone(), node_dependencies.into_iter().collect());
    }
    if !diagnostics.is_empty() {
        diagnostics.sort();
        diagnostics.dedup();
        return Err(diagnostics);
    }

    let topological_order = topological_order(&dependencies).ok_or_else(|| {
        let cycle = find_dependency_cycle(&dependencies).unwrap_or_default();
        vec![VariableGraphDiagnostic {
            code: VariableGraphDiagnosticCode::DependencyCycle,
            variable_id: cycle.first().cloned(),
            argument_name: None,
            reference_id: None,
            cycle,
        }]
    })?;

    Ok(CompiledVariableGraph {
        topological_order,
        dependencies,
    })
}

fn result_descriptor_is_valid(result: &VariableGraphResultType) -> bool {
    match result {
        VariableGraphResultType::Scalar { .. } => true,
        VariableGraphResultType::Table { columns } => {
            if columns.is_empty() {
                return false;
            }
            let mut names = BTreeSet::new();
            columns
                .iter()
                .all(|column| !column.name.trim().is_empty() && names.insert(column.name.as_str()))
        }
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

fn valid_saved_sql_identity(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}

fn inline_sql_parameter_names(
    document: &crate::sql_document::SqlDocument,
) -> Option<BTreeSet<String>> {
    use crate::sql_document::{SqlDocumentStatus, SqlDocumentTokenKind};

    if document.status != SqlDocumentStatus::Parsed {
        return None;
    }

    let mut source_cursor = 0;
    let mut parameters = BTreeSet::new();
    for token in &document.tokens {
        if token.range.start != source_cursor
            || token.range.start >= token.range.end
            || document.source_slice(token.range).is_none()
        {
            return None;
        }
        source_cursor = token.range.end;
        if token.kind != SqlDocumentTokenKind::Placeholder {
            continue;
        }
        let name = document.source_slice(token.range)?.strip_prefix('$')?;
        if !valid_argument_name(name) {
            return None;
        }
        parameters.insert(name.to_owned());
    }
    (source_cursor == document.source.len()).then_some(parameters)
}

fn scalar_type_of(kind: &VariableGraphNodeKind) -> Option<VariableGraphScalarDescriptor> {
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

/// A nullable value can satisfy a nullable argument; a non-null value can
/// satisfy either nullability, but a nullable value cannot satisfy a required
/// non-null argument.
fn scalar_type_assignable(
    expected: VariableGraphScalarDescriptor,
    actual: VariableGraphScalarDescriptor,
) -> bool {
    expected.scalar_type == actual.scalar_type && (!actual.nullable || expected.nullable)
}

pub(crate) fn literal_matches(
    descriptor: &VariableGraphScalarDescriptor,
    literal: &VariableGraphLiteral,
) -> bool {
    if matches!(literal, VariableGraphLiteral::Null) {
        return descriptor.nullable;
    }
    match (descriptor.scalar_type, literal) {
        (VariableGraphScalarType::String, VariableGraphLiteral::String(value)) => {
            super::valid_variable_string(value)
        }
        (VariableGraphScalarType::Boolean, VariableGraphLiteral::Boolean(_)) => true,
        (VariableGraphScalarType::Integer, VariableGraphLiteral::Integer(value)) => {
            (-super::MAX_COMPOSITION_VARIABLE_INTEGER..=super::MAX_COMPOSITION_VARIABLE_INTEGER)
                .contains(value)
        }
        (VariableGraphScalarType::Float, VariableGraphLiteral::Float(value)) => value.is_finite(),
        (VariableGraphScalarType::Decimal, VariableGraphLiteral::Decimal(value)) => {
            decimal_text_is_valid(value)
        }
        (VariableGraphScalarType::Date, VariableGraphLiteral::Date(value)) => {
            super::valid_variable_date(value)
        }
        (VariableGraphScalarType::Timestamp, VariableGraphLiteral::Timestamp(value)) => {
            super::valid_variable_timestamp(value)
        }
        (VariableGraphScalarType::Json, VariableGraphLiteral::Json(value)) => !value.is_null(),
        _ => false,
    }
}

/// Query results use the same scalar types and numeric/date constraints as
/// declared values, but SQL output strings are not user-entered variables and
/// therefore do not inherit the input size/control-character restrictions.
fn query_result_literal_matches(
    descriptor: &VariableGraphScalarDescriptor,
    literal: &VariableGraphLiteral,
) -> bool {
    if matches!(literal, VariableGraphLiteral::String(_))
        && descriptor.scalar_type == VariableGraphScalarType::String
    {
        return true;
    }
    literal_matches(descriptor, literal)
}

/// Stable diagnostics for a query result that violates its declared contract.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableGraphResultDiagnosticCode {
    InvalidExpectedDescriptor,
    ScalarColumnCountMismatch,
    ScalarNoRows,
    ScalarMultipleRows,
    TableColumnCountMismatch,
    ColumnNameMismatch,
    ColumnTypeMismatch,
    RowWidthMismatch,
    NullNotAllowed,
    ValueTypeMismatch,
}

impl VariableGraphResultDiagnosticCode {
    /// Return the stable wire spelling for this result diagnostic.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidExpectedDescriptor => "invalid_expected_descriptor",
            Self::ScalarColumnCountMismatch => "scalar_column_count_mismatch",
            Self::ScalarNoRows => "scalar_no_rows",
            Self::ScalarMultipleRows => "scalar_multiple_rows",
            Self::TableColumnCountMismatch => "table_column_count_mismatch",
            Self::ColumnNameMismatch => "column_name_mismatch",
            Self::ColumnTypeMismatch => "column_type_mismatch",
            Self::RowWidthMismatch => "row_width_mismatch",
            Self::NullNotAllowed => "null_not_allowed",
            Self::ValueTypeMismatch => "value_type_mismatch",
        }
    }
}

/// A result-contract failure with optional row and column location.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphResultDiagnostic {
    pub code: VariableGraphResultDiagnosticCode,
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
    pub expected_value_type: Option<VariableGraphScalarDescriptor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_value_type: Option<VariableGraphScalarDescriptor>,
}

impl VariableGraphResultDiagnostic {
    fn new(code: VariableGraphResultDiagnosticCode) -> Self {
        Self {
            code,
            expected_count: None,
            observed_count: None,
            row_index: None,
            column_index: None,
            column_name: None,
            expected_column_name: None,
            expected_value_type: None,
            observed_value_type: None,
        }
    }

    fn for_count(
        code: VariableGraphResultDiagnosticCode,
        expected_count: usize,
        observed_count: usize,
    ) -> Self {
        Self {
            code,
            expected_count: Some(expected_count),
            observed_count: Some(observed_count),
            row_index: None,
            column_index: None,
            column_name: None,
            expected_column_name: None,
            expected_value_type: None,
            observed_value_type: None,
        }
    }

    fn for_column(
        code: VariableGraphResultDiagnosticCode,
        column_index: usize,
        expected_column_name: Option<&str>,
        expected_value_type: Option<VariableGraphScalarDescriptor>,
        observed_column: &VariableGraphColumn,
    ) -> Self {
        Self {
            code,
            expected_count: None,
            observed_count: None,
            row_index: None,
            column_index: Some(column_index),
            column_name: Some(observed_column.name.clone()),
            expected_column_name: expected_column_name.map(str::to_owned),
            expected_value_type,
            observed_value_type: Some(observed_column.value_type),
        }
    }

    fn for_row(
        code: VariableGraphResultDiagnosticCode,
        row_index: usize,
        column_index: Option<usize>,
        column_name: Option<&str>,
    ) -> Self {
        Self {
            code,
            expected_count: None,
            observed_count: None,
            row_index: Some(row_index),
            column_index,
            column_name: column_name.map(str::to_owned),
            expected_column_name: None,
            expected_value_type: None,
            observed_value_type: None,
        }
    }

    fn for_row_count(
        code: VariableGraphResultDiagnosticCode,
        row_index: usize,
        expected_count: usize,
        observed_count: usize,
    ) -> Self {
        let mut diagnostic = Self::for_row(code, row_index, None, None);
        diagnostic.expected_count = Some(expected_count);
        diagnostic.observed_count = Some(observed_count);
        diagnostic
    }
}

/// The checked value returned by a query variable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableGraphValidatedResult {
    /// A query declared scalar returned one row with one typed cell.
    Scalar { value: VariableGraphLiteral },
    /// A query declared table returned a matching schema and typed rows.
    Table {
        columns: Vec<VariableGraphColumn>,
        rows: Vec<Vec<VariableGraphLiteral>>,
    },
}

/// Validate an executed query result against its declared scalar or table type.
///
/// This function does not execute SQL. Callers provide the planner's observed
/// schema and the result values; a scalar query only succeeds with one column
/// and exactly one row. SQL NULL stays represented as `VariableGraphLiteral::Null`
/// and can only satisfy a nullable scalar descriptor.
pub fn validate_variable_graph_result(
    expected: &VariableGraphResultType,
    observed_columns: &[VariableGraphColumn],
    rows: &[Vec<VariableGraphLiteral>],
) -> Result<VariableGraphValidatedResult, Vec<VariableGraphResultDiagnostic>> {
    if !result_descriptor_is_valid(expected) {
        return Err(vec![VariableGraphResultDiagnostic::new(
            VariableGraphResultDiagnosticCode::InvalidExpectedDescriptor,
        )]);
    }

    let mut diagnostics = Vec::new();
    match expected {
        VariableGraphResultType::Scalar { value_type } => {
            if observed_columns.len() != 1 {
                diagnostics.push(VariableGraphResultDiagnostic::for_count(
                    VariableGraphResultDiagnosticCode::ScalarColumnCountMismatch,
                    1,
                    observed_columns.len(),
                ));
            }
            match rows.len() {
                0 => diagnostics.push(VariableGraphResultDiagnostic::for_count(
                    VariableGraphResultDiagnosticCode::ScalarNoRows,
                    1,
                    0,
                )),
                1 => {}
                _ => diagnostics.push(VariableGraphResultDiagnostic::for_count(
                    VariableGraphResultDiagnosticCode::ScalarMultipleRows,
                    1,
                    rows.len(),
                )),
            }

            if let Some(observed_column) = observed_columns.first() {
                if !scalar_type_assignable(*value_type, observed_column.value_type) {
                    diagnostics.push(VariableGraphResultDiagnostic::for_column(
                        VariableGraphResultDiagnosticCode::ColumnTypeMismatch,
                        0,
                        None,
                        Some(*value_type),
                        observed_column,
                    ));
                }
            }

            if rows.len() == 1 {
                let row = &rows[0];
                if row.len() != 1 {
                    diagnostics.push(VariableGraphResultDiagnostic::for_row_count(
                        VariableGraphResultDiagnosticCode::RowWidthMismatch,
                        0,
                        1,
                        row.len(),
                    ));
                } else if observed_columns
                    .first()
                    .is_some_and(|column| column.value_type.scalar_type == value_type.scalar_type)
                    && (!query_result_literal_matches(&observed_columns[0].value_type, &row[0])
                        || !query_result_literal_matches(value_type, &row[0]))
                {
                    diagnostics.push(value_diagnostic(
                        &row[0],
                        0,
                        0,
                        Some(observed_columns[0].name.as_str()),
                    ));
                }
            }

            if diagnostics.is_empty() {
                Ok(VariableGraphValidatedResult::Scalar {
                    value: rows[0][0].clone(),
                })
            } else {
                diagnostics.sort();
                diagnostics.dedup();
                Err(diagnostics)
            }
        }
        VariableGraphResultType::Table {
            columns: expected_columns,
        } => {
            if observed_columns.len() != expected_columns.len() {
                diagnostics.push(VariableGraphResultDiagnostic::for_count(
                    VariableGraphResultDiagnosticCode::TableColumnCountMismatch,
                    expected_columns.len(),
                    observed_columns.len(),
                ));
            }

            for (index, expected_column) in expected_columns.iter().enumerate() {
                let Some(observed_column) = observed_columns.get(index) else {
                    continue;
                };
                if expected_column.name != observed_column.name {
                    diagnostics.push(VariableGraphResultDiagnostic::for_column(
                        VariableGraphResultDiagnosticCode::ColumnNameMismatch,
                        index,
                        Some(&expected_column.name),
                        Some(expected_column.value_type),
                        observed_column,
                    ));
                }
                if !scalar_type_assignable(expected_column.value_type, observed_column.value_type) {
                    diagnostics.push(VariableGraphResultDiagnostic::for_column(
                        VariableGraphResultDiagnosticCode::ColumnTypeMismatch,
                        index,
                        Some(&expected_column.name),
                        Some(expected_column.value_type),
                        observed_column,
                    ));
                }
            }

            for (row_index, row) in rows.iter().enumerate() {
                if row.len() != observed_columns.len() {
                    diagnostics.push(VariableGraphResultDiagnostic::for_row_count(
                        VariableGraphResultDiagnosticCode::RowWidthMismatch,
                        row_index,
                        observed_columns.len(),
                        row.len(),
                    ));
                }
                for (column_index, value) in row.iter().enumerate() {
                    let Some(expected_column) = expected_columns.get(column_index) else {
                        continue;
                    };
                    let Some(observed_column) = observed_columns.get(column_index) else {
                        continue;
                    };
                    if expected_column.name != observed_column.name
                        || expected_column.value_type.scalar_type
                            != observed_column.value_type.scalar_type
                    {
                        continue;
                    }
                    if !query_result_literal_matches(&observed_column.value_type, value)
                        || !query_result_literal_matches(&expected_column.value_type, value)
                    {
                        diagnostics.push(value_diagnostic(
                            value,
                            row_index,
                            column_index,
                            Some(expected_column.name.as_str()),
                        ));
                    }
                }
            }

            if diagnostics.is_empty() {
                Ok(VariableGraphValidatedResult::Table {
                    columns: expected_columns.clone(),
                    rows: rows.to_vec(),
                })
            } else {
                diagnostics.sort();
                diagnostics.dedup();
                Err(diagnostics)
            }
        }
    }
}

fn value_diagnostic(
    value: &VariableGraphLiteral,
    row_index: usize,
    column_index: usize,
    column_name: Option<&str>,
) -> VariableGraphResultDiagnostic {
    VariableGraphResultDiagnostic::for_row(
        if matches!(value, VariableGraphLiteral::Null) {
            VariableGraphResultDiagnosticCode::NullNotAllowed
        } else {
            VariableGraphResultDiagnosticCode::ValueTypeMismatch
        },
        row_index,
        Some(column_index),
        column_name,
    )
}

fn decimal_text_is_valid(value: &str) -> bool {
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !integer.is_empty()
        && integer.bytes().all(|byte| byte.is_ascii_digit())
        && parts.next().is_none()
        && fraction.is_none_or(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn topological_order(dependencies: &BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
    let mut dependents: BTreeMap<String, Vec<String>> = dependencies
        .keys()
        .cloned()
        .map(|id| (id, Vec::new()))
        .collect();
    let mut remaining: BTreeMap<String, usize> = dependencies
        .iter()
        .map(|(id, required)| (id.clone(), required.len()))
        .collect();
    for (node, required) in dependencies {
        for dependency in required {
            dependents
                .get_mut(dependency)
                .expect("validated dependencies refer to graph nodes")
                .push(node.clone());
        }
    }
    for children in dependents.values_mut() {
        children.sort();
    }

    let mut ready: BTreeSet<String> = remaining
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
        .collect();
    let mut order = Vec::with_capacity(dependencies.len());
    while let Some(id) = ready.pop_first() {
        order.push(id.clone());
        for dependent in dependents.get(&id).expect("every node has dependents") {
            let count = remaining
                .get_mut(dependent)
                .expect("every dependent is a graph node");
            *count -= 1;
            if *count == 0 {
                ready.insert(dependent.clone());
            }
        }
    }
    (order.len() == dependencies.len()).then_some(order)
}

fn find_dependency_cycle(dependencies: &BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
    let mut state = BTreeMap::<String, u8>::new();
    let mut path = Vec::<String>::new();
    let mut path_positions = BTreeMap::<String, usize>::new();

    for root in dependencies.keys() {
        if state.contains_key(root) {
            continue;
        }
        state.insert(root.clone(), 1);
        path_positions.insert(root.clone(), path.len());
        path.push(root.clone());
        let mut frames = vec![(root.clone(), 0usize)];

        while let Some((current, next_index)) = frames.last_mut() {
            let neighbors = dependencies
                .get(current)
                .expect("every traversal frame is a graph node");
            if *next_index >= neighbors.len() {
                let completed = frames.pop().expect("frame exists").0;
                state.insert(completed.clone(), 2);
                path_positions.remove(&completed);
                path.pop();
                continue;
            }

            let neighbor = neighbors[*next_index].clone();
            *next_index += 1;
            match state.get(&neighbor).copied() {
                Some(1) => {
                    let cycle_start = *path_positions
                        .get(&neighbor)
                        .expect("active node has a path position");
                    let mut cycle = path[cycle_start..].to_vec();
                    cycle.push(neighbor);
                    return Some(cycle);
                }
                Some(2) => {}
                None => {
                    state.insert(neighbor.clone(), 1);
                    path_positions.insert(neighbor.clone(), path.len());
                    path.push(neighbor.clone());
                    frames.push((neighbor, 0));
                }
                _ => unreachable!("only active and completed states are stored"),
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(
        scalar_type: VariableGraphScalarType,
        nullable: bool,
    ) -> VariableGraphScalarDescriptor {
        VariableGraphScalarDescriptor {
            scalar_type,
            nullable,
        }
    }

    fn input(id: &str, scalar_type: VariableGraphScalarType, nullable: bool) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: scalar(scalar_type, nullable),
                required: true,
                default: None,
            },
        }
    }

    fn query(id: &str, dependencies: Vec<VariableGraphArgumentBinding>) -> VariableGraphNode {
        let sql = if dependencies.is_empty() {
            "SELECT 1".to_owned()
        } else {
            format!(
                "SELECT {}",
                dependencies
                    .iter()
                    .map(|argument| format!("${}", argument.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::InlineSql { sql },
                arguments: dependencies,
            },
        }
    }

    fn binding(
        name: &str,
        variable: &str,
        scalar_type: VariableGraphScalarType,
    ) -> VariableGraphArgumentBinding {
        VariableGraphArgumentBinding {
            name: name.to_owned(),
            value_type: scalar(scalar_type, false),
            variable: variable.to_owned(),
        }
    }

    fn graph(variables: Vec<VariableGraphNode>) -> VariableGraph {
        VariableGraph { variables }
    }

    fn query_definition(definition: VariableGraphQueryDefinition) -> VariableGraphNode {
        VariableGraphNode {
            id: "result".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: definition,
                arguments: Vec::new(),
            },
        }
    }

    #[test]
    fn accepts_inline_query_and_exact_saved_sql_query_definitions() {
        let inline = query_definition(VariableGraphQueryDefinition::InlineSql {
            sql: "SELECT 1".to_owned(),
        });
        assert!(compile_variable_graph(&graph(vec![inline])).is_ok());

        let saved_sql = query_definition(VariableGraphQueryDefinition::SavedSql {
            entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
            revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
            column: Some("total".to_owned()),
        });
        assert!(compile_variable_graph(&graph(vec![saved_sql])).is_ok());
    }

    #[test]
    fn rejects_malformed_non_query_and_multi_statement_inline_sql_stably() {
        for sql in ["", "SELECT (", "DELETE FROM entries", "SELECT 1; SELECT 2"] {
            let invalid = query_definition(VariableGraphQueryDefinition::InlineSql {
                sql: sql.to_owned(),
            });
            let diagnostics = compile_variable_graph(&graph(vec![invalid]))
                .expect_err("inline SQL must be one syntactically valid query");
            assert_eq!(
                diagnostics,
                [VariableGraphDiagnostic::for_variable(
                    VariableGraphDiagnosticCode::InvalidInlineSql,
                    "result"
                )],
                "SQL: {sql:?}"
            );
        }
        assert_eq!(
            VariableGraphDiagnosticCode::InvalidInlineSql.as_str(),
            "invalid_inline_sql"
        );
    }

    #[test]
    fn rejects_missing_or_invalid_saved_sql_identity_stably() {
        let cases = [
            (
                None,
                Some("00000000-0000-0000-0000-000000000002".to_owned()),
            ),
            (
                Some("00000000-0000-0000-0000-000000000001".to_owned()),
                None,
            ),
            (
                Some("not-a-uuid".to_owned()),
                Some("00000000-0000-0000-0000-000000000002".to_owned()),
            ),
            (
                Some("00000000-0000-0000-0000-000000000001".to_owned()),
                Some("bad".to_owned()),
            ),
        ];
        for (entry_id, revision_id) in cases {
            let invalid = query_definition(VariableGraphQueryDefinition::SavedSql {
                entry_id,
                revision_id,
                column: None,
            });
            assert_eq!(
                compile_variable_graph(&graph(vec![invalid])).expect_err("bad identity"),
                [VariableGraphDiagnostic::for_variable(
                    VariableGraphDiagnosticCode::InvalidSavedSqlIdentity,
                    "result"
                )]
            );
        }
        assert_eq!(
            VariableGraphDiagnosticCode::InvalidSavedSqlIdentity.as_str(),
            "invalid_saved_sql_identity"
        );
    }

    #[test]
    fn rejects_blank_saved_sql_output_column() {
        let invalid = query_definition(VariableGraphQueryDefinition::SavedSql {
            entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
            revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
            column: Some("  ".to_owned()),
        });
        assert_eq!(
            compile_variable_graph(&graph(vec![invalid])).expect_err("blank output column"),
            [VariableGraphDiagnostic::for_variable(
                VariableGraphDiagnosticCode::InvalidSavedSqlColumn,
                "result"
            )]
        );

        let mut table = query_definition(VariableGraphQueryDefinition::SavedSql {
            entry_id: Some("00000000-0000-0000-0000-000000000001".to_owned()),
            revision_id: Some("00000000-0000-0000-0000-000000000002".to_owned()),
            column: Some("total".to_owned()),
        });
        let VariableGraphNodeKind::Query { returns, .. } = &mut table.kind else {
            unreachable!("helper constructs a query node")
        };
        *returns = VariableGraphResultType::Table {
            columns: vec![VariableGraphColumn {
                name: "total".to_owned(),
                value_type: scalar(VariableGraphScalarType::Integer, false),
            }],
        };
        assert_eq!(
            compile_variable_graph(&graph(vec![table]))
                .expect_err("table has no scalar column selector"),
            [VariableGraphDiagnostic::for_variable(
                VariableGraphDiagnosticCode::InvalidSavedSqlColumn,
                "result"
            )]
        );
    }

    #[test]
    fn inline_sql_parameters_require_exact_explicit_bindings() {
        let missing = query_definition(VariableGraphQueryDefinition::InlineSql {
            sql: "SELECT $day".to_owned(),
        });
        assert_eq!(
            compile_variable_graph(&graph(vec![missing]))
                .expect_err("every SQL parameter needs an explicit binding"),
            [VariableGraphDiagnostic::for_argument(
                VariableGraphDiagnosticCode::MissingArgumentBinding,
                "result",
                "day",
                None,
            )]
        );

        let unused = VariableGraphNode {
            id: "result".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT 1".to_owned(),
                },
                arguments: vec![binding("day", "input", VariableGraphScalarType::Integer)],
            },
        };
        let unused_result = compile_variable_graph(&graph(vec![
            input("input", VariableGraphScalarType::Integer, false),
            unused,
        ]))
        .expect_err("every inline SQL binding must be used");
        assert_eq!(
            unused_result,
            [VariableGraphDiagnostic::for_argument(
                VariableGraphDiagnosticCode::UnusedArgumentBinding,
                "result",
                "day",
                None,
            )]
        );
    }

    #[test]
    fn inline_parameter_binding_ignores_comments_literals_and_repeated_uses() {
        let query = VariableGraphNode {
            id: "result".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql:
                        "-- $ignored\nSELECT $day + $day, '$ignored' AS literal /* $also_ignored */"
                            .to_owned(),
                },
                arguments: vec![binding("day", "input", VariableGraphScalarType::Integer)],
            },
        };

        assert!(compile_variable_graph(&graph(vec![
            input("input", VariableGraphScalarType::Integer, false),
            query,
        ]))
        .is_ok());
    }

    #[test]
    fn query_definition_decoding_is_strict_and_missing_saved_ids_are_diagnosed() {
        let inline = serde_json::json!({
            "kind": "inline_sql",
            "sql": "SELECT 1"
        });
        assert_eq!(
            serde_json::from_value::<VariableGraphQueryDefinition>(inline)
                .expect("valid inline query DTO"),
            VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT 1".to_owned()
            }
        );
        assert!(
            serde_json::from_value::<VariableGraphQueryDefinition>(serde_json::json!({
                "kind": "inline_sql"
            }))
            .is_err()
        );

        for unknown in [
            serde_json::json!({
                "kind": "inline_sql",
                "sql": "SELECT 1",
                "source": "legacy"
            }),
            serde_json::json!({
                "kind": "saved_sql",
                "entry_id": "00000000-0000-0000-0000-000000000001",
                "revision_id": "00000000-0000-0000-0000-000000000002",
                "latest": true
            }),
        ] {
            assert!(serde_json::from_value::<VariableGraphQueryDefinition>(unknown).is_err());
        }

        let missing_ids = serde_json::from_value::<VariableGraphQueryDefinition>(
            serde_json::json!({ "kind": "saved_sql" }),
        )
        .expect("draft Saved SQL payload is decoded for stable domain diagnostics");
        assert_eq!(
            compile_variable_graph(&graph(vec![query_definition(missing_ids)]))
                .expect_err("both exact identities are mandatory"),
            [VariableGraphDiagnostic::for_variable(
                VariableGraphDiagnosticCode::InvalidSavedSqlIdentity,
                "result"
            )]
        );

        let missing_query = serde_json::json!({
            "id": "result",
            "kind": {
                "kind": "query",
                "returns": { "kind": "scalar", "value_type": {
                    "scalar_type": "integer", "nullable": false
                }},
                "arguments": []
            }
        });
        assert!(serde_json::from_value::<VariableGraphNode>(missing_query).is_err());
    }

    #[test]
    fn compiles_explicit_scalar_bindings_to_stable_dependency_order() {
        let graph = graph(vec![
            query(
                "total",
                vec![binding(
                    "limit",
                    "minimum",
                    VariableGraphScalarType::Integer,
                )],
            ),
            input("minimum", VariableGraphScalarType::Integer, false),
            input("other", VariableGraphScalarType::String, false),
        ]);

        let compiled = compile_variable_graph(&graph).expect("graph is valid");
        assert_eq!(compiled.topological_order, ["minimum", "other", "total"]);
        assert_eq!(compiled.dependencies["total"], ["minimum".to_owned()]);
        assert!(compiled.dependencies["minimum"].is_empty());
    }

    #[test]
    fn independent_nodes_have_input_order_independent_topology() {
        let first = graph(vec![
            input("zeta", VariableGraphScalarType::String, false),
            input("alpha", VariableGraphScalarType::String, false),
        ]);
        let second = graph(vec![
            input("alpha", VariableGraphScalarType::String, false),
            input("zeta", VariableGraphScalarType::String, false),
        ]);

        assert_eq!(
            compile_variable_graph(&first)
                .expect("valid graph")
                .topological_order,
            ["alpha", "zeta"]
        );
        assert_eq!(
            compile_variable_graph(&first).expect("valid graph"),
            compile_variable_graph(&second).expect("valid graph")
        );
    }

    #[test]
    fn reports_duplicate_variable_ids_and_argument_names_stably() {
        let duplicate_id = graph(vec![
            input("duplicate", VariableGraphScalarType::Integer, false),
            input("duplicate", VariableGraphScalarType::Integer, false),
        ]);
        assert_eq!(
            compile_variable_graph(&duplicate_id).expect_err("duplicate ID"),
            [VariableGraphDiagnostic::for_variable(
                VariableGraphDiagnosticCode::DuplicateVariableId,
                "duplicate"
            )]
        );

        let duplicate_argument = graph(vec![
            input("value", VariableGraphScalarType::Integer, false),
            query(
                "result",
                vec![
                    binding("same", "value", VariableGraphScalarType::Integer),
                    binding("same", "value", VariableGraphScalarType::Integer),
                ],
            ),
        ]);
        assert_eq!(
            compile_variable_graph(&duplicate_argument).expect_err("duplicate argument"),
            [VariableGraphDiagnostic::for_argument(
                VariableGraphDiagnosticCode::DuplicateArgumentName,
                "result",
                "same",
                None,
            )]
        );
    }

    #[test]
    fn accepts_only_sql_identifier_argument_names() {
        let valid = graph(vec![
            input("value", VariableGraphScalarType::Integer, false),
            query(
                "result",
                vec![binding("_x9", "value", VariableGraphScalarType::Integer)],
            ),
        ]);
        assert!(compile_variable_graph(&valid).is_ok());

        for invalid_name in ["", "9value", "has-dash", "has space", "値"] {
            let invalid = graph(vec![
                input("value", VariableGraphScalarType::Integer, false),
                query(
                    "result",
                    vec![binding(
                        invalid_name,
                        "value",
                        VariableGraphScalarType::Integer,
                    )],
                ),
            ]);
            let diagnostics = compile_variable_graph(&invalid).expect_err("invalid argument name");
            assert_eq!(
                diagnostics[0].code,
                VariableGraphDiagnosticCode::InvalidArgumentName,
                "argument name {invalid_name:?}"
            );
        }
    }

    #[test]
    fn rejects_missing_references_and_table_to_scalar_bindings() {
        let missing = graph(vec![query(
            "result",
            vec![binding("value", "absent", VariableGraphScalarType::Integer)],
        )]);
        assert_eq!(
            compile_variable_graph(&missing).expect_err("missing reference"),
            [VariableGraphDiagnostic::for_argument(
                VariableGraphDiagnosticCode::MissingVariableReference,
                "result",
                "value",
                Some("absent"),
            )]
        );

        let table = VariableGraphNode {
            id: "rows".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Table {
                    columns: vec![VariableGraphColumn {
                        name: "id".to_owned(),
                        value_type: scalar(VariableGraphScalarType::Integer, false),
                    }],
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT id FROM \"G\"".to_owned(),
                },
                arguments: Vec::new(),
            },
        };
        let table_binding = graph(vec![
            table,
            query(
                "result",
                vec![binding("value", "rows", VariableGraphScalarType::Integer)],
            ),
        ]);
        assert_eq!(
            compile_variable_graph(&table_binding).expect_err("table is not scalar"),
            [VariableGraphDiagnostic::for_argument(
                VariableGraphDiagnosticCode::VariableTypeMismatch,
                "result",
                "value",
                Some("rows"),
            )]
        );
    }

    #[test]
    fn rejects_scalar_type_mismatch_and_nullable_to_required_binding() {
        let wrong_scalar = graph(vec![
            input("value", VariableGraphScalarType::String, false),
            query(
                "result",
                vec![binding(
                    "parameter",
                    "value",
                    VariableGraphScalarType::Integer,
                )],
            ),
        ]);
        assert_eq!(
            compile_variable_graph(&wrong_scalar).expect_err("scalar kind mismatch")[0].code,
            VariableGraphDiagnosticCode::VariableTypeMismatch
        );

        let nullable_source = graph(vec![
            input("value", VariableGraphScalarType::Integer, true),
            query(
                "result",
                vec![binding(
                    "parameter",
                    "value",
                    VariableGraphScalarType::Integer,
                )],
            ),
        ]);
        assert_eq!(
            compile_variable_graph(&nullable_source)
                .expect_err("nullable source cannot satisfy required argument")[0]
                .code,
            VariableGraphDiagnosticCode::VariableTypeMismatch
        );

        let nullable_target = graph(vec![
            input("value", VariableGraphScalarType::Integer, false),
            query(
                "result",
                vec![VariableGraphArgumentBinding {
                    name: "parameter".to_owned(),
                    value_type: scalar(VariableGraphScalarType::Integer, true),
                    variable: "value".to_owned(),
                }],
            ),
        ]);
        assert!(compile_variable_graph(&nullable_target).is_ok());
    }

    #[test]
    fn reports_a_deterministic_closed_cycle_path() {
        let cyclic = graph(vec![
            query(
                "c",
                vec![binding("from_a", "a", VariableGraphScalarType::Integer)],
            ),
            query(
                "b",
                vec![binding("from_c", "c", VariableGraphScalarType::Integer)],
            ),
            query(
                "a",
                vec![binding("from_b", "b", VariableGraphScalarType::Integer)],
            ),
        ]);

        let diagnostics = compile_variable_graph(&cyclic).expect_err("cycle");
        assert_eq!(
            diagnostics[0].code,
            VariableGraphDiagnosticCode::DependencyCycle
        );
        assert_eq!(diagnostics[0].cycle, ["a", "b", "c", "a"]);
        assert_eq!(
            VariableGraphDiagnosticCode::DependencyCycle.as_str(),
            "dependency_cycle"
        );
    }

    #[test]
    fn validates_typed_constants_input_defaults_and_result_descriptors() {
        let invalid_constant = VariableGraphNode {
            id: "count".to_owned(),
            kind: VariableGraphNodeKind::Constant {
                value_type: scalar(VariableGraphScalarType::Integer, false),
                value: VariableGraphLiteral::String("1".to_owned()),
            },
        };
        assert_eq!(
            compile_variable_graph(&graph(vec![invalid_constant]))
                .expect_err("literal type mismatch")[0]
                .code,
            VariableGraphDiagnosticCode::InvalidConstantValue
        );

        let invalid_default = VariableGraphNode {
            id: "day".to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: scalar(VariableGraphScalarType::Date, false),
                required: true,
                default: Some(VariableGraphLiteral::Date("2026-02-30".to_owned())),
            },
        };
        assert_eq!(
            compile_variable_graph(&graph(vec![invalid_default]))
                .expect_err("invalid date default")[0]
                .code,
            VariableGraphDiagnosticCode::InvalidInputDefault
        );

        for invalid_date in ["0000-01-01", "2026-1-01", "10000-01-01"] {
            let invalid_date = VariableGraphNode {
                id: "date".to_owned(),
                kind: VariableGraphNodeKind::Constant {
                    value_type: scalar(VariableGraphScalarType::Date, false),
                    value: VariableGraphLiteral::Date(invalid_date.to_owned()),
                },
            };
            assert_eq!(
                compile_variable_graph(&graph(vec![invalid_date])).expect_err("non-canonical date")
                    [0]
                .code,
                VariableGraphDiagnosticCode::InvalidConstantValue
            );
        }
        let canonical_leap_day = VariableGraphNode {
            id: "date".to_owned(),
            kind: VariableGraphNodeKind::Constant {
                value_type: scalar(VariableGraphScalarType::Date, false),
                value: VariableGraphLiteral::Date("2024-02-29".to_owned()),
            },
        };
        assert!(compile_variable_graph(&graph(vec![canonical_leap_day])).is_ok());

        let invalid_table = VariableGraphNode {
            id: "rows".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Table {
                    columns: vec![
                        VariableGraphColumn {
                            name: "id".to_owned(),
                            value_type: scalar(VariableGraphScalarType::Integer, false),
                        },
                        VariableGraphColumn {
                            name: "id".to_owned(),
                            value_type: scalar(VariableGraphScalarType::String, false),
                        },
                    ],
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT id FROM \"G\"".to_owned(),
                },
                arguments: Vec::new(),
            },
        };
        assert_eq!(
            compile_variable_graph(&graph(vec![invalid_table]))
                .expect_err("duplicate table column")[0]
                .code,
            VariableGraphDiagnosticCode::InvalidResultDescriptor
        );
    }

    #[test]
    fn scalar_results_distinguish_cardinality_null_and_valid_values() {
        let expected = VariableGraphResultType::Scalar {
            value_type: scalar(VariableGraphScalarType::Integer, false),
        };
        let columns = [VariableGraphColumn {
            name: "count".to_owned(),
            value_type: scalar(VariableGraphScalarType::Integer, false),
        }];

        let no_rows = validate_variable_graph_result(&expected, &columns, &[])
            .expect_err("scalar queries require one row");
        assert_eq!(
            no_rows[0].code,
            VariableGraphResultDiagnosticCode::ScalarNoRows
        );
        assert_eq!(no_rows[0].expected_count, Some(1));
        assert_eq!(no_rows[0].observed_count, Some(0));

        let null = validate_variable_graph_result(
            &expected,
            &columns,
            &[vec![VariableGraphLiteral::Null]],
        )
        .expect_err("a non-null scalar cannot return SQL NULL");
        assert_eq!(
            null[0].code,
            VariableGraphResultDiagnosticCode::NullNotAllowed
        );
        assert_eq!(null[0].row_index, Some(0));
        assert_eq!(null[0].column_index, Some(0));

        let multiple_rows = validate_variable_graph_result(
            &expected,
            &columns,
            &[
                vec![VariableGraphLiteral::Integer(1)],
                vec![VariableGraphLiteral::Integer(2)],
            ],
        )
        .expect_err("scalar queries cannot choose the first row");
        assert_eq!(
            multiple_rows[0].code,
            VariableGraphResultDiagnosticCode::ScalarMultipleRows
        );
        assert_eq!(multiple_rows[0].observed_count, Some(2));

        let valid = validate_variable_graph_result(
            &expected,
            &columns,
            &[vec![VariableGraphLiteral::Integer(7)]],
        )
        .expect("one typed row and column");
        assert_eq!(
            valid,
            VariableGraphValidatedResult::Scalar {
                value: VariableGraphLiteral::Integer(7),
            }
        );

        let drifted_schema = [VariableGraphColumn {
            name: "count".to_owned(),
            value_type: scalar(VariableGraphScalarType::String, false),
        }];
        let drift = validate_variable_graph_result(
            &expected,
            &drifted_schema,
            &[vec![VariableGraphLiteral::String("seven".to_owned())]],
        )
        .expect_err("observed scalar type drift");
        assert_eq!(
            drift[0].code,
            VariableGraphResultDiagnosticCode::ColumnTypeMismatch
        );
    }

    #[test]
    fn result_contract_reports_typed_schema_and_cardinality_drift_together() {
        let expected = VariableGraphResultType::Scalar {
            value_type: scalar(VariableGraphScalarType::Integer, false),
        };
        let observed = [VariableGraphColumn {
            name: "total".to_owned(),
            value_type: scalar(VariableGraphScalarType::Float, false),
        }];

        let diagnostics = validate_variable_graph_result(
            &expected,
            &observed,
            &[
                vec![VariableGraphLiteral::Float(2.5)],
                vec![VariableGraphLiteral::Float(3.5)],
            ],
        )
        .expect_err("type and scalar cardinality must both match the declaration");

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphResultDiagnosticCode::ColumnTypeMismatch
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphResultDiagnosticCode::ScalarMultipleRows
                && diagnostic.expected_count == Some(1)
                && diagnostic.observed_count == Some(2)
        }));
        assert_eq!(
            VariableGraphResultDiagnosticCode::ColumnTypeMismatch.as_str(),
            "column_type_mismatch"
        );
        assert_eq!(
            VariableGraphResultDiagnosticCode::ScalarMultipleRows.as_str(),
            "scalar_multiple_rows"
        );
    }

    #[test]
    fn scalar_result_requires_one_column_and_preserves_nullable_null() {
        let expected = VariableGraphResultType::Scalar {
            value_type: scalar(VariableGraphScalarType::Integer, true),
        };
        let columns = [
            VariableGraphColumn {
                name: "count".to_owned(),
                value_type: scalar(VariableGraphScalarType::Integer, true),
            },
            VariableGraphColumn {
                name: "extra".to_owned(),
                value_type: scalar(VariableGraphScalarType::String, true),
            },
        ];
        let extra_column = validate_variable_graph_result(
            &expected,
            &columns,
            &[vec![
                VariableGraphLiteral::Integer(1),
                VariableGraphLiteral::String("unexpected".to_owned()),
            ]],
        )
        .expect_err("scalar results require one column");
        assert!(extra_column.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphResultDiagnosticCode::ScalarColumnCountMismatch
                && diagnostic.expected_count == Some(1)
                && diagnostic.observed_count == Some(2)
        }));

        let nullable_column = [VariableGraphColumn {
            name: "count".to_owned(),
            value_type: scalar(VariableGraphScalarType::Integer, true),
        }];
        let null = validate_variable_graph_result(
            &expected,
            &nullable_column,
            &[vec![VariableGraphLiteral::Null]],
        )
        .expect("nullable scalar may return SQL NULL");
        assert_eq!(
            null,
            VariableGraphValidatedResult::Scalar {
                value: VariableGraphLiteral::Null,
            }
        );

        let non_nullable_observed = [VariableGraphColumn {
            name: "count".to_owned(),
            value_type: scalar(VariableGraphScalarType::Integer, false),
        }];
        let null_value_drift = validate_variable_graph_result(
            &expected,
            &non_nullable_observed,
            &[vec![VariableGraphLiteral::Null]],
        )
        .expect_err("observed non-null schema cannot contain NULL");
        assert_eq!(
            null_value_drift[0].code,
            VariableGraphResultDiagnosticCode::NullNotAllowed
        );
    }

    #[test]
    fn query_string_results_are_not_limited_by_input_string_constraints() {
        let expected_scalar = VariableGraphResultType::Scalar {
            value_type: scalar(VariableGraphScalarType::String, false),
        };
        let scalar_column = [VariableGraphColumn {
            name: "value".to_owned(),
            value_type: scalar(VariableGraphScalarType::String, false),
        }];
        for value in [
            "x".repeat(super::super::MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1),
            "line\0break".to_owned(),
        ] {
            let literal = VariableGraphLiteral::String(value);
            assert_eq!(
                validate_variable_graph_result(
                    &expected_scalar,
                    &scalar_column,
                    &[vec![literal.clone()]],
                )
                .expect("SQL scalar string results are not input buffers"),
                VariableGraphValidatedResult::Scalar { value: literal }
            );
        }

        let expected_table = VariableGraphResultType::Table {
            columns: vec![VariableGraphColumn {
                name: "value".to_owned(),
                value_type: scalar(VariableGraphScalarType::String, false),
            }],
        };
        let table_column = [VariableGraphColumn {
            name: "value".to_owned(),
            value_type: scalar(VariableGraphScalarType::String, false),
        }];
        for value in [
            "x".repeat(super::super::MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1),
            "line\0break".to_owned(),
        ] {
            let literal = VariableGraphLiteral::String(value);
            assert_eq!(
                validate_variable_graph_result(
                    &expected_table,
                    &table_column,
                    &[vec![literal.clone()]],
                )
                .expect("SQL table string cells are not input buffers"),
                VariableGraphValidatedResult::Table {
                    columns: table_column.to_vec(),
                    rows: vec![vec![literal]],
                }
            );
        }
    }

    #[test]
    fn table_results_allow_empty_rows_and_validate_schema_and_cells() {
        let expected = VariableGraphResultType::Table {
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
        };
        let columns = [
            VariableGraphColumn {
                name: "day".to_owned(),
                value_type: scalar(VariableGraphScalarType::Date, false),
            },
            VariableGraphColumn {
                name: "amount".to_owned(),
                value_type: scalar(VariableGraphScalarType::Decimal, true),
            },
        ];

        assert_eq!(
            validate_variable_graph_result(&expected, &columns, &[]).expect("a table may be empty"),
            VariableGraphValidatedResult::Table {
                columns: columns.to_vec(),
                rows: Vec::new(),
            }
        );

        let invalid = validate_variable_graph_result(
            &expected,
            &[
                VariableGraphColumn {
                    name: "other_day".to_owned(),
                    value_type: scalar(VariableGraphScalarType::Date, false),
                },
                VariableGraphColumn {
                    name: "amount".to_owned(),
                    value_type: scalar(VariableGraphScalarType::String, true),
                },
            ],
            &[
                vec![
                    VariableGraphLiteral::Date("2026-10-18".to_owned()),
                    VariableGraphLiteral::String("100.00".to_owned()),
                ],
                vec![VariableGraphLiteral::Date("2026-10-19".to_owned())],
            ],
        )
        .expect_err("schema drift and row width mismatch");
        assert!(invalid.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphResultDiagnosticCode::ColumnNameMismatch
                && diagnostic.column_index == Some(0)
        }));
        assert!(invalid.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphResultDiagnosticCode::ColumnTypeMismatch
                && diagnostic.column_index == Some(1)
        }));
        assert!(invalid.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphResultDiagnosticCode::RowWidthMismatch
                && diagnostic.row_index == Some(1)
        }));

        let invalid_cell = validate_variable_graph_result(
            &expected,
            &columns,
            &[vec![
                VariableGraphLiteral::Date("2026-10-18".to_owned()),
                VariableGraphLiteral::String("100.00".to_owned()),
            ]],
        )
        .expect_err("cell values must match their typed schema");
        assert_eq!(
            invalid_cell[0].code,
            VariableGraphResultDiagnosticCode::ValueTypeMismatch
        );
        assert_eq!(invalid_cell[0].row_index, Some(0));
        assert_eq!(invalid_cell[0].column_index, Some(1));
    }

    #[test]
    fn result_diagnostics_have_stable_codes_and_context() {
        let observed_column = VariableGraphColumn {
            name: "amount".to_owned(),
            value_type: scalar(VariableGraphScalarType::Decimal, true),
        };
        let diagnostic = VariableGraphResultDiagnostic::for_column(
            VariableGraphResultDiagnosticCode::ColumnTypeMismatch,
            2,
            Some("total"),
            Some(scalar(VariableGraphScalarType::Decimal, false)),
            &observed_column,
        );
        let encoded = serde_json::to_value(&diagnostic).expect("serializes diagnostics");
        assert_eq!(
            encoded,
            serde_json::json!({
                "code": "column_type_mismatch",
                "column_index": 2,
                "column_name": "amount",
                "expected_column_name": "total",
                "expected_value_type": {"scalar_type": "decimal", "nullable": false},
                "observed_value_type": {"scalar_type": "decimal", "nullable": true}
            })
        );
        assert_eq!(
            VariableGraphResultDiagnosticCode::ScalarMultipleRows.as_str(),
            "scalar_multiple_rows"
        );
    }

    #[test]
    fn rejects_string_literals_outside_the_portable_value_contract() {
        let invalid_strings = vec![
            "bad\0value".to_owned(),
            "x".repeat(super::super::MAX_COMPOSITION_VARIABLE_STRING_BYTES + 1),
        ];
        for value in invalid_strings {
            let constant = VariableGraphNode {
                id: "value".to_owned(),
                kind: VariableGraphNodeKind::Constant {
                    value_type: scalar(VariableGraphScalarType::String, false),
                    value: VariableGraphLiteral::String(value),
                },
            };
            assert_eq!(
                compile_variable_graph(&graph(vec![constant]))
                    .expect_err("invalid portable string constant")[0]
                    .code,
                VariableGraphDiagnosticCode::InvalidConstantValue
            );
        }
    }

    #[test]
    fn integer_constants_and_defaults_obey_the_portable_safe_range() {
        let maximum = super::super::MAX_COMPOSITION_VARIABLE_INTEGER;
        for value in [maximum, -maximum] {
            let constant = VariableGraphNode {
                id: "value".to_owned(),
                kind: VariableGraphNodeKind::Constant {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                    value: VariableGraphLiteral::Integer(value),
                },
            };
            assert!(compile_variable_graph(&graph(vec![constant])).is_ok());

            let input = VariableGraphNode {
                id: "value".to_owned(),
                kind: VariableGraphNodeKind::Input {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                    required: false,
                    default: Some(VariableGraphLiteral::Integer(value)),
                },
            };
            assert!(compile_variable_graph(&graph(vec![input])).is_ok());
        }

        for value in [maximum + 1, -maximum - 1] {
            let constant = VariableGraphNode {
                id: "value".to_owned(),
                kind: VariableGraphNodeKind::Constant {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                    value: VariableGraphLiteral::Integer(value),
                },
            };
            assert_eq!(
                compile_variable_graph(&graph(vec![constant]))
                    .expect_err("unsafe integer constant")[0]
                    .code,
                VariableGraphDiagnosticCode::InvalidConstantValue
            );

            let input = VariableGraphNode {
                id: "value".to_owned(),
                kind: VariableGraphNodeKind::Input {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                    required: false,
                    default: Some(VariableGraphLiteral::Integer(value)),
                },
            };
            assert_eq!(
                compile_variable_graph(&graph(vec![input])).expect_err("unsafe integer default")[0]
                    .code,
                VariableGraphDiagnosticCode::InvalidInputDefault
            );
        }
    }

    #[test]
    fn literal_deserialization_rejects_unknown_fields() {
        let known_fields = serde_json::json!({ "kind": "string", "value": "value" });
        assert_eq!(
            serde_json::from_value::<VariableGraphLiteral>(known_fields)
                .expect("known literal fields"),
            VariableGraphLiteral::String("value".to_owned())
        );

        let unknown_field = serde_json::json!({
            "kind": "string",
            "value": "value",
            "legacy": true
        });
        assert!(serde_json::from_value::<VariableGraphLiteral>(unknown_field).is_err());
    }
}
