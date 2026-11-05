use std::cmp::Ordering;

use super::{CalculationFailure, exact_integer_as_f64};

#[derive(Debug, Clone, Copy)]
// {
//   責務: [NumericValue: 算術をchecked integerまたはfinite decimalで行う内部値]
// }
pub(super) enum NumericValue {
    Integer(i64),
    Decimal(f64),
}

impl NumericValue {
    // {
    //   責務: [checked_f64: integerの値を保てる場合だけf64へ変換する]
    //   引数: [self: 変換対象]
    //   戻り値: [Result<f64, CalculationFailure>: 正確な数値またはprecision error]
    // }
    pub(super) fn checked_f64(self) -> Result<f64, CalculationFailure> {
        match self {
            Self::Integer(value) => exact_integer_as_f64(i128::from(value)),
            Self::Decimal(value) if value.is_finite() => Ok(value),
            Self::Decimal(_) => Err(CalculationFailure::ArithmeticOverflow),
        }
    }

    // ```text
    // 責務: [compare: integer同士を正確に比較し、integerとdecimalは境界検証後に比較する]
    // 引数: [self: 左numeric値, other: 右numeric値]
    // 戻り値: [Ordering: numericな大小関係]
    // ```
    pub(super) fn compare(self, other: Self) -> Ordering {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => left.cmp(&right),
            (Self::Decimal(left), Self::Decimal(right)) => left.total_cmp(&right),
            (Self::Integer(integer), Self::Decimal(decimal)) => {
                compare_integer_to_decimal(integer, decimal)
            }
            (Self::Decimal(decimal), Self::Integer(integer)) => {
                compare_integer_to_decimal(integer, decimal).reverse()
            }
        }
    }
}

// ```text
// 責務: [compare_integer_to_decimal: 有限decimalとi64の比較でf64丸めによるinteger精度損失を避ける]
// 引数: [integer: 比較するi64, decimal: 有限と検証済みのf64]
// 戻り値: [Ordering: integerとdecimalの大小関係]
// ```
fn compare_integer_to_decimal(integer: i64, decimal: f64) -> Ordering {
    let minimum = i64::MIN as f64;
    let exclusive_maximum = -(i64::MIN as f64);
    if decimal < minimum {
        return Ordering::Greater;
    }
    if decimal >= exclusive_maximum {
        return Ordering::Less;
    }

    match integer.cmp(&(decimal.trunc() as i64)) {
        Ordering::Equal if decimal.fract() > 0.0 => Ordering::Less,
        Ordering::Equal if decimal.fract() < 0.0 => Ordering::Greater,
        ordering => ordering,
    }
}
