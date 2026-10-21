//! Calculation Bindingの宣言を解析する。

use super::*;

// {
//   責務: [is_calculation_declaration: SET文が計算規則宣言の対象パスを使うか判定する]
//   引数: [text: 判定するDSL statement]
//   戻り値: [bool: calculation target pathから始まる場合はtrue]
// }
pub(super) fn is_calculation_declaration(text: &str) -> bool {
    strip_prefix_ci(text.trim(), "set ").is_some_and(|rest| {
        starts_with_ci(rest.trim(), "this.worksheet.editor.cell(")
            || starts_with_ci(rest.trim(), "this.worksheet.editor.range(")
    })
}

// {
//   責務: [parse_calculation_declaration: pure expressionを既存cellまたはrangeへ結び付けるSET文を解析する]
//   引数: [line: SET文と元の行番号]
//   戻り値: [Result<Statement, ParseError>: targetと安全なexpressionを持つstatement]
//   エラー: [ParseError: target、演算子、cell参照、関数または式が未対応]
// }
pub(super) fn parse_calculation_declaration(line: &SourceLine) -> Result<Statement, ParseError> {
    let statement = strip_prefix_ci(&line.text, "set ")
        .ok_or_else(|| parse_error(line.number, "expected `SET target = expression`"))?;
    let (target, source_expression) = split_top_level_once(statement, '=')
        .ok_or_else(|| parse_error(line.number, "expected `SET target = expression`"))?;
    let target_text = target.trim();
    let (argument, property, is_range) =
        if starts_with_ci(target_text, "this.worksheet.editor.range(") {
            let (argument, property) =
                parse_call(target_text, "this.worksheet.editor.range(", line.number)?;
            (argument, property, true)
        } else {
            let (argument, property) =
                parse_call(target_text, "this.worksheet.editor.cell(", line.number)?;
            (argument, property, false)
        };
    let address = parse_range_argument(argument, line.number)?;
    let target = if is_range {
        let range = address
            .parse::<CellRange>()
            .map_err(|error| parse_error(line.number, error.to_string()))?;
        CalculationTarget::Range(range)
    } else {
        let cell = address
            .parse::<CellRef>()
            .map_err(|error| parse_error(line.number, error.to_string()))?;
        CalculationTarget::Cell(cell)
    };
    if !eq_ci(property.trim(), ".calculation.expression") {
        if eq_ci(property.trim(), ".calculation.trigger") {
            let trigger_name = parse_scalar(source_expression.trim(), line.number)?;
            let trigger = if trigger_name.eq_ignore_ascii_case("Manual") {
                CalculationTrigger::Manual
            } else if trigger_name.eq_ignore_ascii_case("DependencyChange")
                || trigger_name.eq_ignore_ascii_case("Automatic")
            {
                CalculationTrigger::DependencyChange
            } else {
                return Err(parse_error(
                    line.number,
                    "Calculation.Trigger must be Manual or DependencyChange",
                ));
            };
            return Ok(Statement::SetCalculationTrigger { target, trigger });
        }
        return Err(parse_error(
            line.number,
            "calculation target must end in `.Calculation.Expression` or `.Calculation.Trigger`",
        ));
    }
    let expression = parse_expression(source_expression.trim(), line.number)?;
    validate_calculation_expression(&expression, line.number)?;
    Ok(Statement::SetCalculationExpression { target, expression })
}

// {
//   責務: [validate_calculation_expression: DSL expressionが副作用なし計算式として安全か検証する]
//   引数: [expression: 検証する式AST, line: 元の1始まり行番号]
//   戻り値: [Result<(), ParseError>: 許可式なら成功]
//   エラー: [ParseError: CSV cell read、数値変換、算術、固定pure function以外を含む]
// }
pub(super) fn validate_calculation_expression(
    expression: &Expression,
    line: usize,
) -> Result<(), ParseError> {
    match expression {
        Expression::Literal(value) => {
            let integer_literal = value.parse::<i64>().is_ok();
            let finite_decimal_literal =
                value.parse::<f64>().is_ok_and(|number| number.is_finite());
            if !integer_literal && !finite_decimal_literal {
                return Err(parse_error(
                    line,
                    "calculation literals must be numeric or text",
                ));
            }
        }
        Expression::Unary { operand, .. } => validate_calculation_expression(operand, line)?,
        Expression::Arithmetic { left, right, .. } => {
            validate_calculation_expression(left, line)?;
            validate_calculation_expression(right, line)?;
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("cellvalue") => {
            if arguments.len() != 1 {
                return Err(parse_error(line, "CellValue expects one A1 cell reference"));
            }
            let Some(Expression::Literal(reference)) = arguments.first() else {
                return Err(parse_error(
                    line,
                    "CellValue requires a quoted A1 cell reference",
                ));
            };
            reference
                .parse::<CellRef>()
                .map_err(|error| parse_error(line, error.to_string()))?;
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("integer") => {
            let Some(Expression::Literal(value)) = arguments.first() else {
                return Err(parse_error(
                    line,
                    "Integer calculation literal requires one integer string",
                ));
            };
            if arguments.len() != 1 || value.parse::<i64>().is_err() {
                return Err(parse_error(
                    line,
                    "Integer calculation literal requires one integer string",
                ));
            }
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("decimal") => {
            let Some(Expression::Literal(value)) = arguments.first() else {
                return Err(parse_error(
                    line,
                    "Decimal calculation literal requires one finite number string",
                ));
            };
            if arguments.len() != 1 || !value.parse::<f64>().is_ok_and(|number| number.is_finite())
            {
                return Err(parse_error(
                    line,
                    "Decimal calculation literal requires one finite number string",
                ));
            }
        }
        Expression::Call { name, arguments } if name.eq_ignore_ascii_case("string") => {
            if arguments.len() != 1 || !matches!(arguments.first(), Some(Expression::Literal(_))) {
                return Err(parse_error(
                    line,
                    "String calculation literal requires one text string",
                ));
            }
        }
        Expression::Call { name, arguments }
            if name.eq_ignore_ascii_case("abs")
                || name.eq_ignore_ascii_case("min")
                || name.eq_ignore_ascii_case("max") =>
        {
            let expected = if name.eq_ignore_ascii_case("abs") {
                1
            } else {
                2
            };
            if arguments.len() != expected {
                return Err(parse_error(
                    line,
                    format!("pure function `{name}` expects {expected} arguments"),
                ));
            }
            for argument in arguments {
                validate_calculation_expression(argument, line)?;
            }
        }
        _ => {
            return Err(parse_error(
                line,
                "calculation expression allows only literals, CellValue, arithmetic, Abs, Min, and Max",
            ));
        }
    }
    Ok(())
}
