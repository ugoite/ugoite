use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::composition::{
    VariableGraphArgumentBinding, VariableGraphNode, VariableGraphNodeKind,
    VariableGraphScalarDescriptor, VariableGraphScalarType,
};

/// Verify that a Saved SQL query binds every argument in its exact revision,
/// and that each binding points to a scalar graph variable of the required
/// SQL type. The parameter declarations remain owned by Saved SQL; the
/// Composition stores only argument-to-variable references.
pub(super) fn validate_saved_sql_arguments(
    bindings: &[VariableGraphArgumentBinding],
    saved_sql_types: &BTreeMap<String, VariableGraphScalarDescriptor>,
    variables: &[VariableGraphNode],
) -> Result<()> {
    let bound_names = bindings
        .iter()
        .map(|binding| binding.name.as_str())
        .collect::<BTreeSet<_>>();
    let saved_names = saved_sql_types
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if bound_names != saved_names || bound_names.len() != bindings.len() {
        return Err(invalid_composition(
            "Composition Saved SQL arguments must match the exact revision",
        ));
    }

    let nodes = variables
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    for binding in bindings {
        let required = saved_sql_types
            .get(&binding.name)
            .ok_or_else(|| invalid_composition("Composition Saved SQL argument is unknown"))?;
        let node = nodes
            .get(binding.variable.as_str())
            .ok_or_else(|| invalid_composition("Composition SQL argument variable is missing"))?;
        let actual = match &node.kind {
            VariableGraphNodeKind::Constant { value_type, .. }
            | VariableGraphNodeKind::Input { value_type, .. } => *value_type,
            VariableGraphNodeKind::Query {
                returns: ugoite_domain::composition::VariableGraphResultType::Scalar { value_type },
                ..
            } => *value_type,
            VariableGraphNodeKind::Query {
                returns: ugoite_domain::composition::VariableGraphResultType::Table { .. },
                ..
            } => {
                return Err(invalid_composition(
                    "Composition SQL arguments must reference scalar values",
                ));
            }
        };
        if actual.scalar_type != required.scalar_type {
            return Err(invalid_composition(
                "Composition Saved SQL argument type does not match its variable",
            ));
        }
        if !supported_saved_argument_type(required.scalar_type) {
            return Err(invalid_composition(
                "Composition Saved SQL revision uses an unsupported argument type",
            ));
        }
    }
    Ok(())
}

fn supported_saved_argument_type(value: VariableGraphScalarType) -> bool {
    matches!(
        value,
        VariableGraphScalarType::String
            | VariableGraphScalarType::Boolean
            | VariableGraphScalarType::Integer
            | VariableGraphScalarType::Float
            | VariableGraphScalarType::Date
            | VariableGraphScalarType::Timestamp
    )
}

fn invalid_composition(message: &'static str) -> anyhow::Error {
    AppError::invalid_input(ErrorCode::InvalidInput, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ugoite_domain::composition::{
        VariableGraphArgumentBinding, VariableGraphColumn, VariableGraphQuery,
        VariableGraphQueryDefinition, VariableGraphResultType,
    };

    #[test]
    fn saved_sql_arguments_accept_scalar_query_nodes_but_reject_tables() {
        let binding = VariableGraphArgumentBinding {
            name: "minimum".to_owned(),
            variable: "source".to_owned(),
        };
        let saved_sql_types = BTreeMap::from([(
            "minimum".to_owned(),
            VariableGraphScalarDescriptor {
                scalar_type: VariableGraphScalarType::Integer,
                nullable: false,
            },
        )]);
        let query = VariableGraphQuery {
            definition: VariableGraphQueryDefinition::InlineSql {
                sql: "SELECT 7 AS value".to_owned(),
            },
            arguments: Vec::new(),
            form_bindings: Vec::new(),
            schema_fingerprint: String::new(),
        };
        let scalar = VariableGraphNode {
            id: "source".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Scalar {
                    value_type: saved_sql_types["minimum"],
                },
                query: query.clone(),
            },
        };
        assert!(validate_saved_sql_arguments(
            std::slice::from_ref(&binding),
            &saved_sql_types,
            &[scalar],
        )
        .is_ok());

        let table = VariableGraphNode {
            id: "source".to_owned(),
            kind: VariableGraphNodeKind::Query {
                returns: VariableGraphResultType::Table {
                    columns: vec![VariableGraphColumn {
                        name: "value".to_owned(),
                        value_type: saved_sql_types["minimum"],
                    }],
                },
                query,
            },
        };
        let error = validate_saved_sql_arguments(&[binding], &saved_sql_types, &[table])
            .expect_err("table query variables cannot be SQL scalar arguments");
        assert_eq!(
            error.downcast_ref::<AppError>().map(AppError::code),
            Some(ErrorCode::InvalidInput)
        );
    }
}
