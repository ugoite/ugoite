//! Composition Metric query contracts and bounded Saved SQL scalar handling.
//!
//! Query execution and current authorization remain in the storage-backed
//! service. This module selects the requested resolved metric and validates a
//! two-row probe without treating it as a paged table result.

use crate::composition::{CompositionDiagnostic, ResolvedComponentBinding, ResolvedComponentKind};
use crate::sql_query::SqlScalarProbeResult;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use ugoite_domain::composition::{
    CompositionAggregateOperation, CompositionDiagnosticCode, CompositionResultFieldType,
};
use ugoite_domain::composition_metric::validate_composition_metric_scalar_value;

/// Target of a read-only Composition Metric query.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompositionMetricQueryTarget {
    /// Query one exact stored Composition revision.
    SavedRevision {
        composition_id: String,
        revision_id: String,
    },
    /// Query one unsaved draft without publishing or creating history.
    Draft { yaml: String },
}

/// Request to evaluate one metric component with current parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryRequest {
    pub target: CompositionMetricQueryTarget,
    pub component_id: String,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

/// Semantic operation applied by a metric component.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionMetricOperation {
    SqlScalar,
    CountRows,
    CountValues,
    Sum,
    Avg,
    Min,
    Max,
}

impl From<CompositionAggregateOperation> for CompositionMetricOperation {
    fn from(operation: CompositionAggregateOperation) -> Self {
        match operation {
            CompositionAggregateOperation::CountRows => Self::CountRows,
            CompositionAggregateOperation::CountValues => Self::CountValues,
            CompositionAggregateOperation::Sum => Self::Sum,
            CompositionAggregateOperation::Avg => Self::Avg,
            CompositionAggregateOperation::Min => Self::Min,
            CompositionAggregateOperation::Max => Self::Max,
        }
    }
}

/// Semantic source category. Stable source IDs stay in the Composition and
/// are not repeated in the user-visible result.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionMetricSourceKind {
    EntryQuery,
    SavedSql,
}

/// One successfully evaluated metric value and its portable meaning.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryValue {
    pub component_id: String,
    pub operation: CompositionMetricOperation,
    pub source_kind: CompositionMetricSourceKind,
    pub result_type: CompositionResultFieldType,
    /// Integer results are canonical base-10 strings so Browser JSON parsing
    /// cannot round values above JavaScript's exact integer range. Decimal
    /// results are strings and preserve their declared scale.
    pub value: Value,
}

/// Encode one validated metric result for lossless transport through JSON and
/// JavaScript. The result type is carried beside the value, so integer
/// scalars use decimal text rather than a JSON number that JSON.parse would
/// round above 2^53.
pub fn encode_composition_metric_value(
    result_type: ugoite_domain::composition::CompositionResultFieldType,
    value: Value,
) -> Result<Value, CompositionDiagnostic> {
    let encoded = if value.is_null() {
        Value::Null
    } else if result_type == ugoite_domain::composition::CompositionResultFieldType::Integer {
        match &value {
            Value::Number(number) if number.as_i64().is_some() || number.as_u64().is_some() => {
                Value::String(number.to_string())
            }
            Value::String(_) => value,
            _ => {
                return Err(CompositionDiagnostic {
                    code: CompositionDiagnosticCode::MetricResultTypeMismatch,
                    parameter_id: None,
                });
            }
        }
    } else {
        value
    };
    validate_composition_metric_scalar_value(result_type, &encoded).map_err(|code| {
        CompositionDiagnostic {
            code: code.into(),
            parameter_id: None,
        }
    })?;
    Ok(encoded)
}

/// Stable diagnostic for a failed metric query.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryDiagnostic {
    pub code: CompositionDiagnosticCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameter_id: Option<String>,
}

/// Result of one metric query. A successful SQL `NULL` is represented as a
/// present `value: null`; zero rows is a separate diagnostic outcome.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompositionMetricQueryResponse {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<CompositionMetricQueryValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<CompositionMetricQueryDiagnostic>,
}

impl CompositionMetricQueryResponse {
    pub fn from_diagnostics(
        draft_fingerprint: Option<String>,
        diagnostics: impl IntoIterator<Item = CompositionDiagnostic>,
    ) -> Self {
        Self {
            ok: false,
            draft_fingerprint,
            result: None,
            diagnostics: diagnostics
                .into_iter()
                .map(|diagnostic| CompositionMetricQueryDiagnostic {
                    code: diagnostic.code,
                    component_id: None,
                    parameter_id: diagnostic.parameter_id,
                })
                .collect(),
        }
    }
}

/// Evaluate an exact Saved SQL column from a bounded two-row probe.
pub fn evaluate_saved_sql_metric_probe(
    binding: &ResolvedComponentBinding,
    probe: &SqlScalarProbeResult,
) -> Result<Value, CompositionDiagnostic> {
    let diagnostic = |code| CompositionDiagnostic {
        code,
        parameter_id: None,
    };
    if binding.kind != ResolvedComponentKind::Metric || binding.metric_field_id.is_some() {
        return Err(diagnostic(CompositionDiagnosticCode::InvalidComposition));
    }
    let expected_type = binding
        .expected_result_type
        .ok_or_else(|| diagnostic(CompositionDiagnosticCode::InvalidComposition))?;
    let column_name = binding
        .result_property_key
        .as_deref()
        .ok_or_else(|| diagnostic(CompositionDiagnosticCode::InvalidComposition))?;

    if probe.rows.is_empty() {
        return Err(diagnostic(CompositionDiagnosticCode::MetricResultEmpty));
    }
    if probe.rows.len() > 1 {
        return Err(diagnostic(
            CompositionDiagnosticCode::MetricScalarExpectedOneRow,
        ));
    }

    let column_count = probe
        .columns
        .iter()
        .filter(|column| column.as_str() == column_name)
        .count();
    if column_count == 0 {
        return Err(diagnostic(
            CompositionDiagnosticCode::MetricResultColumnMissing,
        ));
    }
    if column_count > 1 {
        return Err(diagnostic(
            CompositionDiagnosticCode::MetricResultColumnAmbiguous,
        ));
    }

    let schema_columns = probe
        .result_schema
        .iter()
        .filter(|column| column.name == column_name)
        .collect::<Vec<_>>();
    let Some(schema_column) = schema_columns.first() else {
        return Err(diagnostic(
            CompositionDiagnosticCode::MetricResultColumnMissing,
        ));
    };
    if schema_columns.len() > 1 {
        return Err(diagnostic(
            CompositionDiagnosticCode::MetricResultColumnAmbiguous,
        ));
    }
    if schema_column.column_type.as_str() != expected_type.as_str() {
        return Err(diagnostic(
            CompositionDiagnosticCode::MetricResultTypeMismatch,
        ));
    }

    let Some(row) = probe.rows[0].as_object() else {
        return Err(diagnostic(
            CompositionDiagnosticCode::MetricResultColumnMissing,
        ));
    };
    // The bounded SQL JSON writer omits NULL-valued object properties. The
    // exact output column has already been confirmed in both the column list
    // and typed result schema, so an omitted cell in this row is SQL NULL.
    let selected_value = row.get(column_name).unwrap_or(&Value::Null);
    let encoded = encode_composition_metric_value(expected_type, selected_value.clone())?;
    validate_composition_metric_scalar_value(expected_type, &encoded)
        .map_err(|code| diagnostic(code.into()))?;
    Ok(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql_query::{SqlResultColumn, SqlResultColumnType};
    use serde_json::json;
    use ugoite_domain::composition::CompositionResultFieldType;

    fn binding(result_type: CompositionResultFieldType) -> ResolvedComponentBinding {
        ResolvedComponentBinding {
            component_id: "total".to_owned(),
            kind: ResolvedComponentKind::Metric,
            label: Some("合計".to_owned()),
            source_id: Some("monthly_sql".to_owned()),
            metric_field_id: None,
            aggregate_operation: None,
            result_property_key: Some("total".to_owned()),
            expected_result_type: Some(result_type),
        }
    }

    fn probe(rows: Vec<Value>, columns: &[&str]) -> SqlScalarProbeResult {
        SqlScalarProbeResult {
            columns: columns.iter().map(|column| (*column).to_owned()).collect(),
            rows,
            result_schema: columns
                .iter()
                .map(|column| SqlResultColumn {
                    name: (*column).to_owned(),
                    column_type: SqlResultColumnType::Integer,
                })
                .collect(),
        }
    }

    #[test]
    fn sql_scalar_probe_returns_one_typed_column_from_one_row() {
        let result = evaluate_saved_sql_metric_probe(
            &binding(CompositionResultFieldType::Integer),
            &probe(vec![json!({"total": 42, "other": 9})], &["total", "other"]),
        )
        .unwrap();
        assert_eq!(result, json!("42"));
    }

    #[test]
    fn integer_metric_transport_preserves_values_beyond_javascript_safe_range() {
        let value = json!(9_007_199_254_740_993_i64);
        assert_eq!(
            encode_composition_metric_value(CompositionResultFieldType::Integer, value).unwrap(),
            json!("9007199254740993")
        );
    }

    #[test]
    fn sql_scalar_probe_preserves_null_as_a_present_scalar_value() {
        let result = evaluate_saved_sql_metric_probe(
            &binding(CompositionResultFieldType::Integer),
            &probe(vec![json!({"total": null})], &["total"]),
        )
        .unwrap();
        assert!(result.is_null());

        let omitted_null = evaluate_saved_sql_metric_probe(
            &binding(CompositionResultFieldType::Integer),
            &probe(vec![json!({})], &["total"]),
        )
        .unwrap();
        assert!(omitted_null.is_null());
    }

    #[test]
    fn sql_scalar_probe_distinguishes_empty_and_multiple_row_results() {
        assert_eq!(
            evaluate_saved_sql_metric_probe(
                &binding(CompositionResultFieldType::Integer),
                &probe(Vec::new(), &["total"]),
            )
            .unwrap_err()
            .code,
            CompositionDiagnosticCode::MetricResultEmpty
        );
        assert_eq!(
            evaluate_saved_sql_metric_probe(
                &binding(CompositionResultFieldType::Integer),
                &probe(vec![json!({"total": 1}), json!({"total": 2})], &["total"]),
            )
            .unwrap_err()
            .code,
            CompositionDiagnosticCode::MetricScalarExpectedOneRow
        );
    }

    #[test]
    fn sql_scalar_probe_rejects_ambiguous_columns_and_type_drift() {
        assert_eq!(
            evaluate_saved_sql_metric_probe(
                &binding(CompositionResultFieldType::Integer),
                &probe(vec![json!({"total": 1})], &["total", "total"]),
            )
            .unwrap_err()
            .code,
            CompositionDiagnosticCode::MetricResultColumnAmbiguous
        );

        assert_eq!(
            evaluate_saved_sql_metric_probe(
                &binding(CompositionResultFieldType::Float),
                &probe(vec![json!({"total": 1})], &["total"]),
            )
            .unwrap_err()
            .code,
            CompositionDiagnosticCode::MetricResultTypeMismatch
        );
    }
}
