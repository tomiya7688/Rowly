use std::cmp::Ordering;

use super::types::{
    CalculationExpression, CalculationFailure, CalculationOperator, CalculationValue,
    PureCalculationFunction,
};
use crate::process::CellRef;

pub(super) const MAX_EXPRESSION_DEPTH: usize = 256;

// ```text
// 責務: [expression_depth: ASTをstack traversalで走査してrootからの最大node深さを返す]
// 引数: [expression: 評価対象AST]
// 戻り値: [usize: 最大node深さ]
// ```
pub(super) fn expression_depth(expression: &CalculationExpression) -> usize {
    let mut pending = vec![(expression, 0usize)];
    let mut maximum_depth = 0;

    while let Some((expression, depth)) = pending.pop() {
        maximum_depth = maximum_depth.max(depth);
        match expression {
            CalculationExpression::Literal(_) | CalculationExpression::Cell(_) => {}
            CalculationExpression::Negate(operand) => pending.push((operand, depth + 1)),
            CalculationExpression::Arithmetic { left, right, .. } => {
                pending.push((left, depth + 1));
                pending.push((right, depth + 1));
            }
            CalculationExpression::Call { arguments, .. } => {
                pending.extend(arguments.iter().map(|argument| (argument, depth + 1)));
            }
        }
    }

    maximum_depth
}

// {
//   責務: [evaluate_expression: 副作用なしASTを現在のCSVと派生結果から評価する]
//   処理: [literal、cell read、checked arithmetic、固定pure functionだけを実行する]
//   引数: [expression: 評価対象, depth: rootからのnode深さ, read_cell: 制限されたcell reader]
//   戻り値: [Result<CalculationValue, CalculationFailure>: scalar resultまたは評価理由]
// }
pub(super) fn evaluate_expression(
    expression: &CalculationExpression,
    depth: usize,
    read_cell: &mut impl FnMut(CellRef) -> Result<CalculationValue, CalculationFailure>,
) -> Result<CalculationValue, CalculationFailure> {
    if depth > MAX_EXPRESSION_DEPTH {
        return Err(CalculationFailure::ExpressionTooDeep);
    }

    match expression {
        CalculationExpression::Literal(CalculationValue::Decimal(value)) if !value.is_finite() => {
            Err(CalculationFailure::ArithmeticOverflow)
        }
        CalculationExpression::Literal(value) => Ok(value.clone()),
        CalculationExpression::Cell(reference) => read_cell(*reference),
        CalculationExpression::Negate(operand) => {
            let value = evaluate_expression(operand, depth + 1, read_cell)?;
            negate(value)
        }
        CalculationExpression::Arithmetic {
            left,
            operator,
            right,
        } => {
            let left = evaluate_expression(left, depth + 1, read_cell)?;
            let right = evaluate_expression(right, depth + 1, read_cell)?;
            arithmetic(left, *operator, right)
        }
        CalculationExpression::Call {
            function,
            arguments,
        } => {
            let expected = match function {
                PureCalculationFunction::Abs => 1,
                PureCalculationFunction::Min | PureCalculationFunction::Max => 2,
            };
            if arguments.len() != expected {
                return Err(CalculationFailure::InvalidFunctionArguments {
                    function: function_name(*function),
                    expected,
                    actual: arguments.len(),
                });
            }

            let values = arguments
                .iter()
                .map(|argument| evaluate_expression(argument, depth + 1, read_cell))
                .collect::<Result<Vec<_>, _>>()?;
            call_pure_function(*function, values)
        }
    }
}

// {
//   責務: [negate: 数値scalarの符号をchecked arithmeticで反転する]
//   引数: [value: 符号反転するscalar]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 反転値または型・overflow error]
// }
fn negate(value: CalculationValue) -> Result<CalculationValue, CalculationFailure> {
    match numeric_value(value)? {
        NumericValue::Integer(value) => value
            .checked_neg()
            .map(CalculationValue::Integer)
            .ok_or(CalculationFailure::ArithmeticOverflow),
        NumericValue::Decimal(value) => finite_decimal(-value),
    }
}

// {
//   責務: [arithmetic: 2つのnumeric scalarへ算術operatorを適用する]
//   引数: [left: 左辺値, operator: 算術operator, right: 右辺値]
//   戻り値: [Result<CalculationValue, CalculationFailure>: checkedなscalar結果]
// }
fn arithmetic(
    left: CalculationValue,
    operator: CalculationOperator,
    right: CalculationValue,
) -> Result<CalculationValue, CalculationFailure> {
    let left = numeric_value(left)?;
    let right = numeric_value(right)?;

    if operator == CalculationOperator::Divide {
        if let (NumericValue::Integer(left), NumericValue::Integer(right)) = (left, right) {
            if right == 0 {
                return Err(CalculationFailure::DivisionByZero);
            }
            let remainder = left
                .checked_rem(right)
                .ok_or(CalculationFailure::ArithmeticOverflow)?;
            if remainder == 0 {
                return left
                    .checked_div(right)
                    .map(CalculationValue::Integer)
                    .ok_or(CalculationFailure::ArithmeticOverflow);
            }
            let quotient = left
                .checked_div(right)
                .ok_or(CalculationFailure::ArithmeticOverflow)?;
            // Reject integer components that cannot survive the existing precision contract.
            let _ = exact_integer_as_f64(i128::from(quotient))?;
            let (reduced_remainder, reduced_divisor) =
                reduce_fraction(i128::from(remainder), i128::from(right));
            let _ = exact_integer_as_f64(reduced_remainder)?;
            let _ = exact_integer_as_f64(reduced_divisor)?;
            return finite_decimal(round_integer_ratio_as_f64(left, right)?);
        }
        let (left, right) = (left.checked_f64()?, right.checked_f64()?);
        if right == 0.0 {
            return Err(CalculationFailure::DivisionByZero);
        }
        return finite_decimal(left / right);
    }

    match (left, right) {
        (NumericValue::Integer(left), NumericValue::Integer(right)) => {
            let result = match operator {
                CalculationOperator::Add => left.checked_add(right),
                CalculationOperator::Subtract => left.checked_sub(right),
                CalculationOperator::Multiply => left.checked_mul(right),
                CalculationOperator::Divide => None,
            };
            result
                .map(CalculationValue::Integer)
                .ok_or(CalculationFailure::ArithmeticOverflow)
        }
        (left, right) => {
            let left = left.checked_f64()?;
            let right = right.checked_f64()?;
            let result = match operator {
                CalculationOperator::Add => left + right,
                CalculationOperator::Subtract => left - right,
                CalculationOperator::Multiply => left * right,
                CalculationOperator::Divide => left / right,
            };
            finite_decimal(result)
        }
    }
}

// {
//   責務: [numeric_value: Integer/Decimalまたは数値文字列を内部numeric型へ変換する]
//   引数: [value: 判定するscalar]
//   戻り値: [Result<NumericValue, CalculationFailure>: numeric値または入力型error]
// }
fn numeric_value(value: CalculationValue) -> Result<NumericValue, CalculationFailure> {
    match value {
        CalculationValue::Integer(value) => Ok(NumericValue::Integer(value)),
        CalculationValue::Decimal(value) if value.is_finite() => Ok(NumericValue::Decimal(value)),
        CalculationValue::Decimal(_) => Err(CalculationFailure::ArithmeticOverflow),
        CalculationValue::Text(value) => {
            if let Ok(integer) = value.parse::<i64>() {
                return Ok(NumericValue::Integer(integer));
            }
            match value.parse::<f64>() {
                Ok(decimal) if decimal.is_finite() => Ok(NumericValue::Decimal(decimal)),
                _ => Err(CalculationFailure::ExpectedNumber(value)),
            }
        }
    }
}

// {
//   責務: [call_pure_function: 許可したAbs/Min/Max関数だけを評価する]
//   引数: [function: 呼び出すpure function, arguments: 個数検証済みscalar引数]
//   戻り値: [Result<CalculationValue, CalculationFailure>: function resultまたは型・overflow error]
// }
fn call_pure_function(
    function: PureCalculationFunction,
    arguments: Vec<CalculationValue>,
) -> Result<CalculationValue, CalculationFailure> {
    match function {
        PureCalculationFunction::Abs => negate_abs(arguments.into_iter().next().ok_or(
            CalculationFailure::InvalidFunctionArguments {
                function: "Abs",
                expected: 1,
                actual: 0,
            },
        )?),
        PureCalculationFunction::Min | PureCalculationFunction::Max => {
            let mut arguments = arguments.into_iter();
            let first = arguments
                .next()
                .ok_or(CalculationFailure::InvalidFunctionArguments {
                    function: function_name(function),
                    expected: 2,
                    actual: 0,
                })?;
            let second = arguments
                .next()
                .ok_or(CalculationFailure::InvalidFunctionArguments {
                    function: function_name(function),
                    expected: 2,
                    actual: 1,
                })?;
            let comparison = numeric_value(first.clone())?.compare(numeric_value(second.clone())?);
            let take_first = match function {
                PureCalculationFunction::Min => comparison != Ordering::Greater,
                PureCalculationFunction::Max => comparison != Ordering::Less,
                PureCalculationFunction::Abs => true,
            };
            Ok(if take_first { first } else { second })
        }
    }
}

// {
//   責務: [negate_abs: numeric valueのabsolute valueをoverflowなく返す]
//   引数: [value: absolute valueへ変換するscalar]
//   戻り値: [Result<CalculationValue, CalculationFailure>: absolute valueまたは型・overflow error]
// }
fn negate_abs(value: CalculationValue) -> Result<CalculationValue, CalculationFailure> {
    match numeric_value(value)? {
        NumericValue::Integer(value) => value
            .checked_abs()
            .map(CalculationValue::Integer)
            .ok_or(CalculationFailure::ArithmeticOverflow),
        NumericValue::Decimal(value) => finite_decimal(value.abs()),
    }
}

// {
//   責務: [finite_decimal: finiteなf64だけをcalculation valueとして受け入れる]
//   引数: [value: 検査するdecimal]
//   戻り値: [Result<CalculationValue, CalculationFailure>: finite decimalまたはoverflow error]
// }
fn finite_decimal(value: f64) -> Result<CalculationValue, CalculationFailure> {
    if value.is_finite() {
        Ok(CalculationValue::Decimal(value))
    } else {
        Err(CalculationFailure::ArithmeticOverflow)
    }
}

// ```text
// 責務: [exact_integer_as_f64: i64からf64への変換で値が変わる場合はprecision errorを返す]
// 引数: [integer: f64へ変換するinteger]
// 戻り値: [Result<f64, CalculationFailure>: exact conversionまたはprecision loss]
// ```
fn exact_integer_as_f64(integer: i128) -> Result<f64, CalculationFailure> {
    let decimal = integer as f64;
    if decimal as i128 != integer {
        return Err(CalculationFailure::PrecisionLoss);
    }
    Ok(decimal)
}

// ```text
// 責務: [reduce_fraction: 割り算の余りと除数を最大公約数で約分する]
// 引数: [numerator: signed remainder, denominator: signed divisor]
// 戻り値: [(i128, i128): 約分後の分子と分母]
// ```
fn reduce_fraction(numerator: i128, denominator: i128) -> (i128, i128) {
    let mut left = numerator.unsigned_abs();
    let mut right = denominator.unsigned_abs();
    while right != 0 {
        (left, right) = (right, left % right);
    }
    let greatest_common_divisor = left as i128;
    (
        numerator / greatest_common_divisor,
        denominator / greatest_common_divisor,
    )
}

// {
//   責務: [round_integer_ratio_as_f64: i64の比を中間丸めなしで最近接f64へ変換する]
//   処理: [符号と二進指数を求め、53桁の仮数とguard/sticky bitでties-to-even丸めを行う]
//   引数: [numerator: 分子, denominator: 0以外の分母]
//   戻り値: [Result<f64, CalculationFailure>: 最近接のfinite値または0除算error]
// }
fn round_integer_ratio_as_f64(numerator: i64, denominator: i64) -> Result<f64, CalculationFailure> {
    if denominator == 0 {
        return Err(CalculationFailure::DivisionByZero);
    }
    if numerator == 0 {
        return Ok(0.0);
    }

    let numerator_magnitude = u128::from(numerator.unsigned_abs());
    let denominator_magnitude = u128::from(denominator.unsigned_abs());
    let mut exponent = (127 - numerator_magnitude.leading_zeros() as i32)
        - (127 - denominator_magnitude.leading_zeros() as i32);

    // Bit lengths give an exponent that is at most one too large.
    let ratio_is_below_exponent = if exponent >= 0 {
        numerator_magnitude < (denominator_magnitude << exponent as u32)
    } else {
        (numerator_magnitude << (-exponent) as u32) < denominator_magnitude
    };
    if ratio_is_below_exponent {
        exponent -= 1;
    }

    // Scale the exact ratio into [1, 2) before generating its binary significand.
    let (scaled_numerator, scaled_denominator) = if exponent >= 0 {
        (
            numerator_magnitude,
            denominator_magnitude << exponent as u32,
        )
    } else {
        (
            numerator_magnitude << (-exponent) as u32,
            denominator_magnitude,
        )
    };
    let mut remainder = scaled_numerator - scaled_denominator;
    let mut significand = 1_u64 << 52;

    // Generate the 52 stored fraction bits from the exact integer remainder.
    for bit in (0..52).rev() {
        remainder <<= 1;
        if remainder >= scaled_denominator {
            remainder -= scaled_denominator;
            significand |= 1_u64 << bit;
        }
    }

    // The next bit and remaining fraction decide round-to-nearest, ties-to-even.
    remainder <<= 1;
    let guard_bit = remainder >= scaled_denominator;
    if guard_bit {
        remainder -= scaled_denominator;
    }
    let sticky_bit = remainder != 0;
    if guard_bit && (sticky_bit || significand & 1 == 1) {
        significand += 1;
    }
    if significand == 1_u64 << 53 {
        significand >>= 1;
        exponent += 1;
    }

    let magnitude = (significand as f64) * 2.0_f64.powi(exponent - 52);
    Ok(if numerator.is_negative() ^ denominator.is_negative() {
        -magnitude
    } else {
        magnitude
    })
}

// {
//   責務: [function_name: pure function enumを安定したerror名へ変換する]
//   引数: [function: 対象function]
//   戻り値: [&'static str: function名]
// }
const fn function_name(function: PureCalculationFunction) -> &'static str {
    match function {
        PureCalculationFunction::Abs => "Abs",
        PureCalculationFunction::Min => "Min",
        PureCalculationFunction::Max => "Max",
    }
}

#[derive(Debug, Clone, Copy)]
// {
//   責務: [NumericValue: 算術をchecked integerまたはfinite decimalで行う内部値]
// }
enum NumericValue {
    Integer(i64),
    Decimal(f64),
}

impl NumericValue {
    // {
    //   責務: [checked_f64: integerの値を保てる場合だけf64へ変換する]
    //   引数: [self: 変換対象]
    //   戻り値: [Result<f64, CalculationFailure>: 正確な数値またはprecision error]
    // }
    fn checked_f64(self) -> Result<f64, CalculationFailure> {
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
    fn compare(self, other: Self) -> Ordering {
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

impl CalculationValue {
    // {
    //   責務: [into_text: 評価結果をCSV文字列相当のderived valueへ直列化する]
    //   引数: [self: 直列化するscalar]
    //   戻り値: [String: canonical CSVを書き換えずViewへ渡す表示可能文字列]
    // }
    pub(super) fn into_text(self) -> String {
        match self {
            Self::Text(value) => value,
            Self::Integer(value) => value.to_string(),
            Self::Decimal(value) => value.to_string(),
        }
    }
}

