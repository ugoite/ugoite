//! Shared Unicode semantics for keyword search.
//!
//! Search is a DataFusion plan over authoritative Entry data and the
//! regenerable AssetText projection. Keeping normalization in one scalar UDF
//! makes both sides of those plans use the same semantics without introducing
//! a second index or changing stored content.

use arrow_array::builder::{LargeStringBuilder, StringBuilder, StringViewBuilder};
use arrow_array::{Array, ArrayRef, LargeStringArray, StringArray, StringViewArray};
use arrow_schema::DataType;
use datafusion::common::{DataFusionError, Result, ScalarValue};
use datafusion::logical_expr::{
    Coercion, ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature,
    TypeSignatureClass, Volatility,
};
use std::fmt::Debug;
use std::sync::Arc;
use unicode_normalization::UnicodeNormalization;

pub(crate) const SEARCH_NORMALIZE_FUNCTION_NAME: &str = "ugoite_search_normalize";

/// Normalizes text for case-insensitive substring search.
///
/// NFKC handles compatibility forms such as fullwidth Latin characters, and
/// Unicode lowercase preserves the case-insensitive behavior already provided
/// by the DataFusion `lower` function for scripts such as Japanese.
pub(crate) fn normalize_search_text(value: &str) -> String {
    value.nfkc().collect::<String>().to_lowercase()
}

pub(crate) fn search_normalize_udf() -> Arc<ScalarUDF> {
    Arc::new(ScalarUDF::from(SearchNormalizeFunc::new()))
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct SearchNormalizeFunc {
    signature: Signature,
}

impl SearchNormalizeFunc {
    fn new() -> Self {
        Self {
            signature: Signature::coercible(
                vec![Coercion::new_exact(TypeSignatureClass::Native(
                    datafusion::common::types::logical_string(),
                ))],
                Volatility::Immutable,
            ),
        }
    }
}

impl ScalarUDFImpl for SearchNormalizeFunc {
    fn name(&self) -> &str {
        SEARCH_NORMALIZE_FUNCTION_NAME
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, arg_types: &[DataType]) -> Result<DataType> {
        arg_types.first().cloned().ok_or_else(|| {
            DataFusionError::Internal(
                "search normalization requires one string argument".to_string(),
            )
        })
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let Some(argument) = args.args.first() else {
            return Err(DataFusionError::Internal(
                "search normalization requires one string argument".to_string(),
            ));
        };

        match argument {
            ColumnarValue::Array(array) => normalize_array(array),
            ColumnarValue::Scalar(value) => normalize_scalar(value),
        }
    }
}

fn normalize_array(array: &ArrayRef) -> Result<ColumnarValue> {
    match array.data_type() {
        DataType::Utf8 => {
            let values = array
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    DataFusionError::Internal("invalid Utf8 search normalization input".to_string())
                })?;
            let mut builder = StringBuilder::with_capacity(values.len(), 0);
            for index in 0..values.len() {
                if values.is_null(index) {
                    builder.append_null();
                } else {
                    builder.append_value(normalize_search_text(values.value(index)));
                }
            }
            Ok(ColumnarValue::Array(Arc::new(builder.finish())))
        }
        DataType::LargeUtf8 => {
            let values = array
                .as_any()
                .downcast_ref::<LargeStringArray>()
                .ok_or_else(|| {
                    DataFusionError::Internal(
                        "invalid LargeUtf8 search normalization input".to_string(),
                    )
                })?;
            let mut builder = LargeStringBuilder::with_capacity(values.len(), 0);
            for index in 0..values.len() {
                if values.is_null(index) {
                    builder.append_null();
                } else {
                    builder.append_value(normalize_search_text(values.value(index)));
                }
            }
            Ok(ColumnarValue::Array(Arc::new(builder.finish())))
        }
        DataType::Utf8View => {
            let values = array
                .as_any()
                .downcast_ref::<StringViewArray>()
                .ok_or_else(|| {
                    DataFusionError::Internal(
                        "invalid Utf8View search normalization input".to_string(),
                    )
                })?;
            let mut builder = StringViewBuilder::with_capacity(values.len());
            for index in 0..values.len() {
                if values.is_null(index) {
                    builder.append_null();
                } else {
                    builder.append_value(normalize_search_text(values.value(index)));
                }
            }
            Ok(ColumnarValue::Array(Arc::new(builder.finish())))
        }
        data_type => Err(DataFusionError::Internal(format!(
            "unsupported search normalization input type: {data_type:?}"
        ))),
    }
}

fn normalize_scalar(value: &ScalarValue) -> Result<ColumnarValue> {
    let normalized = match value {
        ScalarValue::Utf8(value) => ScalarValue::Utf8(value.as_deref().map(normalize_search_text)),
        ScalarValue::LargeUtf8(value) => {
            ScalarValue::LargeUtf8(value.as_deref().map(normalize_search_text))
        }
        ScalarValue::Utf8View(value) => {
            ScalarValue::Utf8View(value.as_deref().map(normalize_search_text))
        }
        other => {
            return Err(DataFusionError::Internal(format!(
                "unsupported search normalization scalar type: {other:?}"
            )))
        }
    };
    Ok(ColumnarValue::Scalar(normalized))
}

#[cfg(test)]
mod tests {
    use super::normalize_search_text;

    #[test]
    fn normalizes_compatibility_and_composed_forms() {
        assert_eq!(normalize_search_text("Ｕｇｏｉｔｅ"), "ugoite");
        assert_eq!(normalize_search_text("Cafe\u{301}"), "café");
        assert_eq!(normalize_search_text("日本語"), "日本語");
    }
}
