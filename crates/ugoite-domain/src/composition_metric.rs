//! Pure scalar Composition metric type and result validation.
//!
//! Source execution and authorization stay in the query paths. This module
//! validates normalized scalar values selected from Saved SQL results.

use crate::composition::{CompositionDiagnosticCode, CompositionResultFieldType};
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable diagnostic identifiers for metric result validation.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionMetricDiagnosticCode {
    MetricResultNotScalar,
    MetricResultTypeMismatch,
    MetricScalarNoRows,
    MetricScalarMultipleRows,
    MetricResultColumnMissing,
    MetricResultColumnAmbiguous,
}

impl CompositionMetricDiagnosticCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MetricResultNotScalar => "metric_result_not_scalar",
            Self::MetricResultTypeMismatch => "metric_result_type_mismatch",
            Self::MetricScalarNoRows => "metric_scalar_no_rows",
            Self::MetricScalarMultipleRows => "metric_scalar_multiple_rows",
            Self::MetricResultColumnMissing => "metric_result_column_missing",
            Self::MetricResultColumnAmbiguous => "metric_result_column_ambiguous",
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
            CompositionMetricDiagnosticCode::MetricScalarNoRows => Self::MetricScalarNoRows,
            CompositionMetricDiagnosticCode::MetricScalarMultipleRows => {
                Self::MetricScalarMultipleRows
            }
            CompositionMetricDiagnosticCode::MetricResultColumnMissing => {
                Self::MetricResultColumnMissing
            }
            CompositionMetricDiagnosticCode::MetricResultColumnAmbiguous => {
                Self::MetricResultColumnAmbiguous
            }
        }
    }
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

/// Validate one nullable scalar returned by a typed SQL Scalar metric.
///
/// The query layer must first establish row and column cardinality. SQL NULL
/// is valid for every scalar result type; non-null values must retain their
/// exact portable representation.
pub fn validate_composition_metric_scalar_value(
    expected_type: CompositionResultFieldType,
    value: &Value,
) -> Result<(), CompositionMetricDiagnosticCode> {
    validate_composition_metric_result_type(expected_type)?;
    if value.is_null() {
        return Ok(());
    }
    if matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(CompositionMetricDiagnosticCode::MetricResultNotScalar);
    }

    let matches_expected_type = match expected_type {
        CompositionResultFieldType::String => value.is_string(),
        CompositionResultFieldType::Boolean => value.is_boolean(),
        CompositionResultFieldType::Integer => value
            .as_str()
            .is_some_and(is_normalized_composition_metric_integer),
        CompositionResultFieldType::Float => value.as_f64().is_some_and(f64::is_finite),
        CompositionResultFieldType::Decimal => value
            .as_str()
            .is_some_and(is_normalized_composition_metric_decimal),
        CompositionResultFieldType::Date => value
            .as_str()
            .is_some_and(is_normalized_composition_metric_date),
        CompositionResultFieldType::Timestamp => value
            .as_str()
            .is_some_and(is_normalized_composition_metric_timestamp),
        CompositionResultFieldType::Json => false,
    };
    if matches_expected_type {
        Ok(())
    } else {
        Err(CompositionMetricDiagnosticCode::MetricResultTypeMismatch)
    }
}

fn is_normalized_composition_metric_integer(value: &str) -> bool {
    let unsigned = value.strip_prefix('-').unwrap_or(value);
    !unsigned.is_empty()
        && unsigned.bytes().all(|byte| byte.is_ascii_digit())
        && (unsigned == "0" || !unsigned.starts_with('0'))
        && !(value.starts_with('-') && unsigned == "0")
}

/// Accept the normalized portable date representation used by metric results.
/// A date is a four-digit-year Gregorian calendar date in `YYYY-MM-DD` form.
fn is_normalized_composition_metric_date(value: &str) -> bool {
    let Some(parsed) = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok() else {
        return false;
    };
    parsed.format("%Y-%m-%d").to_string() == value
}

/// Accept exact decimal values as plain JSON strings, preserving fractional
/// scale without passing through a binary floating-point representation.
fn is_normalized_composition_metric_decimal(value: &str) -> bool {
    let (negative, unsigned) = match value.strip_prefix('-') {
        Some(unsigned) => (true, unsigned),
        None => (false, value),
    };
    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || (integer.len() > 1 && integer.starts_with('0'))
    {
        return false;
    }
    if let Some(fraction) = fraction {
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
    }
    let is_zero =
        integer == "0" && fraction.is_none_or(|fraction| fraction.bytes().all(|b| b == b'0'));
    !(negative && is_zero)
}

/// Accept normalized wall-clock timestamps and RFC 3339 timestamps with an
/// explicit UTC designator or numeric offset. Wall-clock values retain the
/// existing minute or seconds/fractional-seconds forms used by Composition
/// parameter binding; round-tripping rejects non-normalized spellings.
fn is_normalized_composition_metric_timestamp(value: &str) -> bool {
    if !value.is_ascii() || value.as_bytes().get(10) != Some(&b'T') {
        return false;
    }

    let wall_clock = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M"));
    if let Ok(parsed) = wall_clock {
        let seconds = parsed.format("%Y-%m-%dT%H:%M:%S%.f").to_string();
        let minutes = parsed.format("%Y-%m-%dT%H:%M").to_string();
        return seconds == value || minutes == value;
    }

    has_supported_fractional_precision(value) && DateTime::parse_from_rfc3339(value).is_ok()
}

/// Chrono stores subsecond values at nanosecond precision and otherwise
/// truncates longer RFC 3339 fractions during parsing. Reject those spellings
/// instead of silently changing the selected metric value.
fn has_supported_fractional_precision(value: &str) -> bool {
    let Some(dot) = value.find('.') else {
        return true;
    };
    let digits = value[dot + 1..]
        .chars()
        .take_while(char::is_ascii_digit)
        .count();
    (1..=9).contains(&digits)
}

#[cfg(test)]
mod tests {
    use super::{
        validate_composition_metric_result_type, validate_composition_metric_scalar_value,
        CompositionMetricDiagnosticCode as Code,
    };
    use crate::composition::{CompositionDiagnosticCode, CompositionResultFieldType};
    use serde_json::{json, Value};

    #[test]
    fn validates_declared_sql_result_type_before_query_execution() {
        for result_type in [
            CompositionResultFieldType::String,
            CompositionResultFieldType::Boolean,
            CompositionResultFieldType::Integer,
            CompositionResultFieldType::Float,
            CompositionResultFieldType::Decimal,
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
    fn validates_nullable_scalar_results_and_exact_numeric_representations() {
        for result_type in [
            CompositionResultFieldType::String,
            CompositionResultFieldType::Boolean,
            CompositionResultFieldType::Integer,
            CompositionResultFieldType::Float,
            CompositionResultFieldType::Decimal,
            CompositionResultFieldType::Date,
            CompositionResultFieldType::Timestamp,
        ] {
            assert_eq!(
                validate_composition_metric_scalar_value(result_type, &json!(null)),
                Ok(())
            );
        }

        for value in ["0", "42", "-42", "9007199254740993"] {
            assert_eq!(
                validate_composition_metric_scalar_value(
                    CompositionResultFieldType::Integer,
                    &json!(value)
                ),
                Ok(())
            );
        }
        for value in ["", "01", "-0", "1.0", "1e3"] {
            assert_eq!(
                validate_composition_metric_scalar_value(
                    CompositionResultFieldType::Integer,
                    &json!(value)
                ),
                Err(Code::MetricResultTypeMismatch)
            );
        }
        for value in ["0", "0.00", "12.3400", "-12.34"] {
            assert_eq!(
                validate_composition_metric_scalar_value(
                    CompositionResultFieldType::Decimal,
                    &json!(value)
                ),
                Ok(())
            );
        }
        assert_eq!(
            validate_composition_metric_scalar_value(
                CompositionResultFieldType::Decimal,
                &json!(12.34)
            ),
            Err(Code::MetricResultTypeMismatch)
        );
    }

    #[test]
    fn validates_typed_scalar_values() {
        let cases = [
            (CompositionResultFieldType::String, json!("rent")),
            (CompositionResultFieldType::Boolean, json!(true)),
            (CompositionResultFieldType::Integer, json!("42")),
            (CompositionResultFieldType::Float, json!(42.5)),
            (CompositionResultFieldType::Decimal, json!("12.3400")),
            (CompositionResultFieldType::Date, json!("2026-10-03")),
            (
                CompositionResultFieldType::Timestamp,
                json!("2026-10-03T12:00:00Z"),
            ),
        ];

        for (expected_type, value) in cases {
            assert_eq!(validate_value(expected_type, &value), Ok(()));
        }
    }

    #[test]
    fn validates_exact_decimal_metric_strings_without_losing_scale() {
        for value in ["0", "0.00", "12.3400", "-12.34", "999999999999999999999999"] {
            let value = json!(value);
            assert_eq!(
                validate_value(CompositionResultFieldType::Decimal, &value),
                Ok(())
            );
        }

        for value in [
            "", "-", "+1.2", "01.2", "-01", ".5", "1.", "1e3", "1.2.3", "-0", "-0.0000",
        ] {
            assert_eq!(
                validate_value(CompositionResultFieldType::Decimal, &json!(value)),
                Err(Code::MetricResultTypeMismatch),
                "accepted noncanonical decimal {value:?}"
            );
        }
        assert_eq!(
            validate_value(CompositionResultFieldType::Decimal, &json!(12.34)),
            Err(Code::MetricResultTypeMismatch)
        );
    }

    #[test]
    fn rejects_structured_and_type_mismatched_runtime_values() {
        for value in [json!([1, 2]), json!({ "value": 1 })] {
            assert_eq!(
                validate_value(CompositionResultFieldType::Integer, &value),
                Err(Code::MetricResultNotScalar)
            );
        }

        let mismatches = [
            (CompositionResultFieldType::String, json!(1)),
            (CompositionResultFieldType::Boolean, json!("true")),
            (CompositionResultFieldType::Integer, json!(1.5)),
            (CompositionResultFieldType::Float, json!("1.5")),
            (CompositionResultFieldType::Date, json!(1)),
        ];

        for (expected_type, value) in mismatches {
            assert_eq!(
                validate_value(expected_type, &value),
                Err(Code::MetricResultTypeMismatch)
            );
        }
    }

    #[test]
    fn accepts_normalized_composition_metric_dates_and_timestamps() {
        let dates = ["2026-10-03", "2024-02-29"];
        for value in dates {
            assert_eq!(
                validate_value(CompositionResultFieldType::Date, &json!(value)),
                Ok(())
            );
        }

        let timestamps = [
            "2026-10-03T12:00",
            "2026-10-03T12:00:00",
            "2026-10-03T12:00:00.123456789",
            "2026-10-03T12:00:00Z",
            "2026-10-03T12:00:00.123456789+09:00",
        ];
        for value in timestamps {
            assert_eq!(
                validate_value(CompositionResultFieldType::Timestamp, &json!(value)),
                Ok(())
            );
        }
    }

    #[test]
    fn rejects_malformed_or_non_normalized_composition_metric_dates_and_timestamps() {
        for value in ["not-a-date", "2026-02-29", "2026-13-03", "2026-1-03"] {
            assert_eq!(
                validate_value(CompositionResultFieldType::Date, &json!(value)),
                Err(Code::MetricResultTypeMismatch),
                "date value {value:?} should be rejected"
            );
        }

        for value in [
            "not-a-timestamp",
            "2026-02-30T12:00",
            "2026-10-03T24:00",
            "2026-10-03T12:00:00.1200",
            "2026-10-03t12:00:00Z",
            "2026-10-03T12:00:00+24:00",
            "2026-10-03T12:00:00.1234567890Z",
        ] {
            assert_eq!(
                validate_value(CompositionResultFieldType::Timestamp, &json!(value)),
                Err(Code::MetricResultTypeMismatch),
                "timestamp value {value:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_json_values_for_scalar_metrics() {
        let value = json!({ "amount": 1 });
        assert_eq!(
            validate_value(CompositionResultFieldType::Json, &value),
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
                Code::MetricScalarNoRows,
                CompositionDiagnosticCode::MetricScalarNoRows,
            ),
            (
                Code::MetricScalarMultipleRows,
                CompositionDiagnosticCode::MetricScalarMultipleRows,
            ),
            (
                Code::MetricResultColumnMissing,
                CompositionDiagnosticCode::MetricResultColumnMissing,
            ),
            (
                Code::MetricResultColumnAmbiguous,
                CompositionDiagnosticCode::MetricResultColumnAmbiguous,
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
            (Code::MetricScalarNoRows, "metric_scalar_no_rows"),
            (
                Code::MetricScalarMultipleRows,
                "metric_scalar_multiple_rows",
            ),
            (
                Code::MetricResultColumnMissing,
                "metric_result_column_missing",
            ),
            (
                Code::MetricResultColumnAmbiguous,
                "metric_result_column_ambiguous",
            ),
        ];

        for (code, expected) in cases {
            assert_eq!(code.as_str(), expected);
            assert_eq!(serde_json::to_value(code).unwrap(), json!(expected));
            let shared_code: CompositionDiagnosticCode = code.into();
            assert_eq!(shared_code.as_str(), expected);
        }
    }

    fn validate_value(
        expected_type: CompositionResultFieldType,
        selected_value: &Value,
    ) -> Result<(), Code> {
        validate_composition_metric_scalar_value(expected_type, selected_value)
    }
}
