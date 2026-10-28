//! Resolve typed constants and caller-supplied inputs for a variable graph.

use super::variable_graph::{
    compile_variable_graph, variable_graph_literal_matches, VariableGraph, VariableGraphDiagnostic,
    VariableGraphLiteral, VariableGraphNodeKind,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Result of parsing one editor buffer against a typed input descriptor.
#[derive(Debug, Clone, PartialEq)]
pub enum VariableGraphInputValue {
    Unset,
    Valid(VariableGraphLiteral),
    Invalid,
}

/// Parse an uncoerced text buffer using the graph's scalar contract.
/// Empty text means unset so callers can preserve their raw draft separately.
pub fn parse_variable_graph_input(
    descriptor: super::variable_graph::VariableGraphScalarDescriptor,
    raw: &str,
) -> VariableGraphInputValue {
    if raw.is_empty() {
        return VariableGraphInputValue::Unset;
    }
    use super::variable_graph::{VariableGraphLiteral as Literal, VariableGraphScalarType as Type};
    if matches!(
        descriptor.scalar_type,
        Type::Integer | Type::Float | Type::Decimal
    ) && raw.len() > super::MAX_COMPOSITION_VARIABLE_NUMERIC_TOKEN_BYTES
    {
        return VariableGraphInputValue::Invalid;
    }
    let literal = match descriptor.scalar_type {
        Type::String => Literal::String(raw.to_owned()),
        Type::Boolean => match raw {
            "true" => Literal::Boolean(true),
            "false" => Literal::Boolean(false),
            _ => return VariableGraphInputValue::Invalid,
        },
        Type::Integer => match raw.parse::<i64>() {
            Ok(value) => Literal::Integer(value),
            Err(_) => return VariableGraphInputValue::Invalid,
        },
        Type::Float => match raw.parse::<f64>() {
            Ok(value) if !float_underflowed_to_zero(raw, value) => Literal::Float(value),
            Ok(_) | Err(_) => return VariableGraphInputValue::Invalid,
        },
        Type::Decimal => Literal::Decimal(raw.to_owned()),
        Type::Date => Literal::Date(raw.to_owned()),
        Type::Timestamp => Literal::Timestamp(raw.to_owned()),
        Type::Json => match serde_json::from_str(raw) {
            Ok(value) => Literal::Json(value),
            Err(_) => return VariableGraphInputValue::Invalid,
        },
    };
    if super::variable_graph::variable_graph_literal_matches(&descriptor, &literal) {
        VariableGraphInputValue::Valid(literal)
    } else {
        VariableGraphInputValue::Invalid
    }
}

fn float_underflowed_to_zero(raw: &str, value: f64) -> bool {
    value == 0.0
        && raw.split(['e', 'E']).next().is_some_and(|significand| {
            significand
                .bytes()
                .any(|byte| (b'1'..=b'9').contains(&byte))
        })
}

/// Stable diagnostics for input values supplied to a variable graph.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableGraphInputDiagnosticCode {
    UnknownInput,
    ValueForNonInput,
    MissingRequiredInput,
    InputTypeMismatch,
}

impl VariableGraphInputDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownInput => "unknown_input",
            Self::ValueForNonInput => "value_for_non_input",
            Self::MissingRequiredInput => "missing_required_input",
            Self::InputTypeMismatch => "input_type_mismatch",
        }
    }
}

/// One deterministic diagnostic for a supplied or required input value.
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableGraphInputDiagnostic {
    pub code: VariableGraphInputDiagnosticCode,
    pub variable_id: String,
}

/// Keep graph-definition errors distinct from value-binding errors.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct VariableGraphInputError {
    pub graph: Vec<VariableGraphDiagnostic>,
    pub inputs: Vec<VariableGraphInputDiagnostic>,
}

/// Resolved constant and input values in graph identity order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedVariableGraphInputs {
    pub values: BTreeMap<String, VariableGraphLiteral>,
}

/// Resolve constants and explicit input values without coercion.
///
/// Supplied values may name only input nodes. A provided value overrides its
/// typed default; an invalid provided value never falls back to that default.
/// Optional unset inputs are omitted so downstream query bindings can report
/// them as missing only when they are actually needed.
pub fn resolve_variable_graph_inputs(
    graph: &VariableGraph,
    supplied: &BTreeMap<String, VariableGraphLiteral>,
) -> Result<ResolvedVariableGraphInputs, VariableGraphInputError> {
    if let Err(graph) = compile_variable_graph(graph) {
        return Err(VariableGraphInputError {
            graph,
            inputs: Vec::new(),
        });
    }

    let mut diagnostics = Vec::new();
    let mut values = BTreeMap::new();
    let mut input_ids = BTreeSet::new();

    for node in &graph.variables {
        match &node.kind {
            VariableGraphNodeKind::Constant { value, .. } => {
                values.insert(node.id.clone(), value.clone());
            }
            VariableGraphNodeKind::Input {
                value_type,
                required,
                default,
            } => {
                input_ids.insert(node.id.as_str());
                let value = supplied.get(&node.id).or(default.as_ref());
                match value {
                    Some(value) if variable_graph_literal_matches(value_type, value) => {
                        values.insert(node.id.clone(), value.clone());
                    }
                    Some(_) => diagnostics.push(VariableGraphInputDiagnostic {
                        code: VariableGraphInputDiagnosticCode::InputTypeMismatch,
                        variable_id: node.id.clone(),
                    }),
                    None if *required => diagnostics.push(VariableGraphInputDiagnostic {
                        code: VariableGraphInputDiagnosticCode::MissingRequiredInput,
                        variable_id: node.id.clone(),
                    }),
                    None => {}
                }
            }
            VariableGraphNodeKind::Query { .. } => {}
        }
    }

    for variable_id in supplied.keys() {
        if input_ids.contains(variable_id.as_str()) {
            continue;
        }
        let code = if graph.variables.iter().any(|node| node.id == *variable_id) {
            VariableGraphInputDiagnosticCode::ValueForNonInput
        } else {
            VariableGraphInputDiagnosticCode::UnknownInput
        };
        diagnostics.push(VariableGraphInputDiagnostic {
            code,
            variable_id: variable_id.clone(),
        });
    }

    if !diagnostics.is_empty() {
        diagnostics.sort();
        diagnostics.dedup();
        return Err(VariableGraphInputError {
            graph: Vec::new(),
            inputs: diagnostics,
        });
    }

    Ok(ResolvedVariableGraphInputs { values })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composition::variable_graph::{
        VariableGraphNode, VariableGraphQuery, VariableGraphQueryDefinition,
        VariableGraphResultType, VariableGraphScalarDescriptor, VariableGraphScalarType,
    };

    fn descriptor(
        scalar_type: VariableGraphScalarType,
        nullable: bool,
    ) -> VariableGraphScalarDescriptor {
        VariableGraphScalarDescriptor {
            scalar_type,
            nullable,
        }
    }

    #[test]
    fn float_input_rejects_nonzero_values_that_underflow_to_zero() {
        let value_type = descriptor(VariableGraphScalarType::Float, false);
        assert_eq!(
            parse_variable_graph_input(value_type, "1e-4000"),
            VariableGraphInputValue::Invalid
        );
        assert_eq!(
            parse_variable_graph_input(value_type, "0e-4000"),
            VariableGraphInputValue::Valid(VariableGraphLiteral::Float(0.0))
        );
    }

    #[test]
    fn numeric_inputs_enforce_the_shared_bounded_lexical_contract() {
        let float_type = descriptor(VariableGraphScalarType::Float, false);
        let maximum = "1".to_owned()
            + &"0".repeat(super::super::MAX_COMPOSITION_VARIABLE_NUMERIC_TOKEN_BYTES - 1);
        let too_long = maximum.clone() + "0";
        assert!(matches!(
            parse_variable_graph_input(float_type, &maximum),
            VariableGraphInputValue::Valid(VariableGraphLiteral::Float(_))
        ));
        assert_eq!(
            parse_variable_graph_input(float_type, &too_long),
            VariableGraphInputValue::Invalid
        );
        assert_eq!(
            parse_variable_graph_input(
                descriptor(VariableGraphScalarType::Decimal, false),
                &too_long
            ),
            VariableGraphInputValue::Invalid
        );
    }

    fn input(id: &str, required: bool, default: Option<VariableGraphLiteral>) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: descriptor(VariableGraphScalarType::Integer, false),
                required,
                default,
            },
        }
    }

    fn string_input(id: &str) -> VariableGraphNode {
        VariableGraphNode {
            id: id.to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: descriptor(VariableGraphScalarType::String, false),
                required: false,
                default: None,
            },
        }
    }

    fn graph(variables: Vec<VariableGraphNode>) -> VariableGraph {
        VariableGraph { variables }
    }

    fn diagnostic(
        code: VariableGraphInputDiagnosticCode,
        variable_id: &str,
    ) -> VariableGraphInputDiagnostic {
        VariableGraphInputDiagnostic {
            code,
            variable_id: variable_id.to_owned(),
        }
    }

    #[test]
    fn resolves_constants_defaults_and_supplied_values_without_coercion() {
        let graph = graph(vec![
            VariableGraphNode {
                id: "constant".to_owned(),
                kind: VariableGraphNodeKind::Constant {
                    value_type: descriptor(VariableGraphScalarType::String, false),
                    value: VariableGraphLiteral::String("fixed".to_owned()),
                },
            },
            input("with_default", true, Some(VariableGraphLiteral::Integer(1))),
            input("overridden", false, Some(VariableGraphLiteral::Integer(2))),
        ]);
        let supplied =
            BTreeMap::from([("overridden".to_owned(), VariableGraphLiteral::Integer(3))]);

        let resolved = resolve_variable_graph_inputs(&graph, &supplied).expect("valid inputs");
        assert_eq!(resolved.values.len(), 3);
        assert_eq!(
            resolved.values["constant"],
            VariableGraphLiteral::String("fixed".to_owned())
        );
        assert_eq!(
            resolved.values["with_default"],
            VariableGraphLiteral::Integer(1)
        );
        assert_eq!(
            resolved.values["overridden"],
            VariableGraphLiteral::Integer(3)
        );
    }

    #[test]
    fn optional_unset_inputs_are_omitted() {
        let resolved = resolve_variable_graph_inputs(
            &graph(vec![input("optional", false, None)]),
            &BTreeMap::new(),
        )
        .expect("optional inputs may be unset");
        assert!(resolved.values.is_empty());
    }

    #[test]
    fn required_input_without_default_must_be_supplied() {
        let error = resolve_variable_graph_inputs(
            &graph(vec![input("required", true, None)]),
            &BTreeMap::new(),
        )
        .expect_err("required value is absent");
        assert_eq!(
            error.inputs,
            [diagnostic(
                VariableGraphInputDiagnosticCode::MissingRequiredInput,
                "required"
            )]
        );
    }

    #[test]
    fn supplied_value_overrides_default_only_when_its_type_is_valid() {
        let error = resolve_variable_graph_inputs(
            &graph(vec![input(
                "value",
                false,
                Some(VariableGraphLiteral::Integer(1)),
            )]),
            &BTreeMap::from([(
                "value".to_owned(),
                VariableGraphLiteral::String("1".to_owned()),
            )]),
        )
        .expect_err("wrong type must not fall back to the default");
        assert_eq!(
            error.inputs,
            [diagnostic(
                VariableGraphInputDiagnosticCode::InputTypeMismatch,
                "value"
            )]
        );
    }

    #[test]
    fn null_requires_nullable_input() {
        let graph = graph(vec![VariableGraphNode {
            id: "nullable".to_owned(),
            kind: VariableGraphNodeKind::Input {
                value_type: descriptor(VariableGraphScalarType::Integer, true),
                required: true,
                default: None,
            },
        }]);
        let resolved = resolve_variable_graph_inputs(
            &graph,
            &BTreeMap::from([("nullable".to_owned(), VariableGraphLiteral::Null)]),
        )
        .expect("nullable input accepts null");
        assert_eq!(resolved.values["nullable"], VariableGraphLiteral::Null);
    }

    #[test]
    fn supplied_strings_obey_the_portable_value_contract() {
        let error = resolve_variable_graph_inputs(
            &graph(vec![string_input("text")]),
            &BTreeMap::from([(
                "text".to_owned(),
                VariableGraphLiteral::String("bad\0value".to_owned()),
            )]),
        )
        .expect_err("control characters are not portable input values");
        assert_eq!(
            error.inputs,
            [diagnostic(
                VariableGraphInputDiagnosticCode::InputTypeMismatch,
                "text"
            )]
        );
    }

    #[test]
    fn supplied_integers_accept_the_complete_i64_range() {
        for value in [i64::MIN, i64::MAX] {
            let resolved = resolve_variable_graph_inputs(
                &graph(vec![input("amount", false, None)]),
                &BTreeMap::from([("amount".to_owned(), VariableGraphLiteral::Integer(value))]),
            )
            .expect("signed 64-bit integer input");
            assert_eq!(
                resolved.values["amount"],
                VariableGraphLiteral::Integer(value)
            );
        }

        for raw in ["9223372036854775808", "-9223372036854775809", "1.0"] {
            assert_eq!(
                parse_variable_graph_input(
                    descriptor(VariableGraphScalarType::Integer, false),
                    raw,
                ),
                VariableGraphInputValue::Invalid,
            );
        }
    }

    #[test]
    fn query_nodes_are_not_caller_inputs() {
        let query = VariableGraphNode {
            id: "computed".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: descriptor(VariableGraphScalarType::Integer, false),
                },
                query: VariableGraphQuery {
                    definition: VariableGraphQueryDefinition::InlineSql {
                        sql: "SELECT 1".to_owned(),
                    },
                    arguments: Vec::new(),
                    form_bindings: Vec::new(),
                    schema_fingerprint: String::new(),
                },
            },
        };
        let error = resolve_variable_graph_inputs(
            &graph(vec![query]),
            &BTreeMap::from([("computed".to_owned(), VariableGraphLiteral::Integer(1))]),
        )
        .expect_err("query values are computed, not supplied");
        assert_eq!(
            error.inputs,
            [diagnostic(
                VariableGraphInputDiagnosticCode::ValueForNonInput,
                "computed"
            )]
        );
    }

    #[test]
    fn unknown_input_ids_are_reported_in_sorted_order() {
        let supplied = BTreeMap::from([
            ("zeta".to_owned(), VariableGraphLiteral::Integer(1)),
            ("alpha".to_owned(), VariableGraphLiteral::Integer(2)),
        ]);
        let error = resolve_variable_graph_inputs(&graph(Vec::new()), &supplied)
            .expect_err("unknown names fail closed");
        assert_eq!(
            error.inputs,
            [
                diagnostic(VariableGraphInputDiagnosticCode::UnknownInput, "alpha"),
                diagnostic(VariableGraphInputDiagnosticCode::UnknownInput, "zeta"),
            ]
        );
    }
}
