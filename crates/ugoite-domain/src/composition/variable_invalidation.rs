//! Deterministic query invalidation planning for a typed Composition graph.

use super::variable_graph::{
    compile_variable_graph, CompiledVariableGraph, VariableGraph, VariableGraphDiagnostic,
    VariableGraphNodeKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Stable diagnostics for invalid change sets.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableInvalidationDiagnosticCode {
    UnknownChangedVariable,
}

impl VariableInvalidationDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownChangedVariable => "unknown_changed_variable",
        }
    }
}

/// A deterministic invalidation diagnostic.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableInvalidationDiagnostic {
    pub code: VariableInvalidationDiagnosticCode,
    pub variable_id: String,
}

/// Graph validation errors and invalidation request errors are kept distinct.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VariableInvalidationError {
    pub graph: Vec<VariableGraphDiagnostic>,
    pub changes: Vec<VariableInvalidationDiagnostic>,
}

/// Query variables that need recomputation in their deterministic dependency
/// order after the provided values or query definitions have changed.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VariableInvalidationPlan {
    pub queries: Vec<String>,
}

/// Plan which query variables are affected by changed graph values.
///
/// A changed query variable is included itself and then invalidates its query
/// dependents. A changed input or constant invalidates only downstream query
/// nodes. Shared consumers still produce one query ID because invalidation is
/// keyed by the variable graph, not display-component references.
pub fn plan_variable_graph_invalidation(
    graph: &VariableGraph,
    changed_variable_ids: &[String],
) -> Result<VariableInvalidationPlan, VariableInvalidationError> {
    let compiled = match compile_variable_graph(graph) {
        Ok(compiled) => compiled,
        Err(graph) => {
            return Err(VariableInvalidationError {
                graph,
                changes: Vec::new(),
            });
        }
    };
    plan_compiled_variable_graph_invalidation(graph, &compiled, changed_variable_ids)
}

/// Plan invalidation with a graph compilation that the caller already owns.
fn plan_compiled_variable_graph_invalidation(
    graph: &VariableGraph,
    compiled: &CompiledVariableGraph,
    changed_variable_ids: &[String],
) -> Result<VariableInvalidationPlan, VariableInvalidationError> {
    let nodes: BTreeMap<&str, &VariableGraphNodeKind> = graph
        .variables
        .iter()
        .map(|node| (node.id.as_str(), &node.kind))
        .collect();
    let mut changed = BTreeSet::new();
    let mut diagnostics = Vec::new();

    for variable_id in changed_variable_ids {
        if nodes.contains_key(variable_id.as_str()) {
            changed.insert(variable_id.as_str());
        } else {
            diagnostics.push(VariableInvalidationDiagnostic {
                code: VariableInvalidationDiagnosticCode::UnknownChangedVariable,
                variable_id: variable_id.clone(),
            });
        }
    }
    if !diagnostics.is_empty() {
        diagnostics.sort();
        diagnostics.dedup();
        return Err(VariableInvalidationError {
            graph: Vec::new(),
            changes: diagnostics,
        });
    }

    let mut queries = Vec::new();
    for variable_id in &compiled.topological_order {
        let Some(VariableGraphNodeKind::Query { .. }) = nodes.get(variable_id.as_str()).copied()
        else {
            continue;
        };

        let is_changed = changed.contains(variable_id.as_str());
        let has_changed_dependency =
            compiled
                .dependencies
                .get(variable_id)
                .is_some_and(|dependencies| {
                    dependencies
                        .iter()
                        .any(|dependency| changed.contains(dependency.as_str()))
                });
        if is_changed || has_changed_dependency {
            changed.insert(variable_id);
            queries.push(variable_id.clone());
        }
    }

    Ok(VariableInvalidationPlan { queries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::variable_graph::{
        VariableGraphArgumentBinding, VariableGraphNode, VariableGraphQueryDefinition,
        VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
    };

    fn scalar(scalar_type: VariableGraphScalarType) -> VariableGraphScalarDescriptor {
        VariableGraphScalarDescriptor {
            scalar_type,
            nullable: false,
        }
    }

    fn input(id: &str) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: scalar(VariableGraphScalarType::Integer),
                required: true,
                default: None,
            },
        }
    }

    fn query(id: &str, dependencies: &[(&str, &str)]) -> VariableGraphNode {
        let sql = if dependencies.is_empty() {
            "SELECT 1".to_owned()
        } else {
            format!(
                "SELECT {}",
                dependencies
                    .iter()
                    .map(|(name, _)| format!("${name}"))
                    .collect::<Vec<_>>()
                    .join(" + ")
            )
        };
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: scalar(VariableGraphScalarType::Integer),
                },
                query: VariableGraphQueryDefinition::InlineSql { sql },
                arguments: dependencies
                    .iter()
                    .map(|(name, variable)| VariableGraphArgumentBinding {
                        name: (*name).to_owned(),
                        value_type: scalar(VariableGraphScalarType::Integer),
                        variable: (*variable).to_owned(),
                    })
                    .collect(),
            },
        }
    }

    fn graph() -> VariableGraph {
        VariableGraph {
            variables: vec![
                input("day"),
                input("minimum"),
                query("count", &[("day", "day")]),
                query("total", &[("day", "day"), ("minimum", "minimum")]),
                query("label", &[("count", "count")]),
                query("independent", &[("minimum", "minimum")]),
                query("constant_query", &[]),
            ],
        }
    }

    fn changed(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn invalidates_only_transitive_queries_in_topological_order() {
        let graph = graph();
        let plan =
            plan_variable_graph_invalidation(&graph, &changed(&["day"])).expect("known change set");
        assert_eq!(plan.queries, ["count", "label", "total"]);

        let plan = plan_variable_graph_invalidation(&graph, &changed(&["minimum"]))
            .expect("known change set");
        assert_eq!(plan.queries, ["independent", "total"]);

        let plan = plan_variable_graph_invalidation(&graph, &[]).expect("empty change set");
        assert!(plan.queries.is_empty());
    }

    #[test]
    fn changed_query_definition_invalidates_it_and_its_dependents() {
        let graph = graph();
        let plan = plan_variable_graph_invalidation(&graph, &changed(&["count"]))
            .expect("known query variable");
        assert_eq!(plan.queries, ["count", "label"]);
    }

    #[test]
    fn duplicate_changes_are_deduplicated_and_unknown_ids_fail_closed() {
        let graph = graph();
        let plan = plan_variable_graph_invalidation(&graph, &changed(&["day", "day"]))
            .expect("known change set");
        assert_eq!(plan.queries, ["count", "label", "total"]);

        let error = plan_variable_graph_invalidation(&graph, &changed(&["missing"]))
            .expect_err("unknown variable");
        assert!(error.graph.is_empty());
        assert_eq!(error.changes.len(), 1);
        assert_eq!(
            error.changes[0].code,
            VariableInvalidationDiagnosticCode::UnknownChangedVariable
        );
    }
}
