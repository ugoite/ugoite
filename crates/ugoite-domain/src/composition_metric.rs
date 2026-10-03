//! Pure scalar Composition metric type and result validation.
//!
//! Source execution, authorization, and pagination stay in the existing query
//! paths. This module only classifies the selected descriptor and validates a
//! normalized page supplied by those paths.

use crate::composition::{CompositionDiagnosticCode, CompositionResultFieldType};
use crate::form::FieldType;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable diagnostic identifiers for metric result validation.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionMetricDiagnosticCode {
    MetricResultNotScalar,
    MetricResultTypeMismatch,
    MetricResultEmpty,
    MetricResultMultipleRows,
    MetricResultColumnMissing,
    MetricResultColumnAmbiguous,
    MetricResultPageIncomplete,
}

impl CompositionMetricDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MetricResultNotScalar => "metric_result_not_scalar",
            Self::MetricResultTypeMismatch => "metric_result_type_mismatch",
            Self::MetricResultEmpty => "metric_result_empty",
            Self::MetricResultMultipleRows => "metric_result_multiple_rows",
            Self::MetricResultColumnMissing => "metric_result_column_missing",
            Self::MetricResultColumnAmbiguous => "metric_result_column_ambiguous",
            Self::MetricResultPageIncomplete => "metric_result_page_incomplete",
        }
    }
}

impl From<CompositionMetricDiagnosticCode> for CompositionDiagnosticCode {
    fn from(code: CompositionMetricDiagnosticCode) -> Self {
        match code {
            CompositionMetricDiagnosticCode::MetricResultNotScalar => Self::MetricResultNotScalar,
            CompositionMetricDiagnosticCode::MetricResultTypeMismatch => {
                Self::MetricResultTypeMismatch
            }
            CompositionMetricDiagnosticCode::MetricResultEmpty => Self::MetricResultEmpty,
            CompositionMetricDiagnosticCode::MetricResultMultipleRows => {
                Self::MetricResultMultipleRows
            }
            CompositionMetricDiagnosticCode::MetricResultColumnMissing => {
                Self::MetricResultColumnMissing
            }
            CompositionMetricDiagnosticCode::MetricResultColumnAmbiguous => {
                Self::MetricResultColumnAmbiguous
            }
            CompositionMetricDiagnosticCode::MetricResultPageIncomplete => {
                Self::MetricResultPageIncomplete
            }
        }
    }
}

/// Map a current Form field type to its portable Composition metric type.
pub fn composition_metric_result_type(
    field_type: &FieldType,
) -> Result<CompositionResultFieldType, CompositionMetricDiagnosticCode> {
    let result_type = match field_type {
        FieldType::String
        | FieldType::Markdown
        | FieldType::Sql
        | FieldType::Time
        | FieldType::Uuid
        | FieldType::Binary => CompositionResultFieldType::String,
        FieldType::Boolean => CompositionResultFieldType::Boolean,
        FieldType::Integer | FieldType::Long => CompositionResultFieldType::Integer,
        FieldType::Float | FieldType::Double => CompositionResultFieldType::Float,
        FieldType::Date => CompositionResultFieldType::Date,
        FieldType::Timestamp
        | FieldType::TimestampTz
        | FieldType::TimestampNs
        | FieldType::TimestampTzNs => CompositionResultFieldType::Timestamp,
        FieldType::RowReference
        | FieldType::AssetReference
        | FieldType::List
        | FieldType::ObjectList => {
            return Err(CompositionMetricDiagnosticCode::MetricResultNotScalar);
        }
    };

    validate_composition_metric_result_type(result_type)?;
    Ok(result_type)
}

/// Validate a declared result type before a metric source is executed.
pub fn validate_composition_metric_result_type(
    result_type: CompositionResultFieldType,
) -> Result<(), CompositionMetricDiagnosticCode> {
    if result_type.supports_metric() {
        Ok(())
    } else {
        Err(CompositionMetricDiagnosticCode::MetricResultNotScalar)
    }
}

/// The metadata needed to evaluate a selected metric value from one query page.
///
/// `is_complete` must be false whenever the source has a continuation, even if
/// this page contains exactly one row. The source adapter is responsible for
/// counting result columns that match the selected name.
#[derive(Debug, Clone, Copy)]
pub struct CompositionMetricPage<'a> {
    pub is_complete: bool,
    pub row_count: usize,
    pub selected_column_count: usize,
    pub selected_value: Option<&'a Value>,
}

/// Return the selected scalar from a complete one-row page, without aggregation.
pub fn evaluate_composition_metric_page(
    expected_type: CompositionResultFieldType,
    page: CompositionMetricPage<'_>,
) -> Result<Value, CompositionMetricDiagnosticCode> {
    validate_composition_metric_result_type(expected_type)?;

    if !page.is_complete {
        return Err(CompositionMetricDiagnosticCode::MetricResultPageIncomplete);
    }
    if page.row_count == 0 {
        return Err(CompositionMetricDiagnosticCode::MetricResultEmpty);
    }
    if page.row_count > 1 {
        return Err(CompositionMetricDiagnosticCode::MetricResultMultipleRows);
    }
    if page.selected_column_count == 0 {
        return Err(CompositionMetricDiagnosticCode::MetricResultColumnMissing);
    }
    if page.selected_column_count > 1 {
        return Err(CompositionMetricDiagnosticCode::MetricResultColumnAmbiguous);
    }

    let Some(value) = page.selected_value else {
        return Err(CompositionMetricDiagnosticCode::MetricResultColumnMissing);
    };

    if matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(CompositionMetricDiagnosticCode::MetricResultNotScalar);
    }

    let matches_expected_type = match expected_type {
        CompositionResultFieldType::String
        | CompositionResultFieldType::Date
        | CompositionResultFieldType::Timestamp => value.is_string(),
        CompositionResultFieldType::Boolean => value.is_boolean(),
        CompositionResultFieldType::Integer => value.as_i64().is_some(),
        CompositionResultFieldType::Float => value.as_f64().is_some_and(f64::is_finite),
        CompositionResultFieldType::Json => false,
    };
    if !matches_expected_type {
        return Err(CompositionMetricDiagnosticCode::MetricResultTypeMismatch);
    }

    Ok(value.clone())
}

#[cfg(test)]
mod tests {
    use super::{
        composition_metric_result_type, evaluate_composition_metric_page,
        validate_composition_metric_result_type, CompositionMetricDiagnosticCode as Code,
        CompositionMetricPage,
    };
    use crate::composition::{CompositionDiagnosticCode, CompositionResultFieldType};
    use crate::form::FieldType;
    use serde_json::{json, Value};

    #[test]
    fn maps_form_scalar_types_to_portable_result_types() {
        let cases = [
            (FieldType::String, CompositionResultFieldType::String),
            (FieldType::Markdown, CompositionResultFieldType::String),
            (FieldType::Sql, CompositionResultFieldType::String),
            (FieldType::Time, CompositionResultFieldType::String),
            (FieldType::Uuid, CompositionResultFieldType::String),
            (FieldType::Binary, CompositionResultFieldType::String),
            (FieldType::Boolean, CompositionResultFieldType::Boolean),
            (FieldType::Integer, CompositionResultFieldType::Integer),
            (FieldType::Long, CompositionResultFieldType::Integer),
            (FieldType::Float, CompositionResultFieldType::Float),
            (FieldType::Double, CompositionResultFieldType::Float),
            (FieldType::Date, CompositionResultFieldType::Date),
            (FieldType::Timestamp, CompositionResultFieldType::Timestamp),
            (
                FieldType::TimestampTz,
                CompositionResultFieldType::Timestamp,
            ),
            (
                FieldType::TimestampNs,
                CompositionResultFieldType::Timestamp,
            ),
            (
                FieldType::TimestampTzNs,
                CompositionResultFieldType::Timestamp,
            ),
        ];

        for (field_type, expected) in cases {
            assert_eq!(composition_metric_result_type(&field_type), Ok(expected));
        }
    }

    #[test]
    fn rejects_reference_and_collection_form_types_as_non_scalar() {
        for field_type in [
            FieldType::RowReference,
            FieldType::AssetReference,
            FieldType::List,
            FieldType::ObjectList,
        ] {
            assert_eq!(
                composition_metric_result_type(&field_type),
                Err(Code::MetricResultNotScalar)
            );
        }
    }

    #[test]
    fn validates_declared_sql_result_type_before_query_execution() {
        for result_type in [
            CompositionResultFieldType::String,
            CompositionResultFieldType::Boolean,
            CompositionResultFieldType::Integer,
            CompositionResultFieldType::Float,
            CompositionResultFieldType::Date,
            CompositionResultFieldType::Timestamp,
        ] {
            assert_eq!(validate_composition_metric_result_type(result_type), Ok(()));
        }
        assert_eq!(
            validate_composition_metric_result_type(CompositionResultFieldType::Json),
            Err(Code::MetricResultNotScalar)
        );
    }

    #[test]
    fn evaluates_complete_single_row_scalar_values() {
        let cases = [
            (CompositionResultFieldType::String, json!("rent")),
            (CompositionResultFieldType::Boolean, json!(true)),
            (CompositionResultFieldType::Integer, json!(42)),
            (CompositionResultFieldType::Float, json!(42.5)),
            (CompositionResultFieldType::Date, json!("2026-10-03")),
            (
                CompositionResultFieldType::Timestamp,
                json!("2026-10-03T12:00:00Z"),
            ),
        ];

        for (expected_type, value) in cases {
            assert_eq!(evaluate_page(expected_type, &value), Ok(value));
        }
    }

    #[test]
    fn returns_stable_diagnostics_for_page_shape_failures() {
        let value = json!(1);
        let cases = [
            (
                CompositionResultFieldType::Integer,
                CompositionMetricPage {
                    is_complete: false,
                    row_count: 1,
                    selected_column_count: 1,
                    selected_value: Some(&value),
                },
                Code::MetricResultPageIncomplete,
            ),
            (
                CompositionResultFieldType::Integer,
                CompositionMetricPage {
                    is_complete: true,
                    row_count: 0,
                    selected_column_count: 1,
                    selected_value: None,
                },
                Code::MetricResultEmpty,
            ),
            (
                CompositionResultFieldType::Integer,
                CompositionMetricPage {
                    is_complete: true,
                    row_count: 2,
                    selected_column_count: 1,
                    selected_value: Some(&value),
                },
                Code::MetricResultMultipleRows,
            ),
            (
                CompositionResultFieldType::Integer,
                CompositionMetricPage {
                    is_complete: true,
                    row_count: 1,
                    selected_column_count: 0,
                    selected_value: None,
                },
                Code::MetricResultColumnMissing,
            ),
            (
                CompositionResultFieldType::Integer,
                CompositionMetricPage {
                    is_complete: true,
                    row_count: 1,
                    selected_column_count: 2,
                    selected_value: Some(&value),
                },
                Code::MetricResultColumnAmbiguous,
            ),
            (
                CompositionResultFieldType::Integer,
                CompositionMetricPage {
                    is_complete: true,
                    row_count: 1,
                    selected_column_count: 1,
                    selected_value: None,
                },
                Code::MetricResultColumnMissing,
            ),
        ];

        for (expected_type, page, expected_error) in cases {
            assert_eq!(
                evaluate_composition_metric_page(expected_type, page),
                Err(expected_error)
            );
        }
    }

    #[test]
    fn rejects_structured_and_type_mismatched_runtime_values() {
        for value in [json!([1, 2]), json!({ "value": 1 })] {
            assert_eq!(
                evaluate_page(CompositionResultFieldType::Integer, &value),
                Err(Code::MetricResultNotScalar)
            );
        }

        let mismatches = [
            (CompositionResultFieldType::String, json!(1)),
            (CompositionResultFieldType::Boolean, json!("true")),
            (CompositionResultFieldType::Integer, json!(1.5)),
            (CompositionResultFieldType::Float, json!("1.5")),
            (CompositionResultFieldType::Date, json!(1)),
            (CompositionResultFieldType::Timestamp, Value::Null),
        ];

        for (expected_type, value) in mismatches {
            assert_eq!(
                evaluate_page(expected_type, &value),
                Err(Code::MetricResultTypeMismatch)
            );
        }
    }

    #[test]
    fn rejects_json_descriptors_without_evaluating_a_page() {
        let value = json!({ "amount": 1 });
        let page = CompositionMetricPage {
            is_complete: true,
            row_count: 1,
            selected_column_count: 1,
            selected_value: Some(&value),
        };
        assert_eq!(
            evaluate_composition_metric_page(CompositionResultFieldType::Json, page),
            Err(Code::MetricResultNotScalar)
        );
    }

    #[test]
    fn exposes_identical_codes_through_shared_and_metric_diagnostics() {
        let pairs = [
            (
                Code::MetricResultNotScalar,
                CompositionDiagnosticCode::MetricResultNotScalar,
            ),
            (
                Code::MetricResultTypeMismatch,
                CompositionDiagnosticCode::MetricResultTypeMismatch,
            ),
            (
                Code::MetricResultEmpty,
                CompositionDiagnosticCode::MetricResultEmpty,
            ),
            (
                Code::MetricResultMultipleRows,
                CompositionDiagnosticCode::MetricResultMultipleRows,
            ),
            (
                Code::MetricResultColumnMissing,
                CompositionDiagnosticCode::MetricResultColumnMissing,
            ),
            (
                Code::MetricResultColumnAmbiguous,
                CompositionDiagnosticCode::MetricResultColumnAmbiguous,
            ),
            (
                Code::MetricResultPageIncomplete,
                CompositionDiagnosticCode::MetricResultPageIncomplete,
            ),
        ];

        for (metric_code, composition_code) in pairs {
            let shared_code: CompositionDiagnosticCode = metric_code.into();
            assert_eq!(shared_code, composition_code);
            assert_eq!(metric_code.as_str(), shared_code.as_str());
        }
    }

    #[test]
    fn metric_diagnostic_codes_keep_the_stable_wire_names() {
        let cases = [
            (Code::MetricResultNotScalar, "metric_result_not_scalar"),
            (
                Code::MetricResultTypeMismatch,
                "metric_result_type_mismatch",
            ),
            (Code::MetricResultEmpty, "metric_result_empty"),
            (
                Code::MetricResultMultipleRows,
                "metric_result_multiple_rows",
            ),
            (
                Code::MetricResultColumnMissing,
                "metric_result_column_missing",
            ),
            (
                Code::MetricResultColumnAmbiguous,
                "metric_result_column_ambiguous",
            ),
            (
                Code::MetricResultPageIncomplete,
                "metric_result_page_incomplete",
            ),
        ];

        for (code, expected) in cases {
            assert_eq!(code.as_str(), expected);
            assert_eq!(serde_json::to_value(code).unwrap(), json!(expected));
            let shared_code: CompositionDiagnosticCode = code.into();
            assert_eq!(shared_code.as_str(), expected);
        }
    }

    fn evaluate_page(
        expected_type: CompositionResultFieldType,
        selected_value: &Value,
    ) -> Result<Value, Code> {
        evaluate_composition_metric_page(
            expected_type,
            CompositionMetricPage {
                is_complete: true,
                row_count: 1,
                selected_column_count: 1,
                selected_value: Some(selected_value),
            },
        )
    }
}
