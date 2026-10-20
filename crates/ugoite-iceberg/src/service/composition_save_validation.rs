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
            VariableGraphNodeKind::Query { .. } => {
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
