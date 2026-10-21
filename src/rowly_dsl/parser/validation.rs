//! 入力規則の宣言を解析する。

use super::*;

// {
//   責務: [parse_validation_statement: SET行から列検証ルールを解析する]
//   処理: [AllowedValuesのリテラル一覧またはValue参照式を検証する]
//   引数: [line: SET文と元の行番号]
//   戻り値: [Result<Statement, ParseError>: 列選択子と検証ルールを持つ文]
//   エラー: [ParseError: 対象、ルール形式、または式が不正]
// }
pub(super) fn parse_validation_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    let rest = strip_prefix_ci(&line.text, "set ")
        .ok_or_else(|| parse_error(line.number, "expected `SET target = value`"))?;
    let (target, value) = split_top_level_once(rest, '=')
        .ok_or_else(|| parse_error(line.number, "expected `SET target = value`"))?;
    if !starts_with_ci(target.trim(), "this.worksheet.editor.column(") {
        return Err(parse_error(
            line.number,
            "SET target must start with `This.Worksheet.Editor.Column(...)`",
        ));
    }
    let (selector, remainder) =
        parse_call(target.trim(), "this.worksheet.editor.column(", line.number)?;
    let selector = parse_column_selector(selector, line.number)?;
    let value = value.trim();
    let rule = if eq_ci(remainder, ".validation.allowedvalues") {
        ValidationRule::AllowedValues(parse_validation_values(value, line.number)?)
    } else if eq_ci(remainder, ".validation.expression") {
        let condition = parse_condition(value, line.number)?;
        let mut uses_value = false;
        let expression = compile_validation_expression(&condition, &mut uses_value, line.number)?;
        if !uses_value {
            return Err(parse_error(
                line.number,
                "Validation.Expression must reference the candidate `Value`",
            ));
        }
        ValidationRule::Expression(expression)
    } else {
        return Err(parse_error(
            line.number,
            "SET target must end in `.Validation.AllowedValues` or `.Validation.Expression`",
        ));
    };
    Ok(Statement::SetValidationRule { selector, rule })
}

// {
//   責務: [parse_validation_values: AllowedValuesの角括弧リストを解析する]
//   引数: [text: リスト, line: 元の1始まり行番号]
//   戻り値: [Result<Vec<String>, ParseError>: リテラル値一覧]
//   エラー: [ParseError: リストまたは項目が不正、引用符が閉じていない]
// }
pub(super) fn parse_validation_values(text: &str, line: usize) -> Result<Vec<String>, ParseError> {
    let text = text.trim();
    if !text.starts_with('[') || !text.ends_with(']') {
        return Err(parse_error(
            line,
            "Validation.AllowedValues expects a bracketed list",
        ));
    }
    let contents = &text[1..text.len() - 1];
    if contents.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut values = Vec::new();
    let mut start = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in contents.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if !quoted {
            if matches!(ch, '[' | ']' | '(' | ')') {
                return Err(parse_error(
                    line,
                    "AllowedValues items must be literal values",
                ));
            }
            if ch == ',' {
                values.push(parse_validation_value(&contents[start..index], line)?);
                start = index + ch.len_utf8();
            }
        }
    }
    if quoted || escaped {
        return Err(parse_error(line, "unterminated string in AllowedValues"));
    }
    values.push(parse_validation_value(&contents[start..], line)?);
    Ok(values)
}

// {
//   責務: [parse_validation_value: 検証リストの項目を単一リテラルとして解析する]
//   引数: [text: リスト項目, line: 元の1始まり行番号]
//   戻り値: [Result<String, ParseError>: リテラル値]
//   エラー: [ParseError: 項目がリテラルでない、または式が不正]
// }
pub(super) fn parse_validation_value(text: &str, line: usize) -> Result<String, ParseError> {
    match parse_expression(text.trim(), line)? {
        Expression::Literal(value) => Ok(value),
        _ => Err(parse_error(
            line,
            "AllowedValues items must be literal values",
        )),
    }
}

// {
//   責務: [compile_validation_expression: 条件ASTを検証式ASTへ変換する]
//   処理: [各比較が候補Valueを参照することを確認する]
//   引数: [condition: 条件AST, uses_value: Value参照の累積結果, line: 元の行番号]
//   戻り値: [Result<ValidationExpression, ParseError>: 検証式AST]
//   エラー: [ParseError: 比較形式またはValue参照要件を満たさない]
// }
pub(super) fn compile_validation_expression(
    condition: &Condition,
    uses_value: &mut bool,
    line: usize,
) -> Result<ValidationExpression, ParseError> {
    match condition {
        Condition::Compare {
            left,
            operator,
            right,
        } => {
            let mut comparison_uses_value = false;
            let left = compile_validation_operand(left, &mut comparison_uses_value, line)?;
            let right = compile_validation_operand(right, &mut comparison_uses_value, line)?;
            if !comparison_uses_value {
                return Err(parse_error(
                    line,
                    "each Validation.Expression comparison must reference the candidate `Value`",
                ));
            }
            *uses_value = true;
            Ok(ValidationExpression::Compare {
                left,
                operator: match operator {
                    ComparisonOperator::Equal => ValidationComparisonOperator::Equal,
                    ComparisonOperator::NotEqual => ValidationComparisonOperator::NotEqual,
                    ComparisonOperator::Less => ValidationComparisonOperator::Less,
                    ComparisonOperator::LessOrEqual => ValidationComparisonOperator::LessOrEqual,
                    ComparisonOperator::Greater => ValidationComparisonOperator::Greater,
                    ComparisonOperator::GreaterOrEqual => {
                        ValidationComparisonOperator::GreaterOrEqual
                    }
                },
                right,
            })
        }
        Condition::Not(inner) => Ok(ValidationExpression::Not(Box::new(
            compile_validation_expression(inner, uses_value, line)?,
        ))),
        Condition::And(left, right) => Ok(ValidationExpression::And(
            Box::new(compile_validation_expression(left, uses_value, line)?),
            Box::new(compile_validation_expression(right, uses_value, line)?),
        )),
        Condition::Or(left, right) => Ok(ValidationExpression::Or(
            Box::new(compile_validation_expression(left, uses_value, line)?),
            Box::new(compile_validation_expression(right, uses_value, line)?),
        )),
        _ => Err(parse_error(
            line,
            "Validation.Expression must be a Boolean comparison using `Value` and string literals",
        )),
    }
}

// {
//   責務: [compile_validation_operand: 検証比較のオペランドを許可型へ変換する]
//   引数: [expression: 式AST, uses_value: Value参照の記録先, line: 元の行番号]
//   戻り値: [Result<ValidationOperand, ParseError>: Valueまたは文字列リテラル]
//   エラー: [ParseError: 関数呼び出しや他の変数など未対応オペランド]
// }
pub(super) fn compile_validation_operand(
    expression: &Expression,
    uses_value: &mut bool,
    line: usize,
) -> Result<ValidationOperand, ParseError> {
    match expression {
        Expression::Variable(name) if name.eq_ignore_ascii_case("Value") => {
            *uses_value = true;
            Ok(ValidationOperand::Value)
        }
        Expression::Literal(value) => Ok(ValidationOperand::Literal(value.clone())),
        _ => Err(parse_error(
            line,
            "Validation.Expression operands must be `Value` or string literals; calls and variables are unsupported",
        )),
    }
}

// {
//   責務: [validation_declaration_needs_continuation: 複数行検証宣言の続きが必要か判定する]
//   引数: [text: 宣言の代入部分]
//   戻り値: [bool: リストまたは式が未完了ならtrue]
// }
pub(super) fn validation_declaration_needs_continuation(text: &str) -> bool {
    let Some((target, value)) = split_top_level_once(text, '=') else {
        return false;
    };
    let target = target.trim().to_ascii_lowercase();
    let value = value.trim();
    if target.ends_with(".validation.allowedvalues") {
        return value.is_empty() || has_unclosed_square_bracket(value);
    }
    if target.ends_with(".validation.expression") {
        if value.is_empty() {
            return true;
        }
        let lower = value.to_ascii_lowercase();
        return [" and", " or", " =", " !=", " <=", " >=", " <", " >"]
            .iter()
            .any(|suffix| lower.ends_with(suffix));
    }
    false
}

// {
//   責務: [has_unclosed_square_bracket: 文字列リテラル外の未閉鎖角括弧を検出する]
//   引数: [text: 検査対象の式]
//   戻り値: [bool: 開き括弧が残っていればtrue]
// }
pub(super) fn has_unclosed_square_bracket(text: &str) -> bool {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for ch in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if !quoted {
            match ch {
                '[' => depth += 1,
                ']' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    depth > 0
}
