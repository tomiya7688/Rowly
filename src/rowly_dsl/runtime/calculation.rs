//! Calculation Bindingの対象展開と式変換を行う。

use super::*;
use crate::process::calculation::{CalculationCellReference, CalculationRangeReference};
use crate::rowly_dsl::ast::aggregate_call;

// {
//   責務: [resolve_column: 列選択子をdocument内の0-based列indexへ解決する]
//   引数: [selector: 0-based indexまたはheader選択子, document: 列情報の参照元]
//   戻り値: [Result<usize, ColumnError>: document列index]
//   エラー: [ColumnError: indexまたはheaderが解決できない]
// }
// {
//   責務: [expand_calculation_target: calculation targetを既存CSV内の出力cell列へ展開する。]
//   引数: [target: cellまたはrange target。 document: targetの存在検証に使うCSV。]
//   戻り値: [Result<Vec<CellRef>, ExecutionError>: 検証済み出力cell列またはtarget error。]
// }
pub(super) fn expand_calculation_target(
    target: CalculationTarget,
    document: &CsvDocument,
) -> Result<Vec<CellRef>, ExecutionError> {
    let cells = match target {
        CalculationTarget::Cell(cell) => vec![cell],
        CalculationTarget::Range(range) => {
            if document.cell_ref(range.end()).is_none() {
                return Err(CalculationError::MissingTarget(range.end()).into());
            }
            range.iter().collect()
        }
        CalculationTarget::ColumnRelative { .. } => {
            return Err(ExecutionError::InvalidCalculationExpression(
                "column-relative calculation targets are not supported".to_owned(),
            ));
        }
    };
    if let Some(missing) = cells
        .iter()
        .copied()
        .find(|cell| document.cell_ref(*cell).is_none())
    {
        return Err(CalculationError::MissingTarget(missing).into());
    }
    Ok(cells)
}

// {
//   責務: [calculation_target_origin: relative式の基準にするtarget先頭cellを返す。]
//   引数: [target: 計算target。]
//   戻り値: [CellRef: relative offsetの原点。]
// }
pub(super) fn calculation_target_origin(target: CalculationTarget) -> CellRef {
    match target {
        CalculationTarget::Cell(cell) => cell,
        CalculationTarget::Range(range) => range.start(),
        CalculationTarget::ColumnRelative {
            column,
            first_data_row,
        } => CellRef::new(first_data_row, column),
    }
}

pub(super) fn compile_calculation_expression(
    expression: &Expression,
) -> Result<CalculationExpression, ExecutionError> {
    match expression {
        Expression::Literal(value) => {
            if let Ok(integer) = value.parse::<i64>() {
                return Ok(CalculationExpression::Literal(CalculationValue::Integer(
                    integer,
                )));
            }
            let decimal = value.parse::<f64>().map_err(|_| {
                ExecutionError::InvalidCalculationExpression(format!(
                    "literal `{value}` is not numeric"
                ))
            })?;
            if !decimal.is_finite() {
                return Err(ExecutionError::InvalidCalculationExpression(format!(
                    "decimal literal `{value}` is not finite"
                )));
            }
            Ok(CalculationExpression::Literal(CalculationValue::Decimal(
                decimal,
            )))
        }
        Expression::Unary { operand, .. } => Ok(CalculationExpression::Negate(Box::new(
            compile_calculation_expression(operand)?,
        ))),
        Expression::Arithmetic {
            left,
            operator,
            right,
        } => {
            let operator = match operator {
                ArithmeticOperator::Add => CalculationOperator::Add,
                ArithmeticOperator::Subtract => CalculationOperator::Subtract,
                ArithmeticOperator::Multiply => CalculationOperator::Multiply,
                ArithmeticOperator::Divide => CalculationOperator::Divide,
            };
            Ok(CalculationExpression::Arithmetic {
                left: Box::new(compile_calculation_expression(left)?),
                operator,
                right: Box::new(compile_calculation_expression(right)?),
            })
        }
        Expression::Call { name, arguments } if aggregate_call(name, arguments).is_some() => {
            let Some((function, range)) = aggregate_call(name, arguments) else {
                return Err(ExecutionError::InvalidCalculationExpression(
                    "expression contains an unsupported DSL operation".to_owned(),
                ));
            };
            let range = range
                .parse::<CalculationRangeReference>()
                .map_err(|error| ExecutionError::InvalidCalculationExpression(error.to_string()))?;
            Ok(CalculationExpression::Aggregate { function, range })
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("cellvalue") => {
            let text = calculation_literal_argument(arguments, name)?;
            if text.trim() == "#REF!" {
                return Ok(CalculationExpression::DeletedReference);
            }
            let reference = text
                .parse::<CalculationCellReference>()
                .map_err(|error| ExecutionError::InvalidCalculationExpression(error.to_string()))?;
            Ok(if reference.is_relative() {
                CalculationExpression::Cell(reference.cell())
            } else {
                CalculationExpression::AnchoredCell(reference)
            })
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("integer") => {
            let value = calculation_literal_argument(arguments, name)?
                .parse::<i64>()
                .map_err(|_| {
                    ExecutionError::InvalidCalculationExpression(
                        "Integer requires an integer string".to_owned(),
                    )
                })?;
            Ok(CalculationExpression::Literal(CalculationValue::Integer(
                value,
            )))
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("decimal") => {
            let value = calculation_literal_argument(arguments, name)?
                .parse::<f64>()
                .map_err(|_| {
                    ExecutionError::InvalidCalculationExpression(
                        "Decimal requires a finite number string".to_owned(),
                    )
                })?;
            if !value.is_finite() {
                return Err(ExecutionError::InvalidCalculationExpression(
                    "Decimal requires a finite number string".to_owned(),
                ));
            }
            Ok(CalculationExpression::Literal(CalculationValue::Decimal(
                value,
            )))
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("string") => {
            Ok(CalculationExpression::Literal(CalculationValue::Text(
                calculation_literal_argument(arguments, name)?.to_owned(),
            )))
        }
        Expression::Call { name, arguments } if pure_function_by_name(name).is_some() => {
            let Some(function) = pure_function_by_name(name) else {
                return Err(ExecutionError::InvalidCalculationExpression(
                    "expression contains an unsupported DSL operation".to_owned(),
                ));
            };
            let arguments = arguments
                .iter()
                .map(compile_calculation_expression)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(CalculationExpression::Call {
                function,
                arguments,
            })
        }
        _ => Err(ExecutionError::InvalidCalculationExpression(
            "expression contains an unsupported DSL operation".to_owned(),
        )),
    }
}

// {
//   責務: [pure_function_by_name: DSL関数名を許可済みpure functionへ大文字小文字無視で対応付ける]
//   引数: [name: DSL上の関数名]
//   戻り値: [Option<PureCalculationFunction>: 許可済み関数、未対応名はNone]
// }
pub(in crate::rowly_dsl) fn pure_function_by_name(name: &str) -> Option<PureCalculationFunction> {
    match name.to_ascii_lowercase().as_str() {
        "abs" => Some(PureCalculationFunction::Abs),
        "min" => Some(PureCalculationFunction::Min),
        "max" => Some(PureCalculationFunction::Max),
        "round" => Some(PureCalculationFunction::Round),
        "floor" => Some(PureCalculationFunction::Floor),
        "ceil" => Some(PureCalculationFunction::Ceil),
        "mod" => Some(PureCalculationFunction::Mod),
        "len" => Some(PureCalculationFunction::Len),
        "concat" => Some(PureCalculationFunction::Concat),
        "left" => Some(PureCalculationFunction::Left),
        "right" => Some(PureCalculationFunction::Right),
        "mid" => Some(PureCalculationFunction::Mid),
        "trim" => Some(PureCalculationFunction::Trim),
        "upper" => Some(PureCalculationFunction::Upper),
        "lower" => Some(PureCalculationFunction::Lower),
        "date" => Some(PureCalculationFunction::Date),
        "year" => Some(PureCalculationFunction::Year),
        "month" => Some(PureCalculationFunction::Month),
        "day" => Some(PureCalculationFunction::Day),
        _ => None,
    }
}

pub(super) fn calculation_literal_argument<'a>(
    arguments: &'a [Expression],
    function_name: &str,
) -> Result<&'a str, ExecutionError> {
    match arguments {
        [Expression::Literal(value)] => Ok(value),
        _ => Err(ExecutionError::InvalidCalculationExpression(format!(
            "{function_name} requires one literal argument"
        ))),
    }
}
