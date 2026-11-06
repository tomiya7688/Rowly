use std::cmp::Ordering;

use super::types::{
    CalculationExpression, CalculationFailure, CalculationOperator, CalculationValue,
    PureCalculationFunction,
};
use crate::process::CellRef;

mod aggregate;
pub use aggregate::aggregate_texts;
mod conditional;
mod numeric;
use numeric::NumericValue;
mod compare;
mod date;
mod logic;
mod math;
mod text;
use math::{RoundingMode, modulo, round_to_integer};
mod ratio;
use ratio::{reduce_fraction, round_integer_ratio_as_f64};

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
            CalculationExpression::Literal(_)
            | CalculationExpression::Cell(_)
            | CalculationExpression::AnchoredCell(_)
            | CalculationExpression::DeletedReference
            | CalculationExpression::Aggregate { .. } => {}
            CalculationExpression::ConditionalAggregate { value, .. } => {
                pending.push((value, depth + 1));
            }
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
        CalculationExpression::AnchoredCell(reference) => read_cell(reference.cell()),
        CalculationExpression::DeletedReference => Err(CalculationFailure::DeletedReference),
        CalculationExpression::ConditionalAggregate {
            function,
            criteria_range,
            comparison,
            value,
            sum_range,
        } => conditional::evaluate_conditional(
            conditional::ConditionalSpec {
                function: *function,
                criteria_range: *criteria_range,
                comparison: *comparison,
                value,
                sum_range: *sum_range,
            },
            depth,
            read_cell,
        ),
        CalculationExpression::Aggregate { function, range } => {
            aggregate::evaluate_aggregate(*function, *range, read_cell)
        }
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
                PureCalculationFunction::Abs
                | PureCalculationFunction::Round
                | PureCalculationFunction::Floor
                | PureCalculationFunction::Ceil
                | PureCalculationFunction::Len
                | PureCalculationFunction::Trim
                | PureCalculationFunction::Upper
                | PureCalculationFunction::Lower
                | PureCalculationFunction::Year
                | PureCalculationFunction::Month
                | PureCalculationFunction::Day
                | PureCalculationFunction::Not => 1,
                PureCalculationFunction::Min
                | PureCalculationFunction::Max
                | PureCalculationFunction::Mod
                | PureCalculationFunction::Concat
                | PureCalculationFunction::Left
                | PureCalculationFunction::Right
                | PureCalculationFunction::And
                | PureCalculationFunction::Or
                | PureCalculationFunction::Equals
                | PureCalculationFunction::NotEquals
                | PureCalculationFunction::Less
                | PureCalculationFunction::LessOrEqual
                | PureCalculationFunction::Greater
                | PureCalculationFunction::GreaterOrEqual => 2,
                PureCalculationFunction::Mid
                | PureCalculationFunction::Date
                | PureCalculationFunction::If => 3,
            };
            if arguments.len() != expected {
                return Err(CalculationFailure::InvalidFunctionArguments {
                    function: function_name(*function),
                    expected,
                    actual: arguments.len(),
                });
            }

            if let Some(result) = logic::evaluate_lazy(*function, arguments, depth, read_cell) {
                return result;
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
        if matches!(right, NumericValue::Decimal(value) if value == 0.0) {
            return Err(CalculationFailure::DivisionByZero);
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
        CalculationValue::Boolean(value) => Err(CalculationFailure::ExpectedNumber(
            CalculationValue::Boolean(value).into_text(),
        )),
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
        PureCalculationFunction::Round
        | PureCalculationFunction::Floor
        | PureCalculationFunction::Ceil => {
            let mode = match function {
                PureCalculationFunction::Floor => RoundingMode::Floor,
                PureCalculationFunction::Ceil => RoundingMode::Ceil,
                _ => RoundingMode::HalfAwayFromZero,
            };
            let value = arguments.into_iter().next().ok_or(
                CalculationFailure::InvalidFunctionArguments {
                    function: function_name(function),
                    expected: 1,
                    actual: 0,
                },
            )?;
            round_to_integer(value, mode)
        }
        PureCalculationFunction::Mod => {
            let mut arguments = arguments.into_iter();
            let (Some(dividend), Some(divisor)) = (arguments.next(), arguments.next()) else {
                return Err(CalculationFailure::InvalidFunctionArguments {
                    function: "Mod",
                    expected: 2,
                    actual: 0,
                });
            };
            modulo(dividend, divisor)
        }
        PureCalculationFunction::Len
        | PureCalculationFunction::Concat
        | PureCalculationFunction::Left
        | PureCalculationFunction::Right
        | PureCalculationFunction::Mid
        | PureCalculationFunction::Trim
        | PureCalculationFunction::Upper
        | PureCalculationFunction::Lower => text::call_text_function(function, arguments),
        PureCalculationFunction::Date
        | PureCalculationFunction::Year
        | PureCalculationFunction::Month
        | PureCalculationFunction::Day => date::call_date_function(function, arguments),
        PureCalculationFunction::Equals
        | PureCalculationFunction::NotEquals
        | PureCalculationFunction::Less
        | PureCalculationFunction::LessOrEqual
        | PureCalculationFunction::Greater
        | PureCalculationFunction::GreaterOrEqual => {
            compare::call_compare_function(function, arguments)
        }
        // If・And・Or・Notは短絡評価のためevaluate_lazyが処理し、ここへは来ない。
        PureCalculationFunction::If
        | PureCalculationFunction::And
        | PureCalculationFunction::Or
        | PureCalculationFunction::Not => Err(CalculationFailure::InvalidFunctionArguments {
            function: function_name(function),
            expected: 0,
            actual: arguments.len(),
        }),
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
                _ => true,
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
        PureCalculationFunction::Round => "Round",
        PureCalculationFunction::Floor => "Floor",
        PureCalculationFunction::Ceil => "Ceil",
        PureCalculationFunction::Mod => "Mod",
        PureCalculationFunction::Len => "Len",
        PureCalculationFunction::Concat => "Concat",
        PureCalculationFunction::Left => "Left",
        PureCalculationFunction::Right => "Right",
        PureCalculationFunction::Mid => "Mid",
        PureCalculationFunction::Trim => "Trim",
        PureCalculationFunction::Upper => "Upper",
        PureCalculationFunction::Lower => "Lower",
        PureCalculationFunction::If => "If",
        PureCalculationFunction::And => "And",
        PureCalculationFunction::Or => "Or",
        PureCalculationFunction::Not => "Not",
        PureCalculationFunction::Equals => "Equals",
        PureCalculationFunction::NotEquals => "NotEquals",
        PureCalculationFunction::Less => "Less",
        PureCalculationFunction::LessOrEqual => "LessOrEqual",
        PureCalculationFunction::Greater => "Greater",
        PureCalculationFunction::GreaterOrEqual => "GreaterOrEqual",
        PureCalculationFunction::Date => "Date",
        PureCalculationFunction::Year => "Year",
        PureCalculationFunction::Month => "Month",
        PureCalculationFunction::Day => "Day",
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
            Self::Boolean(value) => if value { "TRUE" } else { "FALSE" }.to_owned(),
        }
    }
}
