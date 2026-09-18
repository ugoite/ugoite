//! 0.1.x `_ugoite_title` compatibility projection for title-less storage.
//!
//! New Form tables have no `ugoite_entry_title` physical column; a non-empty
//! compatibility title travels in
//! `extension_metadata["ugoite/legacy-title"]`. This scalar UDF resolves that
//! encoding inside authorized DataFusion views so existing `_ugoite_title`
//! queries keep working without a table rewrite. It is a pure string
//! function: legacy physical titles never reach it, and title-less rows
//! resolve to empty.

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

pub(crate) const LEGACY_TITLE_FUNCTION_NAME: &str = "ugoite_legacy_title";

/// Resolve the compatibility title from an `extension_metadata` JSON string.
pub(crate) fn legacy_title_from_extension_json(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| {
            value
                .get(ugoite_domain::entry::LEGACY_TITLE_EXTENSION_KEY)
                .and_then(|title| title.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

pub(crate) fn legacy_title_udf() -> Arc<ScalarUDF> {
    Arc::new(ScalarUDF::from(LegacyTitleFunc::new()))
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct LegacyTitleFunc {
    signature: Signature,
}

impl LegacyTitleFunc {
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

impl ScalarUDFImpl for LegacyTitleFunc {
    fn name(&self) -> &str {
        LEGACY_TITLE_FUNCTION_NAME
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, arg_types: &[DataType]) -> Result<DataType> {
        arg_types.first().cloned().ok_or_else(|| {
            DataFusionError::Internal(
                "legacy title resolution requires one string argument".to_string(),
            )
        })
    }

    fn invoke_with_args(&self, args: ScalarFunctionArgs) -> Result<ColumnarValue> {
        let Some(argument) = args.args.first() else {
            return Err(DataFusionError::Internal(
                "legacy title resolution requires one string argument".to_string(),
            ));
        };

        match argument {
            ColumnarValue::Array(array) => resolve_array(array),
            ColumnarValue::Scalar(value) => resolve_scalar(value),
        }
    }
}

fn resolve_array(array: &ArrayRef) -> Result<ColumnarValue> {
    match array.data_type() {
        DataType::Utf8 => {
            let values = array
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| {
                    DataFusionError::Internal("invalid Utf8 legacy title input".to_string())
                })?;
            let mut builder = StringBuilder::with_capacity(values.len(), 0);
            for index in 0..values.len() {
                if values.is_null(index) {
                    builder.append_value("");
                } else {
                    builder.append_value(legacy_title_from_extension_json(values.value(index)));
                }
            }
            Ok(ColumnarValue::Array(Arc::new(builder.finish())))
        }
        DataType::LargeUtf8 => {
            let values = array
                .as_any()
                .downcast_ref::<LargeStringArray>()
                .ok_or_else(|| {
                    DataFusionError::Internal("invalid LargeUtf8 legacy title input".to_string())
                })?;
            let mut builder = LargeStringBuilder::with_capacity(values.len(), 0);
            for index in 0..values.len() {
                if values.is_null(index) {
                    builder.append_value("");
                } else {
                    builder.append_value(legacy_title_from_extension_json(values.value(index)));
                }
            }
            Ok(ColumnarValue::Array(Arc::new(builder.finish())))
        }
        DataType::Utf8View => {
            let values = array
                .as_any()
                .downcast_ref::<StringViewArray>()
                .ok_or_else(|| {
                    DataFusionError::Internal("invalid Utf8View legacy title input".to_string())
                })?;
            let mut builder = StringViewBuilder::with_capacity(values.len());
            for index in 0..values.len() {
                if values.is_null(index) {
                    builder.append_value("");
                } else {
                    builder.append_value(legacy_title_from_extension_json(values.value(index)));
                }
            }
            Ok(ColumnarValue::Array(Arc::new(builder.finish())))
        }
        data_type => Err(DataFusionError::Internal(format!(
            "unsupported legacy title input type: {data_type:?}"
        ))),
    }
}

fn resolve_scalar(value: &ScalarValue) -> Result<ColumnarValue> {
    let resolved = match value {
        ScalarValue::Utf8(value) => ScalarValue::Utf8(Some(
            value
                .as_deref()
                .map_or_else(String::new, legacy_title_from_extension_json),
        )),
        ScalarValue::LargeUtf8(value) => ScalarValue::LargeUtf8(Some(
            value
                .as_deref()
                .map_or_else(String::new, legacy_title_from_extension_json),
        )),
        ScalarValue::Utf8View(value) => ScalarValue::Utf8View(Some(
            value
                .as_deref()
                .map_or_else(String::new, legacy_title_from_extension_json),
        )),
        ScalarValue::Null => ScalarValue::Utf8(Some(String::new())),
        other => {
            return Err(DataFusionError::Internal(format!(
                "unsupported legacy title scalar type: {other:?}"
            )));
        }
    };
    Ok(ColumnarValue::Scalar(resolved))
}

#[cfg(test)]
mod tests {
    use super::legacy_title_from_extension_json;

    #[test]
    fn resolves_legacy_title_and_empty_cases() {
        assert_eq!(
            legacy_title_from_extension_json(r#"{"ugoite/legacy-title":"Alpha"}"#),
            "Alpha"
        );
        assert_eq!(legacy_title_from_extension_json("{}"), "");
        assert_eq!(legacy_title_from_extension_json(""), "");
        assert_eq!(legacy_title_from_extension_json("not json"), "");
        assert_eq!(
            legacy_title_from_extension_json(r#"{"ugoite/legacy-title":42}"#),
            ""
        );
    }
}
