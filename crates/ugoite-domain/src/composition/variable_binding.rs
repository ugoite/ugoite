//! Strict display-component bindings for the typed Composition variable graph.
//!
//! This module validates the reference boundary between a variable graph and
//! the display components that consume it. Display components persist only a
//! variable reference; Form and Saved SQL identity belongs to query nodes.

use super::variable_graph::{
    compile_variable_graph, VariableGraph, VariableGraphDiagnostic, VariableGraphNodeKind,
    VariableGraphResultType, VariableGraphScalarType,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A strict, variable-only reference from a display component.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableGraphComponentBinding {
    /// Renders rows from a table-valued query.
    Table { id: String, variable: String },
    /// Renders a displayable scalar value.
    Metric { id: String, variable: String },
    /// Supplies an input variable value.
    Input { id: String, variable: String },
    /// Renders a string variable or owns literal text when `variable` is absent.
    Text {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        variable: Option<String>,
    },
}

impl VariableGraphComponentBinding {
    fn id(&self) -> &str {
        match self {
            Self::Table { id, .. }
            | Self::Metric { id, .. }
            | Self::Input { id, .. }
            | Self::Text { id, .. } => id,
        }
    }

    fn variable(&self) -> Option<&str> {
        match self {
            Self::Table { variable, .. }
            | Self::Metric { variable, .. }
            | Self::Input { variable, .. } => Some(variable),
            Self::Text { variable, .. } => variable.as_deref(),
        }
    }
}

/// Stable diagnostics for the component-to-variable boundary.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableGraphBindingDiagnosticCode {
    InvalidComponentId,
    DuplicateComponentId,
    DuplicateLayoutComponent,
    MissingVariable,
    UnknownLayoutComponent,
    UnplacedComponent,
    VariableTypeMismatch,
    UnplacedInput,
}

impl VariableGraphBindingDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidComponentId => "invalid_component_id",
            Self::DuplicateComponentId => "duplicate_component_id",
            Self::DuplicateLayoutComponent => "duplicate_layout_component",
            Self::MissingVariable => "missing_variable",
            Self::UnknownLayoutComponent => "unknown_layout_component",
            Self::UnplacedComponent => "unplaced_component",
            Self::VariableTypeMismatch => "variable_type_mismatch",
            Self::UnplacedInput => "unplaced_input",
        }
    }
}

/// A deterministic component binding diagnostic.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphBindingDiagnostic {
    pub code: VariableGraphBindingDiagnosticCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variable_id: Option<String>,
}

/// Separate graph and component diagnostics so callers retain the underlying
/// graph compiler's stable diagnostic contract.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VariableGraphBindingValidationError {
    pub graph: Vec<VariableGraphDiagnostic>,
    pub components: Vec<VariableGraphBindingDiagnostic>,
}

/// A validated set of component bindings for one immutable graph definition.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CompiledVariableGraphBindings {
    /// Component ID to the single referenced variable ID.
    pub component_variables: BTreeMap<String, String>,
    /// Input variable ID to the sorted components that supply it.
    pub input_components: BTreeMap<String, Vec<String>>,
}

/// Validate display references against the graph's declared value types.
///
/// Every input variable must be represented by at least one Input component.
/// Table, Metric, and Input components always serialize a required `variable`
/// field. Text components may omit it when they own literal content.
pub fn compile_variable_graph_bindings(
    graph: &VariableGraph,
    components: &[VariableGraphComponentBinding],
    layout_component_ids: &[String],
) -> Result<CompiledVariableGraphBindings, VariableGraphBindingValidationError> {
    if let Err(graph) = compile_variable_graph(graph) {
        return Err(VariableGraphBindingValidationError {
            graph,
            components: Vec::new(),
        });
    }

    let nodes: BTreeMap<&str, &VariableGraphNodeKind> = graph
        .variables
        .iter()
        .map(|node| (node.id.as_str(), &node.kind))
        .collect();
    let mut diagnostics = Vec::new();
    let mut component_counts = BTreeMap::<&str, usize>::new();
    let mut component_variables = BTreeMap::new();
    let mut input_components = BTreeMap::<String, Vec<String>>::new();
    let mut known_component_ids = BTreeSet::new();

    for component in components {
        let component_id = component.id();
        if component_id.trim().is_empty() {
            diagnostics.push(binding_diagnostic(
                VariableGraphBindingDiagnosticCode::InvalidComponentId,
                Some(component_id),
                None,
            ));
        } else {
            *component_counts.entry(component_id).or_default() += 1;
            known_component_ids.insert(component_id);
        }

        let Some(variable_id) = component.variable() else {
            continue;
        };
        let Some(variable_kind) = nodes.get(variable_id).copied() else {
            diagnostics.push(binding_diagnostic(
                VariableGraphBindingDiagnosticCode::MissingVariable,
                Some(component_id),
                Some(variable_id),
            ));
            continue;
        };

        if !component_accepts_variable(component, variable_kind) {
            diagnostics.push(binding_diagnostic(
                VariableGraphBindingDiagnosticCode::VariableTypeMismatch,
                Some(component_id),
                Some(variable_id),
            ));
            continue;
        }

        component_variables.insert(component_id.to_owned(), variable_id.to_owned());
        if matches!(component, VariableGraphComponentBinding::Input { .. }) {
            input_components
                .entry(variable_id.to_owned())
                .or_default()
                .push(component_id.to_owned());
        }
    }

    diagnostics.extend(
        component_counts
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(id, _)| {
                binding_diagnostic(
                    VariableGraphBindingDiagnosticCode::DuplicateComponentId,
                    Some(id),
                    None,
                )
            }),
    );

    let mut layout_counts = BTreeMap::<&str, usize>::new();
    for component_id in layout_component_ids {
        if !known_component_ids.contains(component_id.as_str()) {
            diagnostics.push(binding_diagnostic(
                VariableGraphBindingDiagnosticCode::UnknownLayoutComponent,
                Some(component_id),
                None,
            ));
            continue;
        }
        *layout_counts.entry(component_id).or_default() += 1;
    }
    diagnostics.extend(
        layout_counts
            .iter()
            .filter(|(_, count)| **count > 1)
            .map(|(id, _)| {
                binding_diagnostic(
                    VariableGraphBindingDiagnosticCode::DuplicateLayoutComponent,
                    Some(id),
                    None,
                )
            }),
    );
    diagnostics.extend(
        known_component_ids
            .iter()
            .filter(|component_id| !layout_counts.contains_key(**component_id))
            .map(|component_id| {
                binding_diagnostic(
                    VariableGraphBindingDiagnosticCode::UnplacedComponent,
                    Some(*component_id),
                    None,
                )
            }),
    );

    for (variable_id, kind) in &nodes {
        if !matches!(kind, VariableGraphNodeKind::Input { .. }) {
            continue;
        }
        let placed_component = input_components
            .get(*variable_id)
            .and_then(|component_ids| {
                component_ids
                    .iter()
                    .find(|component_id| layout_counts.get(component_id.as_str()) == Some(&1))
            });
        if placed_component.is_none() {
            diagnostics.push(binding_diagnostic(
                VariableGraphBindingDiagnosticCode::UnplacedInput,
                input_components
                    .get(*variable_id)
                    .and_then(|component_ids| component_ids.first())
                    .map(String::as_str),
                Some(variable_id),
            ));
        }
    }

    if !diagnostics.is_empty() {
        diagnostics.sort();
        diagnostics.dedup();
        return Err(VariableGraphBindingValidationError {
            graph: Vec::new(),
            components: diagnostics,
        });
    }

    for component_ids in input_components.values_mut() {
        component_ids.sort();
    }
    Ok(CompiledVariableGraphBindings {
        component_variables,
        input_components,
    })
}

fn binding_diagnostic(
    code: VariableGraphBindingDiagnosticCode,
    component_id: Option<&str>,
    variable_id: Option<&str>,
) -> VariableGraphBindingDiagnostic {
    VariableGraphBindingDiagnostic {
        code,
        component_id: component_id.map(str::to_owned),
        variable_id: variable_id.map(str::to_owned),
    }
}

fn component_accepts_variable(
    component: &VariableGraphComponentBinding,
    variable: &VariableGraphNodeKind,
) -> bool {
    match component {
        VariableGraphComponentBinding::Table { .. } => matches!(
            variable,
            VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Table { .. },
                ..
            }
        ),
        VariableGraphComponentBinding::Metric { .. } => {
            matches!(
                variable,
                VariableGraphNodeKind::Constant { .. }
                    | VariableGraphNodeKind::Input { .. }
                    | VariableGraphNodeKind::Query {
                        returns: VariableGraphResultType::Scalar { .. },
                        ..
                    }
            ) && scalar_type_of(variable).is_some_and(|scalar_type| {
                matches!(
                    scalar_type.scalar_type,
                    VariableGraphScalarType::String
                        | VariableGraphScalarType::Boolean
                        | VariableGraphScalarType::Integer
                        | VariableGraphScalarType::Float
                        | VariableGraphScalarType::Decimal
                        | VariableGraphScalarType::Date
                        | VariableGraphScalarType::Timestamp
                )
            })
        }
        VariableGraphComponentBinding::Input { .. } => {
            matches!(variable, VariableGraphNodeKind::Input { .. })
        }
        VariableGraphComponentBinding::Text { .. } => scalar_type_of(variable)
            .is_some_and(|scalar_type| scalar_type.scalar_type == VariableGraphScalarType::String),
    }
}

fn scalar_type_of(
    variable: &VariableGraphNodeKind,
) -> Option<super::variable_graph::VariableGraphScalarDescriptor> {
    match variable {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::variable_graph::{
        VariableGraphArgumentBinding, VariableGraphColumn, VariableGraphLiteral, VariableGraphNode,
        VariableGraphQueryDefinition, VariableGraphScalarDescriptor,
    };
    use serde_json::json;

    fn scalar(scalar_type: VariableGraphScalarType) -> VariableGraphScalarDescriptor {
        VariableGraphScalarDescriptor {
            scalar_type,
            nullable: false,
        }
    }

    fn input(id: &str, scalar_type: VariableGraphScalarType) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: scalar(scalar_type),
                required: true,
                default: None,
            },
        }
    }

    fn constant(id: &str, scalar_type: VariableGraphScalarType) -> VariableGraphNode {
        let value = match scalar_type {
            VariableGraphScalarType::String => VariableGraphLiteral::String("ready".to_owned()),
            VariableGraphScalarType::Boolean => VariableGraphLiteral::Boolean(true),
            VariableGraphScalarType::Integer => VariableGraphLiteral::Integer(1),
            _ => panic!("test helper only supports String, Boolean, and Integer"),
        };
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Constant {
                value_type: scalar(scalar_type),
                value,
            },
        }
    }

    fn table_query(id: &str) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Table {
                    columns: vec![VariableGraphColumn {
                        name: "id".to_owned(),
                        value_type: scalar(VariableGraphScalarType::Integer),
                    }],
                },
                query: VariableGraphQueryDefinition::InlineSql {
                    sql: "SELECT 1 AS id".to_owned(),
                },
                arguments: Vec::<VariableGraphArgumentBinding>::new(),
            },
        }
    }

    fn graph(variables: Vec<VariableGraphNode>) -> VariableGraph {
        VariableGraph { variables }
    }

    #[test]
    fn compiles_variable_only_bindings_and_tracks_input_components() {
        let graph = graph(vec![
            input("day", VariableGraphScalarType::String),
            constant("title", VariableGraphScalarType::String),
            constant("count", VariableGraphScalarType::Integer),
            constant("enabled", VariableGraphScalarType::Boolean),
            table_query("rows"),
        ]);
        let bindings = vec![
            VariableGraphComponentBinding::Input {
                id: "day_input".to_owned(),
                variable: "day".to_owned(),
            },
            VariableGraphComponentBinding::Text {
                id: "title_text".to_owned(),
                variable: Some("title".to_owned()),
            },
            VariableGraphComponentBinding::Metric {
                id: "count_metric".to_owned(),
                variable: "count".to_owned(),
            },
            VariableGraphComponentBinding::Metric {
                id: "enabled_metric".to_owned(),
                variable: "enabled".to_owned(),
            },
            VariableGraphComponentBinding::Table {
                id: "rows_table".to_owned(),
                variable: "rows".to_owned(),
            },
            VariableGraphComponentBinding::Text {
                id: "literal_text".to_owned(),
                variable: None,
            },
        ];

        let compiled = compile_variable_graph_bindings(
            &graph,
            &bindings,
            &[
                "day_input".to_owned(),
                "title_text".to_owned(),
                "count_metric".to_owned(),
                "enabled_metric".to_owned(),
                "rows_table".to_owned(),
                "literal_text".to_owned(),
            ],
        )
        .unwrap();
        assert_eq!(compiled.component_variables["rows_table"], "rows");
        assert_eq!(compiled.component_variables["count_metric"], "count");
        assert_eq!(compiled.component_variables["enabled_metric"], "enabled");
        assert_eq!(compiled.input_components["day"], ["day_input"]);
        assert!(!compiled.component_variables.contains_key("literal_text"));
    }

    #[test]
    fn rejects_unplaced_inputs_and_incompatible_display_types() {
        let graph = graph(vec![
            input("unplaced", VariableGraphScalarType::String),
            constant("text", VariableGraphScalarType::String),
            constant("number", VariableGraphScalarType::Integer),
            table_query("rows"),
        ]);
        let bindings = vec![
            VariableGraphComponentBinding::Table {
                id: "wrong_table".to_owned(),
                variable: "number".to_owned(),
            },
            VariableGraphComponentBinding::Metric {
                id: "wrong_metric".to_owned(),
                variable: "rows".to_owned(),
            },
            VariableGraphComponentBinding::Text {
                id: "wrong_text".to_owned(),
                variable: Some("number".to_owned()),
            },
            VariableGraphComponentBinding::Input {
                id: "wrong_input".to_owned(),
                variable: "text".to_owned(),
            },
        ];

        let error = compile_variable_graph_bindings(&graph, &bindings, &[]).unwrap_err();
        assert!(error.graph.is_empty());
        assert_eq!(
            error
                .components
                .iter()
                .filter(|diagnostic| {
                    diagnostic.code == VariableGraphBindingDiagnosticCode::VariableTypeMismatch
                })
                .count(),
            4
        );
        assert_eq!(
            error.components.last().unwrap().code,
            VariableGraphBindingDiagnosticCode::UnplacedInput
        );
    }

    #[test]
    fn rejects_duplicate_component_ids_and_missing_variables() {
        let graph = graph(vec![constant("title", VariableGraphScalarType::String)]);
        let bindings = vec![
            VariableGraphComponentBinding::Text {
                id: "duplicate".to_owned(),
                variable: Some("title".to_owned()),
            },
            VariableGraphComponentBinding::Text {
                id: "duplicate".to_owned(),
                variable: Some("missing".to_owned()),
            },
        ];
        let error = compile_variable_graph_bindings(&graph, &bindings, &[]).unwrap_err();
        assert!(error.components.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphBindingDiagnosticCode::DuplicateComponentId
        }));
        assert!(error.components.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphBindingDiagnosticCode::MissingVariable
        }));
    }

    #[test]
    fn component_dto_has_no_source_or_form_compatibility_field() {
        let encoded = serde_json::to_value(VariableGraphComponentBinding::Table {
            id: "rows_table".to_owned(),
            variable: "rows".to_owned(),
        })
        .unwrap();
        assert_eq!(
            encoded,
            json!({"kind":"table","id":"rows_table","variable":"rows"})
        );

        for legacy in ["source", "form_id", "entry_id"] {
            let mut value = encoded.clone();
            value[legacy] = json!("legacy");
            assert!(serde_json::from_value::<VariableGraphComponentBinding>(value).is_err());
        }
    }

    #[test]
    fn input_variables_must_be_placed_and_layout_must_resolve_unique_components() {
        let graph = graph(vec![input("day", VariableGraphScalarType::String)]);
        let components = vec![VariableGraphComponentBinding::Input {
            id: "day_input".to_owned(),
            variable: "day".to_owned(),
        }];

        let unplaced = compile_variable_graph_bindings(&graph, &components, &[])
            .expect_err("input component is not present in layout");
        assert_eq!(
            unplaced.components,
            [
                binding_diagnostic(
                    VariableGraphBindingDiagnosticCode::UnplacedComponent,
                    Some("day_input"),
                    None,
                ),
                binding_diagnostic(
                    VariableGraphBindingDiagnosticCode::UnplacedInput,
                    Some("day_input"),
                    Some("day"),
                ),
            ]
        );

        let invalid_layout = compile_variable_graph_bindings(
            &graph,
            &components,
            &[
                "day_input".to_owned(),
                "day_input".to_owned(),
                "missing_component".to_owned(),
            ],
        )
        .expect_err("layout references must be unique and resolve");
        assert!(invalid_layout.components.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphBindingDiagnosticCode::DuplicateLayoutComponent
        }));
        assert!(invalid_layout.components.iter().any(|diagnostic| {
            diagnostic.code == VariableGraphBindingDiagnosticCode::UnknownLayoutComponent
        }));
    }

    #[test]
    fn every_component_requires_exactly_one_layout_placement() {
        let graph = graph(vec![constant("title", VariableGraphScalarType::String)]);
        let components = vec![VariableGraphComponentBinding::Text {
            id: "title_text".to_owned(),
            variable: Some("title".to_owned()),
        }];

        let error = compile_variable_graph_bindings(&graph, &components, &[])
            .expect_err("component without a layout item");
        assert_eq!(
            error.components,
            [binding_diagnostic(
                VariableGraphBindingDiagnosticCode::UnplacedComponent,
                Some("title_text"),
                None,
            )]
        );
    }
}
