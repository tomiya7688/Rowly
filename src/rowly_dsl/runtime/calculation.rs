//! Calculation Bindingの対象展開と式変換を行う。

use super::*;

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
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("cellvalue") => {
            let reference = calculation_literal_argument(arguments, name)?
                .parse::<CellRef>()
                .map_err(|error| ExecutionError::InvalidCalculationExpression(error.to_string()))?;
            Ok(CalculationExpression::Cell(reference))
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
        Expression::Call { name, arguments }
            if name.eq_ignore_ascii_case("abs")
                || name.eq_ignore_ascii_case("min")
                || name.eq_ignore_ascii_case("max") =>
        {
            let function = if name.eq_ignore_ascii_case("abs") {
                PureCalculationFunction::Abs
            } else if name.eq_ignore_ascii_case("min") {
                PureCalculationFunction::Min
            } else {
                PureCalculationFunction::Max
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
