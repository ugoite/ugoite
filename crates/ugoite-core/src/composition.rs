//! Pure Composition parameter binding.
//!
//! This module resolves typed Composition values only. Source compilation and
//! query execution stay in their existing core contracts and are added by the
//! corresponding resolver layers.

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use ugoite_domain::composition::{
    CompositionDiagnosticCode, CompositionLiteral, CompositionParameter, CompositionParameterType,
    CompositionValue,
};

/// One stable diagnostic emitted while binding a parameter or value template.
///
/// Parameter identifiers are included for caller-value binding errors.
/// Unknown or unset references embedded in a source template are code-only,
/// so resolving them does not disclose a source's parameter names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompositionDiagnostic {
    pub code: CompositionDiagnosticCode,
    pub parameter_id: Option<String>,
}

impl CompositionDiagnostic {
    fn for_parameter(code: CompositionDiagnosticCode, parameter_id: &str) -> Self {
        Self {
            code,
            parameter_id: Some(parameter_id.to_owned()),
        }
    }

    fn without_parameter(code: CompositionDiagnosticCode) -> Self {
        Self {
            code,
            parameter_id: None,
        }
    }
}

/// Bound values and deterministic diagnostics for one Composition request.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParameterBindings {
    /// Values supplied by the caller or selected from typed defaults.
    pub values: BTreeMap<String, Value>,
    /// Declared parameters whose supplied value or default failed type validation.
    /// Keeping this state prevents a source reference from misreporting a rejected
    /// value as merely missing.
    pub invalid_values: BTreeSet<String>,
    /// Declared types, including optional parameters that are currently unset.
    pub parameter_types: BTreeMap<String, CompositionParameterType>,
    pub diagnostics: Vec<CompositionDiagnostic>,
}

/// A value after resolving a literal or named Composition parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundCompositionValue {
    pub value: Value,
    /// `None` means the value was a literal in the Composition document.
    pub parameter_type: Option<CompositionParameterType>,
}

/// Bind caller values and defaults against the document's parameter schema.
///
/// Required parameters without values produce `parameter_missing`. Optional
/// unset parameters remain absent until a source actually references them.
/// Unknown caller keys are reported in lexical order; declared parameters are
/// processed in document order, making the complete diagnostic list stable.
pub fn bind_parameters(
    parameters: &[CompositionParameter],
    supplied: &BTreeMap<String, Value>,
) -> ParameterBindings {
    let mut bindings = ParameterBindings::default();
    let mut declared = BTreeSet::new();

    for parameter in parameters {
        if !declared.insert(parameter.id.clone()) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            return bindings;
        }
        if parameter.default.as_ref().is_some_and(|default| {
            !value_matches_parameter(default.as_json_value(), parameter.parameter_type)
        }) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::InvalidComposition,
                ));
            return bindings;
        }
        bindings
            .parameter_types
            .insert(parameter.id.clone(), parameter.parameter_type);
    }

    for parameter in parameters {
        let value = supplied.get(&parameter.id).or_else(|| {
            parameter
                .default
                .as_ref()
                .map(CompositionLiteral::as_json_value)
        });
        let Some(value) = value else {
            if parameter.required {
                bindings
                    .diagnostics
                    .push(CompositionDiagnostic::for_parameter(
                        CompositionDiagnosticCode::ParameterMissing,
                        &parameter.id,
                    ));
            }
            continue;
        };

        if value_matches_parameter(value, parameter.parameter_type) {
            bindings.values.insert(parameter.id.clone(), value.clone());
        } else {
            bindings.invalid_values.insert(parameter.id.clone());
            bindings
                .diagnostics
                .push(CompositionDiagnostic::for_parameter(
                    CompositionDiagnosticCode::ParameterTypeMismatch,
                    &parameter.id,
                ));
        }
    }

    for name in supplied.keys() {
        if !declared.contains(name) {
            bindings
                .diagnostics
                .push(CompositionDiagnostic::for_parameter(
                    CompositionDiagnosticCode::ParameterUnknown,
                    name,
                ));
        }
    }

    bindings
}

/// Resolve a source value without coercion or expression evaluation.
pub fn resolve_value_template(
    template: &CompositionValue,
    bindings: &ParameterBindings,
) -> Result<BoundCompositionValue, CompositionDiagnostic> {
    match template {
        CompositionValue::Literal(value) => Ok(BoundCompositionValue {
            value: value.as_json_value().clone(),
            parameter_type: None,
        }),
        CompositionValue::Parameter(reference) => {
            let Some(parameter_type) = bindings.parameter_types.get(&reference.parameter).copied()
            else {
                return Err(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::ParameterUnknown,
                ));
            };
            if bindings.invalid_values.contains(&reference.parameter) {
                return Err(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::ParameterTypeMismatch,
                ));
            }
            let Some(value) = bindings.values.get(&reference.parameter) else {
                return Err(CompositionDiagnostic::without_parameter(
                    CompositionDiagnosticCode::ParameterMissing,
                ));
            };
            Ok(BoundCompositionValue {
                value: value.clone(),
                parameter_type: Some(parameter_type),
            })
        }
    }
}

fn value_matches_parameter(value: &Value, parameter_type: CompositionParameterType) -> bool {
    match parameter_type {
        CompositionParameterType::String => value.is_string(),
        CompositionParameterType::Boolean => value.is_boolean(),
        CompositionParameterType::Integer => value.as_i64().is_some(),
        CompositionParameterType::Float => value.as_f64().is_some_and(f64::is_finite),
        CompositionParameterType::Date => value
            .as_str()
            .is_some_and(|value| parse_date(value).is_some()),
        CompositionParameterType::Timestamp => value.as_str().is_some_and(|value| {
            parse_wall_timestamp(value).is_some() || parse_zoned_timestamp(value).is_some()
        }),
    }
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    (parsed.format("%Y-%m-%d").to_string() == value).then_some(parsed)
}

fn parse_wall_timestamp(value: &str) -> Option<NaiveDateTime> {
    let parsed = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    (parsed.format("%Y-%m-%dT%H:%M:%S%.f").to_string() == value).then_some(parsed)
}

fn parse_zoned_timestamp(value: &str) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

#[cfg(test)]
mod tests {
    use super::{bind_parameters, resolve_value_template};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;
    use ugoite_domain::composition::{
        CompositionDiagnosticCode, CompositionLiteral, CompositionParameter,
        CompositionParameterFormat, CompositionParameterReference, CompositionParameterType,
        CompositionValue,
    };

    fn parameter(
        id: &str,
        parameter_type: CompositionParameterType,
        required: bool,
        default: Option<CompositionLiteral>,
        format: Option<CompositionParameterFormat>,
    ) -> CompositionParameter {
        CompositionParameter {
            id: id.to_owned(),
            parameter_type,
            required,
            default,
            format,
        }
    }

    fn parameter_ref(id: &str) -> CompositionValue {
        CompositionValue::Parameter(CompositionParameterReference {
            parameter: id.to_owned(),
        })
    }

    fn literal(value: Value) -> CompositionLiteral {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn defaults_are_bound_without_applying_format_hints() {
        let parameters = [parameter(
            "month_start",
            CompositionParameterType::Date,
            true,
            Some(literal(json!("2026-10-01"))),
            Some(CompositionParameterFormat::YearMonth),
        )];

        let bindings = bind_parameters(&parameters, &BTreeMap::new());

        assert!(bindings.diagnostics.is_empty());
        assert_eq!(bindings.values["month_start"], json!("2026-10-01"));
        let bound = resolve_value_template(&parameter_ref("month_start"), &bindings).unwrap();
        assert_eq!(bound.value, json!("2026-10-01"));
        assert_eq!(bound.parameter_type, Some(CompositionParameterType::Date));
    }

    #[test]
    fn required_missing_and_unknown_names_have_stable_order() {
        let parameters = [
            parameter("start", CompositionParameterType::Date, true, None, None),
            parameter(
                "optional",
                CompositionParameterType::String,
                false,
                None,
                None,
            ),
        ];
        let supplied = BTreeMap::from([
            ("unknown_z".to_owned(), json!(1)),
            ("unknown_a".to_owned(), json!(2)),
        ]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterMissing,
                    parameter_id: Some("start".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterUnknown,
                    parameter_id: Some("unknown_a".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterUnknown,
                    parameter_id: Some("unknown_z".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn invalid_supplied_value_does_not_fall_back_to_a_default() {
        let parameters = [parameter(
            "limit",
            CompositionParameterType::Integer,
            true,
            Some(literal(json!(5))),
            None,
        )];
        let supplied = BTreeMap::from([("limit".to_owned(), json!("five"))]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![super::CompositionDiagnostic {
                code: CompositionDiagnosticCode::ParameterTypeMismatch,
                parameter_id: Some("limit".to_owned()),
            }]
        );
        assert!(!bindings.values.contains_key("limit"));
        assert!(bindings.invalid_values.contains("limit"));
        let reference_error =
            resolve_value_template(&parameter_ref("limit"), &bindings).unwrap_err();
        assert_eq!(
            reference_error.code,
            CompositionDiagnosticCode::ParameterTypeMismatch
        );
        assert_eq!(reference_error.parameter_id, None);
    }

    #[test]
    fn invalid_default_is_rejected_even_when_caller_supplies_a_valid_value() {
        let parameters = [parameter(
            "limit",
            CompositionParameterType::Integer,
            true,
            Some(literal(json!("not an integer"))),
            None,
        )];
        let supplied = BTreeMap::from([("limit".to_owned(), json!(5))]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![super::CompositionDiagnostic {
                code: CompositionDiagnosticCode::InvalidComposition,
                parameter_id: None,
            }]
        );
        assert!(bindings.values.is_empty());
        assert!(bindings.invalid_values.is_empty());
    }

    #[test]
    fn optional_unset_values_are_missing_only_when_referenced() {
        let parameters = [parameter(
            "search_text",
            CompositionParameterType::String,
            false,
            None,
            None,
        )];
        let bindings = bind_parameters(&parameters, &BTreeMap::new());

        assert!(bindings.diagnostics.is_empty());
        let error = resolve_value_template(&parameter_ref("search_text"), &bindings).unwrap_err();
        assert_eq!(error.code, CompositionDiagnosticCode::ParameterMissing);
        assert_eq!(error.parameter_id, None);
    }

    #[test]
    fn undeclared_reference_is_unknown_and_literal_is_passed_through() {
        let bindings = bind_parameters(&[], &BTreeMap::new());

        let error = resolve_value_template(&parameter_ref("typo"), &bindings).unwrap_err();
        assert_eq!(error.code, CompositionDiagnosticCode::ParameterUnknown);
        assert_eq!(error.parameter_id, None);

        let literal_template = CompositionValue::Literal(literal(json!("unchanged")));
        assert_eq!(
            resolve_value_template(&literal_template, &bindings)
                .unwrap()
                .value,
            json!("unchanged")
        );
    }

    #[test]
    fn invalid_date_timestamp_and_float_values_are_not_coerced() {
        let parameters = [
            parameter("date", CompositionParameterType::Date, false, None, None),
            parameter(
                "timestamp",
                CompositionParameterType::Timestamp,
                false,
                None,
                None,
            ),
            parameter("float", CompositionParameterType::Float, false, None, None),
        ];
        let supplied = BTreeMap::from([
            ("date".to_owned(), json!("2026-02-30")),
            ("timestamp".to_owned(), json!("next Tuesday")),
            ("float".to_owned(), json!("1.0")),
        ]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert_eq!(
            bindings.diagnostics,
            vec![
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterTypeMismatch,
                    parameter_id: Some("date".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterTypeMismatch,
                    parameter_id: Some("timestamp".to_owned()),
                },
                super::CompositionDiagnostic {
                    code: CompositionDiagnosticCode::ParameterTypeMismatch,
                    parameter_id: Some("float".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn signed_integer_range_and_timestamp_forms_are_typed_without_coercion() {
        let parameters = [
            parameter(
                "signed",
                CompositionParameterType::Integer,
                true,
                None,
                None,
            ),
            parameter(
                "wall_time",
                CompositionParameterType::Timestamp,
                true,
                None,
                None,
            ),
            parameter(
                "zoned_time",
                CompositionParameterType::Timestamp,
                true,
                None,
                None,
            ),
            parameter("ratio", CompositionParameterType::Float, true, None, None),
            parameter(
                "whole_number_ratio",
                CompositionParameterType::Float,
                true,
                None,
                None,
            ),
        ];
        let supplied = BTreeMap::from([
            ("signed".to_owned(), json!(i64::MAX)),
            ("wall_time".to_owned(), json!("2026-10-02T12:30:00.125")),
            ("zoned_time".to_owned(), json!("2026-10-02T12:30:00+09:00")),
            ("ratio".to_owned(), json!(1.25)),
            ("whole_number_ratio".to_owned(), json!(1)),
        ]);

        let bindings = bind_parameters(&parameters, &supplied);

        assert!(bindings.diagnostics.is_empty());
        assert_eq!(bindings.values, supplied);
    }

    #[test]
    fn duplicate_declarations_are_invalid_composition() {
        let parameters = [
            parameter("same", CompositionParameterType::String, true, None, None),
            parameter("same", CompositionParameterType::String, true, None, None),
        ];

        let bindings = bind_parameters(&parameters, &BTreeMap::new());

        assert_eq!(bindings.diagnostics.len(), 1);
        assert_eq!(
            bindings.diagnostics[0].code,
            CompositionDiagnosticCode::InvalidComposition
        );
        assert_eq!(bindings.diagnostics[0].parameter_id, None);
    }
}
