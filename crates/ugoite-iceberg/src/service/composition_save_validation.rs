use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet};
use ugoite_core::error::{AppError, ErrorCode};
use ugoite_domain::composition::{
    CompositionValue, CompositionVariable, CompositionVariableReference, CompositionVariableType,
};

/// Reject inline SQL parameters that cannot be supplied by the Composition's
/// declared variable schema. The query planner reports parameter names even
/// when a name was absent from that schema, so this must run before publishing.
pub(super) fn validate_inline_parameters(
    parameter_names: &BTreeSet<String>,
    variables: &[CompositionVariable],
) -> Result<()> {
    let declared = variables
        .iter()
        .map(|variable| variable.id.as_str())
        .collect::<BTreeSet<_>>();
    if parameter_names
        .iter()
        .any(|parameter| !declared.contains(parameter.as_str()))
    {
        return Err(invalid_composition(
            "Composition SQL parameters must be declared as Composition variables",
        ));
    }
    Ok(())
}

/// Verify that a Saved SQL source binds every variable in its pinned revision,
/// and that each literal or Composition variable has the exact declared type.
pub(super) fn validate_saved_sql_bindings(
    source_bindings: &BTreeMap<String, CompositionValue>,
    saved_sql_variable_types: &BTreeMap<String, String>,
    variables: &[CompositionVariable],
) -> Result<()> {
    let bound_names = source_bindings.keys().cloned().collect::<BTreeSet<_>>();
    let saved_names = saved_sql_variable_types
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if bound_names != saved_names {
        return Err(invalid_composition(
            "Composition Saved SQL bindings must match the pinned revision variables",
        ));
    }

    let composition_variables = variables
        .iter()
        .map(|variable| (variable.id.as_str(), variable.variable_type))
        .collect::<BTreeMap<_, _>>();
    for (name, value) in source_bindings {
        let Some(sql_type) = saved_sql_variable_types
            .get(name)
            .and_then(|type_name| composition_variable_type_for_sql(type_name))
        else {
            return Err(invalid_composition(
                "Composition Saved SQL revision has an unsupported variable type",
            ));
        };
        let value_matches = match value {
            CompositionValue::Literal(value) => sql_type.accepts_value(value.as_json_value()),
            CompositionValue::Variable(CompositionVariableReference { variable }) => {
                composition_variables.get(variable.as_str()) == Some(&sql_type)
            }
        };
        if !value_matches {
            return Err(invalid_composition(
                "Composition Saved SQL binding does not match its declared variable type",
            ));
        }
    }
    Ok(())
}

fn composition_variable_type_for_sql(type_name: &str) -> Option<CompositionVariableType> {
    match type_name {
        "string" => Some(CompositionVariableType::String),
        "boolean" => Some(CompositionVariableType::Boolean),
        "integer" => Some(CompositionVariableType::Integer),
        "float" => Some(CompositionVariableType::Float),
        "date" => Some(CompositionVariableType::Date),
        "timestamp" => Some(CompositionVariableType::Timestamp),
        _ => None,
    }
}

fn invalid_composition(message: &'static str) -> anyhow::Error {
    AppError::invalid_input(ErrorCode::InvalidInput, message).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn variable(id: &str, variable_type: CompositionVariableType) -> CompositionVariable {
        CompositionVariable {
            id: id.to_owned(),
            variable_type,
            required: true,
            default: None,
            format: None,
        }
    }

    #[test]
    fn inline_parameters_must_be_declared() {
        let variables = [variable("minimum", CompositionVariableType::Integer)];
        assert!(
            validate_inline_parameters(&BTreeSet::from(["minimum".to_owned()]), &variables).is_ok()
        );
        assert!(
            validate_inline_parameters(&BTreeSet::from(["missing".to_owned()]), &variables)
                .is_err()
        );
    }

    #[test]
    fn saved_sql_bindings_must_match_exact_revision_names() {
        let source_bindings = BTreeMap::from([(
            "minimum".to_owned(),
            CompositionValue::Literal(serde_json::from_value(json!(2)).unwrap()),
        )]);
        assert!(validate_saved_sql_bindings(
            &source_bindings,
            &BTreeMap::from([("minimum".to_owned(), "integer".to_owned())]),
            &[]
        )
        .is_ok());
        assert!(validate_saved_sql_bindings(&source_bindings, &BTreeMap::new(), &[]).is_err());
        assert!(validate_saved_sql_bindings(
            &source_bindings,
            &BTreeMap::from([
                ("minimum".to_owned(), "integer".to_owned()),
                ("maximum".to_owned(), "integer".to_owned()),
            ]),
            &[]
        )
        .is_err());
    }

    #[test]
    fn saved_sql_bindings_must_match_exact_types() {
        let variables = [variable("text", CompositionVariableType::String)];
        let literal_binding = BTreeMap::from([(
            "minimum".to_owned(),
            CompositionValue::Literal(serde_json::from_value(json!("not an integer")).unwrap()),
        )]);
        let integer_sql = BTreeMap::from([("minimum".to_owned(), "integer".to_owned())]);
        assert!(validate_saved_sql_bindings(&literal_binding, &integer_sql, &variables).is_err());

        let variable_binding = BTreeMap::from([(
            "minimum".to_owned(),
            CompositionValue::Variable(CompositionVariableReference {
                variable: "text".to_owned(),
            }),
        )]);
        assert!(validate_saved_sql_bindings(&variable_binding, &integer_sql, &variables).is_err());
        assert!(validate_saved_sql_bindings(
            &variable_binding,
            &BTreeMap::from([("minimum".to_owned(), "string".to_owned())]),
            &variables
        )
        .is_ok());
    }
}
