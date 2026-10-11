//! Checked numeric SQL aggregates used by the bounded read-only SQL surface.
//!
//! DataFusion 54's built-in SUM and decimal AVG accumulators use wrapping
//! arithmetic for their intermediate state. These implementations retain the
//! same portable result types while accumulating exact values in arbitrary-
//! precision state, including DISTINCT partial-state merging, and check the
//! final value against its declared result type. Floating aggregates reject
//! non-finite inputs and results.

use std::collections::HashSet;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::Arc;

use arrow_array::{Array, ListArray};
use arrow_buffer::i256;
use datafusion::arrow::datatypes::{
    DataType, Field, FieldRef, DECIMAL128_MAX_PRECISION, DECIMAL128_MAX_SCALE,
    DECIMAL256_MAX_PRECISION, DECIMAL256_MAX_SCALE, DECIMAL32_MAX_PRECISION, DECIMAL32_MAX_SCALE,
    DECIMAL64_MAX_PRECISION, DECIMAL64_MAX_SCALE,
};
use datafusion::common::{DataFusionError, Result as DataFusionResult, ScalarValue};
use datafusion::logical_expr::function::{AccumulatorArgs, StateFieldsArgs};
use datafusion::logical_expr::{
    Accumulator, AggregateUDF, AggregateUDFImpl, Signature, Volatility,
};
use num_bigint::BigInt;

/// Internal marker preserved through DataFusion's error source chain so the
/// SQL adapter can map numeric failures to stable, safe public diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SqlAggregateFailure {
    Overflow,
    NonFinite,
    UnsupportedType,
}

impl SqlAggregateFailure {
    pub(crate) const fn code(self) -> ugoite_core::error::ErrorCode {
        use ugoite_core::error::ErrorCode;
        match self {
            Self::Overflow => ErrorCode::SqlAggregateOverflow,
            Self::NonFinite => ErrorCode::SqlAggregateNonFinite,
            Self::UnsupportedType => ErrorCode::SqlAggregateTypeUnsupported,
        }
    }

    pub(crate) const fn public_message(self) -> &'static str {
        match self {
            Self::Overflow => "SQL aggregate result exceeds the supported numeric range",
            Self::NonFinite => "SQL aggregate result is not a finite number",
            Self::UnsupportedType => "SQL aggregate does not support this numeric type",
        }
    }
}

impl Display for SqlAggregateFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Overflow => "UGOITE_SQL_AGGREGATE_OVERFLOW",
            Self::NonFinite => "UGOITE_SQL_AGGREGATE_NON_FINITE",
            Self::UnsupportedType => "UGOITE_SQL_AGGREGATE_TYPE_UNSUPPORTED",
        })
    }
}

impl std::error::Error for SqlAggregateFailure {}

fn failure(error: SqlAggregateFailure) -> DataFusionError {
    DataFusionError::External(Box::new(error))
}

pub(crate) fn checked_sum_udaf() -> AggregateUDF {
    AggregateUDF::new_from_impl(CheckedSum {
        signature: Signature::numeric(1, Volatility::Immutable),
    })
}

pub(crate) fn checked_avg_udaf() -> AggregateUDF {
    AggregateUDF::new_from_impl(CheckedAvg {
        signature: Signature::numeric(1, Volatility::Immutable),
    })
    // DataFusion exposes `mean` as the built-in alias of `avg`; preserve that
    // SQL spelling when substituting the checked accumulator.
    .with_aliases(["mean"])
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct CheckedSum {
    signature: Signature,
}

impl AggregateUDFImpl for CheckedSum {
    fn name(&self) -> &str {
        "sum"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, arg_types: &[DataType]) -> DataFusionResult<DataType> {
        let Some(input) = arg_types.first() else {
            return Err(DataFusionError::Plan(
                "UGOITE_SQL_AGGREGATE_TYPE_UNSUPPORTED".into(),
            ));
        };
        sum_result_type(input).ok_or_else(|| failure(SqlAggregateFailure::UnsupportedType))
    }

    fn accumulator(&self, args: AccumulatorArgs) -> DataFusionResult<Box<dyn Accumulator>> {
        let input_type = args.expr_fields[0].data_type().clone();
        let output_type = args.return_field.data_type().clone();
        CheckedNumericAccumulator::new(
            NumericAggregate::Sum,
            input_type,
            output_type,
            args.is_distinct,
        )
        .map(|accumulator| Box::new(accumulator) as Box<dyn Accumulator>)
    }

    fn state_fields(&self, args: StateFieldsArgs) -> DataFusionResult<Vec<FieldRef>> {
        if args.is_distinct {
            let input_type = args.input_fields[0].data_type().clone();
            return Ok(vec![Arc::new(Field::new_list(
                format!("{}_checked_sum_distinct", args.name),
                Field::new_list_field(input_type, true),
                false,
            ))]);
        }
        Ok(vec![Arc::new(Field::new(
            format!("{}_checked_sum", args.name),
            state_type(args.input_fields[0].data_type()),
            true,
        ))])
    }

    fn order_sensitivity(&self) -> datafusion::logical_expr::utils::AggregateOrderSensitivity {
        datafusion::logical_expr::utils::AggregateOrderSensitivity::Insensitive
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct CheckedAvg {
    signature: Signature,
}

impl AggregateUDFImpl for CheckedAvg {
    fn name(&self) -> &str {
        "avg"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, arg_types: &[DataType]) -> DataFusionResult<DataType> {
        let Some(input) = arg_types.first() else {
            return Err(failure(SqlAggregateFailure::UnsupportedType));
        };
        avg_result_type(input).ok_or_else(|| failure(SqlAggregateFailure::UnsupportedType))
    }

    fn accumulator(&self, args: AccumulatorArgs) -> DataFusionResult<Box<dyn Accumulator>> {
        let input_type = args.expr_fields[0].data_type().clone();
        let output_type = args.return_field.data_type().clone();
        CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            input_type,
            output_type,
            args.is_distinct,
        )
        .map(|accumulator| Box::new(accumulator) as Box<dyn Accumulator>)
    }

    fn state_fields(&self, args: StateFieldsArgs) -> DataFusionResult<Vec<FieldRef>> {
        if args.is_distinct {
            let input_type = args.input_fields[0].data_type().clone();
            return Ok(vec![Arc::new(Field::new_list(
                format!("{}_checked_avg_distinct", args.name),
                Field::new_list_field(input_type, true),
                false,
            ))]);
        }
        let prefix = format!("{}_checked_avg", args.name);
        Ok(vec![
            Arc::new(Field::new(
                format!("{prefix}_count"),
                DataType::UInt64,
                true,
            )),
            Arc::new(Field::new(
                format!("{prefix}_sum"),
                state_type(args.input_fields[0].data_type()),
                true,
            )),
        ])
    }

    fn order_sensitivity(&self) -> datafusion::logical_expr::utils::AggregateOrderSensitivity {
        datafusion::logical_expr::utils::AggregateOrderSensitivity::Insensitive
    }
}

#[derive(Clone, Copy, Debug)]
enum NumericAggregate {
    Sum,
    Average,
}

#[derive(Debug)]
struct CheckedNumericAccumulator {
    aggregate: NumericAggregate,
    input_type: DataType,
    output_type: DataType,
    distinct: bool,
    distinct_values: HashSet<ScalarValue>,
    exact_sum: BigInt,
    float_sum: f64,
    count: u64,
    seen_value: bool,
}

impl CheckedNumericAccumulator {
    fn new(
        aggregate: NumericAggregate,
        input_type: DataType,
        output_type: DataType,
        distinct: bool,
    ) -> DataFusionResult<Self> {
        if sum_result_type(&input_type).is_none() {
            return Err(failure(SqlAggregateFailure::UnsupportedType));
        }
        Ok(Self {
            aggregate,
            input_type,
            output_type,
            distinct,
            distinct_values: HashSet::new(),
            exact_sum: BigInt::ZERO,
            float_sum: 0.0,
            count: 0,
            seen_value: false,
        })
    }

    fn add_scalar(&mut self, value: ScalarValue) -> DataFusionResult<()> {
        if value.is_null() {
            return Ok(());
        }
        if self.distinct && !self.distinct_values.insert(value.clone()) {
            return Ok(());
        }
        self.count = self
            .count
            .checked_add(1)
            .ok_or_else(|| failure(SqlAggregateFailure::Overflow))?;
        match value {
            ScalarValue::Float16(Some(value)) => self.add_float(f64::from(f32::from(value))),
            ScalarValue::Float32(Some(value)) => self.add_float(f64::from(value)),
            ScalarValue::Float64(Some(value)) => self.add_float(value),
            exact => {
                let value = exact_integer_value(&exact)
                    .ok_or_else(|| failure(SqlAggregateFailure::UnsupportedType))?;
                self.exact_sum += value;
                self.seen_value = true;
                Ok(())
            }
        }
    }

    fn add_float(&mut self, value: f64) -> DataFusionResult<()> {
        if !value.is_finite() {
            return Err(failure(SqlAggregateFailure::NonFinite));
        }
        let next = self.float_sum + value;
        if !next.is_finite() {
            return Err(failure(SqlAggregateFailure::NonFinite));
        }
        self.float_sum = next;
        self.seen_value = true;
        Ok(())
    }

    fn scalar_state(&self) -> ScalarValue {
        if is_float(&self.input_type) {
            ScalarValue::Float64(self.seen_value.then_some(self.float_sum))
        } else {
            ScalarValue::Binary(self.seen_value.then(|| self.exact_sum.to_signed_bytes_be()))
        }
    }

    fn distinct_state(&self) -> ScalarValue {
        let mut values = self.distinct_values.iter().cloned().collect::<Vec<_>>();
        values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        ScalarValue::List(ScalarValue::new_list(&values, &self.input_type, true))
    }

    fn merge_distinct_state(&mut self, state: &dyn Array) -> DataFusionResult<()> {
        let Some(list) = state.as_any().downcast_ref::<ListArray>() else {
            return Err(failure(SqlAggregateFailure::UnsupportedType));
        };
        for row in 0..list.len() {
            if list.is_null(row) {
                continue;
            }
            let values = list.value(row);
            for index in 0..values.len() {
                self.add_scalar(ScalarValue::try_from_array(values.as_ref(), index)?)?;
            }
        }
        Ok(())
    }

    fn merge_state(&mut self, state: ScalarValue) -> DataFusionResult<()> {
        if state.is_null() {
            return Ok(());
        }
        match state {
            ScalarValue::Float64(Some(value)) => self.add_float(value)?,
            ScalarValue::Binary(Some(bytes)) | ScalarValue::FixedSizeBinary(_, Some(bytes)) => {
                self.exact_sum += BigInt::from_signed_bytes_be(&bytes);
                self.seen_value = true;
            }
            _ => return Err(failure(SqlAggregateFailure::UnsupportedType)),
        }
        Ok(())
    }

    fn evaluate_sum(&self) -> DataFusionResult<ScalarValue> {
        if !self.seen_value {
            return ScalarValue::try_new_null(&self.output_type);
        }
        match self.output_type {
            DataType::Float64 => Ok(ScalarValue::Float64(Some(self.float_sum))),
            _ => decimal_or_integer_scalar(
                big_int_to_i256(&self.exact_sum)
                    .ok_or_else(|| failure(SqlAggregateFailure::Overflow))?,
                &self.output_type,
            ),
        }
    }

    fn evaluate_average(&self) -> DataFusionResult<ScalarValue> {
        if self.count == 0 {
            return ScalarValue::try_new_null(&self.output_type);
        }
        match self.output_type {
            DataType::Float64 => {
                let sum = if is_float(&self.input_type) {
                    self.float_sum
                } else {
                    self.exact_sum
                        .to_string()
                        .parse::<f64>()
                        .map_err(|_| failure(SqlAggregateFailure::Overflow))?
                };
                let average = sum / self.count as f64;
                if !average.is_finite() {
                    return Err(failure(SqlAggregateFailure::NonFinite));
                }
                Ok(ScalarValue::Float64(Some(average)))
            }
            DataType::Decimal32(_, target_scale)
            | DataType::Decimal64(_, target_scale)
            | DataType::Decimal128(_, target_scale)
            | DataType::Decimal256(_, target_scale) => {
                let (DataType::Decimal32(_, input_scale)
                | DataType::Decimal64(_, input_scale)
                | DataType::Decimal128(_, input_scale)
                | DataType::Decimal256(_, input_scale)) = self.input_type
                else {
                    return Err(failure(SqlAggregateFailure::UnsupportedType));
                };
                let scale_delta = target_scale - input_scale;
                if scale_delta < 0 {
                    return Err(failure(SqlAggregateFailure::UnsupportedType));
                }
                // Keep the sum and scaling exact until division is complete.
                // Decimal256 inputs can have a sum wider than i256 even when
                // their average fits the declared output type.
                let scale_factor = BigInt::from(10_u8).pow(scale_delta as u32);
                let average = (&self.exact_sum * scale_factor) / BigInt::from(self.count);
                decimal_or_integer_scalar(
                    big_int_to_i256(&average)
                        .ok_or_else(|| failure(SqlAggregateFailure::Overflow))?,
                    &self.output_type,
                )
            }
            _ => Err(failure(SqlAggregateFailure::UnsupportedType)),
        }
    }
}

impl Accumulator for CheckedNumericAccumulator {
    fn update_batch(
        &mut self,
        values: &[datafusion::arrow::array::ArrayRef],
    ) -> DataFusionResult<()> {
        let Some(array) = values.first() else {
            return Ok(());
        };
        for index in 0..array.len() {
            self.add_scalar(ScalarValue::try_from_array(array.as_ref(), index)?)?;
        }
        Ok(())
    }

    fn evaluate(&mut self) -> DataFusionResult<ScalarValue> {
        match self.aggregate {
            NumericAggregate::Sum => self.evaluate_sum(),
            NumericAggregate::Average => self.evaluate_average(),
        }
    }

    fn size(&self) -> usize {
        std::mem::size_of_val(self) + ScalarValue::size_of_hashset(&self.distinct_values)
            - std::mem::size_of_val(&self.distinct_values)
            + usize::try_from(self.exact_sum.bits().div_ceil(8)).unwrap_or(0)
    }

    fn state(&mut self) -> DataFusionResult<Vec<ScalarValue>> {
        if self.distinct {
            return Ok(vec![self.distinct_state()]);
        }
        if matches!(self.aggregate, NumericAggregate::Sum) {
            return Ok(vec![self.scalar_state()]);
        }
        Ok(vec![
            ScalarValue::UInt64(self.seen_value.then_some(self.count)),
            self.scalar_state(),
        ])
    }

    fn merge_batch(
        &mut self,
        states: &[datafusion::arrow::array::ArrayRef],
    ) -> DataFusionResult<()> {
        if self.distinct {
            let Some(state) = states.first() else {
                return Ok(());
            };
            return self.merge_distinct_state(state.as_ref());
        }
        if matches!(self.aggregate, NumericAggregate::Sum) {
            let Some(state) = states.first() else {
                return Ok(());
            };
            for index in 0..state.len() {
                self.merge_state(ScalarValue::try_from_array(state.as_ref(), index)?)?;
            }
            return Ok(());
        }
        if states.len() != 2 {
            return Err(failure(SqlAggregateFailure::UnsupportedType));
        }
        for index in 0..states[0].len() {
            let count = ScalarValue::try_from_array(states[0].as_ref(), index)?;
            if let ScalarValue::UInt64(Some(count)) = count {
                self.count = self
                    .count
                    .checked_add(count)
                    .ok_or_else(|| failure(SqlAggregateFailure::Overflow))?;
            }
            self.merge_state(ScalarValue::try_from_array(states[1].as_ref(), index)?)?;
        }
        Ok(())
    }
}

fn sum_result_type(input: &DataType) -> Option<DataType> {
    use DataType::*;
    Some(match input {
        Int8 | Int16 | Int32 | Int64 => Int64,
        UInt8 | UInt16 | UInt32 | UInt64 => UInt64,
        Float16 | Float32 | Float64 => Float64,
        Decimal32(precision, scale) => {
            Decimal32(DECIMAL32_MAX_PRECISION.min(*precision + 10), *scale)
        }
        Decimal64(precision, scale) => {
            Decimal64(DECIMAL64_MAX_PRECISION.min(*precision + 10), *scale)
        }
        Decimal128(precision, scale) => {
            Decimal128(DECIMAL128_MAX_PRECISION.min(*precision + 10), *scale)
        }
        Decimal256(precision, scale) => {
            Decimal256(DECIMAL256_MAX_PRECISION.min(*precision + 10), *scale)
        }
        _ => return None,
    })
}

fn avg_result_type(input: &DataType) -> Option<DataType> {
    use DataType::*;
    Some(match input {
        Int8 | Int16 | Int32 | Int64 | UInt8 | UInt16 | UInt32 | UInt64 | Float16 | Float32
        | Float64 => Float64,
        Decimal32(precision, scale) => Decimal32(
            DECIMAL32_MAX_PRECISION.min(*precision + 4),
            (*scale + 4).min(DECIMAL32_MAX_SCALE),
        ),
        Decimal64(precision, scale) => Decimal64(
            DECIMAL64_MAX_PRECISION.min(*precision + 4),
            (*scale + 4).min(DECIMAL64_MAX_SCALE),
        ),
        Decimal128(precision, scale) => Decimal128(
            DECIMAL128_MAX_PRECISION.min(*precision + 4),
            (*scale + 4).min(DECIMAL128_MAX_SCALE),
        ),
        Decimal256(precision, scale) => Decimal256(
            DECIMAL256_MAX_PRECISION.min(*precision + 4),
            (*scale + 4).min(DECIMAL256_MAX_SCALE),
        ),
        _ => return None,
    })
}

fn exact_integer_value(value: &ScalarValue) -> Option<BigInt> {
    match value {
        ScalarValue::Int8(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::Int16(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::Int32(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::Int64(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::UInt8(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::UInt16(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::UInt32(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::UInt64(Some(value)) => Some(BigInt::from(*value)),
        ScalarValue::Decimal32(Some(value), _, _) => Some(BigInt::from(*value)),
        ScalarValue::Decimal64(Some(value), _, _) => Some(BigInt::from(*value)),
        ScalarValue::Decimal128(Some(value), _, _) => Some(BigInt::from(*value)),
        ScalarValue::Decimal256(Some(value), _, _) => {
            Some(BigInt::from_signed_bytes_be(&value.to_be_bytes()))
        }
        _ => None,
    }
}

fn big_int_to_i256(value: &BigInt) -> Option<i256> {
    let bytes = value.to_signed_bytes_be();
    if bytes.len() > 32 {
        return None;
    }
    let sign_extension = if value.sign() == num_bigint::Sign::Minus {
        u8::MAX
    } else {
        0
    };
    let mut padded = [sign_extension; 32];
    padded[32 - bytes.len()..].copy_from_slice(&bytes);
    Some(i256::from_be_bytes(padded))
}

fn decimal_or_integer_scalar(value: i256, output_type: &DataType) -> DataFusionResult<ScalarValue> {
    use DataType::*;
    let overflow = || failure(SqlAggregateFailure::Overflow);
    Ok(match output_type {
        Int64 => ScalarValue::Int64(Some(
            value
                .to_i128()
                .and_then(|value| i64::try_from(value).ok())
                .ok_or_else(overflow)?,
        )),
        UInt64 => ScalarValue::UInt64(Some(
            value
                .to_i128()
                .and_then(|value| u64::try_from(value).ok())
                .ok_or_else(overflow)?,
        )),
        Decimal32(precision, scale) => {
            validate_decimal_precision(value, *precision)?;
            ScalarValue::Decimal32(
                Some(
                    value
                        .to_i128()
                        .and_then(|value| i32::try_from(value).ok())
                        .ok_or_else(overflow)?,
                ),
                *precision,
                *scale,
            )
        }
        Decimal64(precision, scale) => {
            validate_decimal_precision(value, *precision)?;
            ScalarValue::Decimal64(
                Some(
                    value
                        .to_i128()
                        .and_then(|value| i64::try_from(value).ok())
                        .ok_or_else(overflow)?,
                ),
                *precision,
                *scale,
            )
        }
        Decimal128(precision, scale) => {
            validate_decimal_precision(value, *precision)?;
            ScalarValue::Decimal128(
                Some(value.to_i128().ok_or_else(overflow)?),
                *precision,
                *scale,
            )
        }
        Decimal256(precision, scale) => {
            validate_decimal_precision(value, *precision)?;
            ScalarValue::Decimal256(Some(value), *precision, *scale)
        }
        _ => return Err(failure(SqlAggregateFailure::UnsupportedType)),
    })
}

fn validate_decimal_precision(value: i256, precision: u8) -> DataFusionResult<()> {
    let digits = value.to_string();
    let digits = digits.strip_prefix('-').unwrap_or(&digits);
    if digits.len() > usize::from(precision) {
        return Err(failure(SqlAggregateFailure::Overflow));
    }
    Ok(())
}

fn is_float(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Float16 | DataType::Float32 | DataType::Float64
    )
}

fn state_type(input_type: &DataType) -> DataType {
    if is_float(input_type) {
        DataType::Float64
    } else {
        DataType::Binary
    }
}

#[cfg(test)]
mod tests {
    use super::{
        avg_result_type, sum_result_type, CheckedNumericAccumulator, NumericAggregate,
        SqlAggregateFailure,
    };
    use arrow_array::{ArrayRef, Decimal128Array, Decimal256Array, Float64Array, Int64Array};
    use arrow_buffer::i256;
    use datafusion::arrow::datatypes::DataType;
    use datafusion::common::ScalarValue;
    use datafusion::logical_expr::Accumulator;
    use std::sync::Arc;

    #[test]
    fn numeric_aggregate_types_match_datafusion_portable_outputs() {
        assert_eq!(sum_result_type(&DataType::Int32), Some(DataType::Int64));
        assert_eq!(sum_result_type(&DataType::UInt32), Some(DataType::UInt64));
        assert_eq!(
            sum_result_type(&DataType::Decimal128(10, 2)),
            Some(DataType::Decimal128(20, 2))
        );
        assert_eq!(
            avg_result_type(&DataType::Decimal128(10, 2)),
            Some(DataType::Decimal128(14, 6))
        );
        assert_eq!(avg_result_type(&DataType::Int64), Some(DataType::Float64));
    }

    #[test]
    fn checked_sum_rejects_signed_integer_overflow() {
        let mut accumulator = CheckedNumericAccumulator::new(
            NumericAggregate::Sum,
            DataType::Int64,
            DataType::Int64,
            false,
        )
        .expect("accumulator");
        let values: ArrayRef = Arc::new(Int64Array::from(vec![i64::MAX, 1]));
        accumulator.update_batch(&[values]).expect("wide sum state");
        let error = accumulator
            .evaluate()
            .expect_err("overflow is detected before narrowing");
        assert!(matches!(
            error,
            datafusion::common::DataFusionError::External(ref source)
                if source.downcast_ref::<SqlAggregateFailure>() == Some(&SqlAggregateFailure::Overflow)
        ));
    }

    #[test]
    fn checked_average_keeps_decimal_scale_and_null_semantics() {
        let mut accumulator = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Decimal128(10, 2),
            DataType::Decimal128(14, 6),
            false,
        )
        .expect("accumulator");
        let values: ArrayRef = Arc::new(
            Decimal128Array::from(vec![Some(100_i128), None, Some(200_i128)])
                .with_precision_and_scale(10, 2)
                .expect("decimal array"),
        );
        accumulator.update_batch(&[values]).expect("update");
        assert_eq!(
            accumulator.evaluate().expect("average"),
            ScalarValue::Decimal128(Some(1_500_000), 14, 6)
        );

        let mut empty = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Decimal128(10, 2),
            DataType::Decimal128(14, 6),
            false,
        )
        .expect("empty accumulator");
        let nulls: ArrayRef = Arc::new(
            Decimal128Array::from(vec![None, None])
                .with_precision_and_scale(10, 2)
                .expect("decimal array"),
        );
        empty.update_batch(&[nulls]).expect("update");
        assert_eq!(
            empty.evaluate().expect("empty average"),
            ScalarValue::Decimal128(None, 14, 6)
        );
    }

    #[test]
    fn decimal256_average_avoids_intermediate_scale_overflow() {
        let input =
            i256::from_string(&format!("9{}", "0".repeat(71))).expect("large decimal value");
        let values: ArrayRef = Arc::new(
            Decimal256Array::from(vec![Some(input); 10])
                .with_precision_and_scale(76, 0)
                .expect("Decimal256 array"),
        );
        let mut accumulator = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Decimal256(76, 0),
            DataType::Decimal256(76, 4),
            false,
        )
        .expect("accumulator");
        accumulator.update_batch(&[values]).expect("update");

        let expected = input
            .checked_mul(i256::from(10_000_i32))
            .expect("scaled average fits Decimal256");
        assert_eq!(
            accumulator.evaluate().expect("representable average"),
            ScalarValue::Decimal256(Some(expected), 76, 4)
        );
    }

    #[test]
    fn decimal256_average_accepts_wide_sum_and_merges_partial_states() {
        let input =
            i256::from_string(&format!("9{}", "0".repeat(75))).expect("76-digit decimal value");
        let input_type = DataType::Decimal256(76, 76);
        let output_type = DataType::Decimal256(76, 76);

        let mut direct = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            input_type.clone(),
            output_type.clone(),
            false,
        )
        .expect("direct accumulator");
        direct
            .update_batch(&[Arc::new(
                Decimal256Array::from(vec![Some(input); 7])
                    .with_precision_and_scale(76, 76)
                    .expect("Decimal256 array"),
            )])
            .expect("seven individually representable values have a wide exact sum");
        assert_eq!(
            direct.evaluate().expect("direct average"),
            ScalarValue::Decimal256(Some(input), 76, 76)
        );

        let mut left = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            input_type.clone(),
            output_type.clone(),
            false,
        )
        .expect("left accumulator");
        left.update_batch(&[Arc::new(
            Decimal256Array::from(vec![Some(input); 3])
                .with_precision_and_scale(76, 76)
                .expect("Decimal256 array"),
        )])
        .expect("left values");

        let mut right = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            input_type.clone(),
            output_type.clone(),
            false,
        )
        .expect("right accumulator");
        right
            .update_batch(&[Arc::new(
                Decimal256Array::from(vec![Some(input); 4])
                    .with_precision_and_scale(76, 76)
                    .expect("Decimal256 array"),
            )])
            .expect("right values");

        let mut merged = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            input_type,
            output_type,
            false,
        )
        .expect("merged accumulator");
        for partial in [
            left.state().expect("left state"),
            right.state().expect("right state"),
        ] {
            let count = partial[0].to_array_of_size(1).expect("count state array");
            let sum = partial[1].to_array_of_size(1).expect("sum state array");
            merged
                .merge_batch(&[count, sum])
                .expect("merge partial state");
        }
        assert_eq!(
            merged.evaluate().expect("merged average"),
            ScalarValue::Decimal256(Some(input), 76, 76)
        );
    }

    #[test]
    fn decimal256_average_rejects_a_result_outside_output_precision() {
        let input =
            i256::from_string(&format!("9{}", "0".repeat(75))).expect("76-digit decimal value");
        let values: ArrayRef = Arc::new(
            Decimal256Array::from(vec![Some(input)])
                .with_precision_and_scale(76, 0)
                .expect("Decimal256 array"),
        );
        let mut accumulator = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Decimal256(76, 0),
            DataType::Decimal256(76, 4),
            false,
        )
        .expect("accumulator");
        accumulator.update_batch(&[values]).expect("update");
        let error = accumulator
            .evaluate()
            .expect_err("scaled result exceeds Decimal256 output precision");
        assert!(matches!(
            error,
            datafusion::common::DataFusionError::External(ref source)
                if source.downcast_ref::<SqlAggregateFailure>() == Some(&SqlAggregateFailure::Overflow)
        ));
    }

    #[test]
    fn floating_aggregates_reject_non_finite_inputs_and_results() {
        let mut accumulator = CheckedNumericAccumulator::new(
            NumericAggregate::Sum,
            DataType::Float64,
            DataType::Float64,
            false,
        )
        .expect("accumulator");
        let values: ArrayRef = Arc::new(Float64Array::from(vec![Some(f64::INFINITY)]));
        let error = accumulator
            .update_batch(&[values])
            .expect_err("infinity rejected");
        assert!(matches!(
            error,
            datafusion::common::DataFusionError::External(ref source)
                if source.downcast_ref::<SqlAggregateFailure>() == Some(&SqlAggregateFailure::NonFinite)
        ));
    }

    #[test]
    fn distinct_sum_merges_list_states_without_double_counting() {
        let mut left = CheckedNumericAccumulator::new(
            NumericAggregate::Sum,
            DataType::Int64,
            DataType::Int64,
            true,
        )
        .expect("left accumulator");
        left.update_batch(&[Arc::new(Int64Array::from(vec![1, 2, 2]))])
            .expect("left values");

        let mut right = CheckedNumericAccumulator::new(
            NumericAggregate::Sum,
            DataType::Int64,
            DataType::Int64,
            true,
        )
        .expect("right accumulator");
        right
            .update_batch(&[Arc::new(Int64Array::from(vec![2, 3, 3]))])
            .expect("right values");

        let mut merged = CheckedNumericAccumulator::new(
            NumericAggregate::Sum,
            DataType::Int64,
            DataType::Int64,
            true,
        )
        .expect("merged accumulator");
        for partial in [
            left.state().expect("left state"),
            right.state().expect("right state"),
        ] {
            let batch = partial[0]
                .to_array_of_size(1)
                .expect("list partial state array");
            merged.merge_batch(&[batch]).expect("merge state");
        }
        assert_eq!(
            merged.evaluate().expect("distinct sum"),
            ScalarValue::Int64(Some(6))
        );
    }

    #[test]
    fn distinct_average_merges_partial_states_without_double_counting() {
        let mut left = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Float64,
            DataType::Float64,
            true,
        )
        .expect("left accumulator");
        left.update_batch(&[Arc::new(Float64Array::from(vec![2.0, 2.0, 8.0]))])
            .expect("left values");

        let mut right = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Float64,
            DataType::Float64,
            true,
        )
        .expect("right accumulator");
        right
            .update_batch(&[Arc::new(Float64Array::from(vec![2.0, 8.0, 14.0]))])
            .expect("right values");

        let mut merged = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Float64,
            DataType::Float64,
            true,
        )
        .expect("merged accumulator");
        for partial in [
            left.state().expect("left state"),
            right.state().expect("right state"),
        ] {
            let batch = partial[0]
                .to_array_of_size(1)
                .expect("list partial state array");
            merged.merge_batch(&[batch]).expect("merge state");
        }

        assert_eq!(
            merged.evaluate().expect("distinct average"),
            ScalarValue::Float64(Some(8.0))
        );
    }

    #[test]
    fn average_truncates_negative_decimal_fraction_toward_zero() {
        let values: ArrayRef = Arc::new(
            Decimal128Array::from(vec![Some(-1_i128), Some(0_i128)])
                .with_precision_and_scale(10, 2)
                .expect("decimal array"),
        );
        let mut accumulator = CheckedNumericAccumulator::new(
            NumericAggregate::Average,
            DataType::Decimal128(10, 2),
            DataType::Decimal128(14, 6),
            false,
        )
        .expect("accumulator");
        accumulator.update_batch(&[values]).expect("update");
        assert_eq!(
            accumulator.evaluate().expect("negative average"),
            ScalarValue::Decimal128(Some(-5_000), 14, 6)
        );
    }
}
