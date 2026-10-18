//! Typed variable graph definitions and deterministic dependency compilation.
//!
//! This module is a pure-domain foundation for a future Composition graph.
//! It deliberately has no adapter to the current Composition document, no SQL
//! parser or executor, and no persistence behavior.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        arguments: Vec<VariableGraphArgumentBinding>,
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
            VariableGraphNodeKind::Query { returns, arguments } => {
                if !result_descriptor_is_valid(returns) {
                    diagnostics.push(VariableGraphDiagnostic::for_variable(
                        VariableGraphDiagnosticCode::InvalidResultDescriptor,
                        &node.id,
                    ));
                }
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

fn literal_matches(
    descriptor: &VariableGraphScalarDescriptor,
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
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer, false),
                },
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
