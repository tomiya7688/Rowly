//! 通常文とcell・column文を解析する。

use super::*;

// {
//   責務: [parse_statement: 1行の宣言、代入、呼び出し等を解析する]
//   引数: [line: 文のソース行と元の行番号]
//   戻り値: [Result<Statement, ParseError>: 文AST]
//   エラー: [ParseError: 未対応文または文の構文が不正]
// }
pub(super) fn parse_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    if let Some(rest) = strip_keyword(&line.text, "var") {
        return parse_declaration(rest, DeclarationKind::Var, line.number);
    }
    if let Some(rest) = strip_keyword(&line.text, "const") {
        return parse_declaration(rest, DeclarationKind::Const, line.number);
    }
    if strip_keyword(&line.text, "let").is_some() || strip_keyword(&line.text, "dim").is_some() {
        return Err(parse_error(
            line.number,
            "LET / DIM are not supported; use VAR or CONST",
        ));
    }
    if eq_ci(&line.text, "return") || starts_with_ci(&line.text, "return ") {
        return parse_return_statement(line);
    }
    if starts_with_ci(&line.text, "this.worksheet.column(") {
        return parse_column_statement(line);
    }
    if starts_with_ci(&line.text, "this.worksheet.editor.cell(") {
        return parse_cell_statement(line);
    }
    if let Some((name, value)) = split_top_level_once(&line.text, '=') {
        let name = name.trim();
        if is_identifier(name) {
            validate_binding_name(name, line.number)?;
            return Ok(Statement::Assign {
                name: name.to_owned(),
                value: parse_binding_value(value, line.number)?,
            });
        }
    }
    if let Some(statement) = parse_field_assignment(line)? {
        return Ok(statement);
    }
    match parse_expression(&line.text, line.number)? {
        Expression::Call { name, arguments } => Ok(Statement::Call { name, arguments }),
        Expression::MethodCall {
            target,
            name,
            arguments,
        } => Ok(Statement::MethodCall {
            target,
            name,
            arguments,
        }),
        _ => Err(parse_error(line.number, "unsupported statement")),
    }
}

// {
//   責務: [parse_return_statement: Return行を戻り値あり・なしの文へ変換する]
//   引数: [line: Return文と元の行番号]
//   戻り値: [Result<Statement, ParseError>: Return文AST]
//   エラー: [ParseError: 戻り値の式が不正]
// }
pub(super) fn parse_return_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    if eq_ci(&line.text, "return") {
        return Ok(Statement::Return { value: None });
    }
    let rest = strip_prefix_ci(&line.text, "return ").unwrap();
    Ok(Statement::Return {
        value: Some(parse_expression(rest.trim(), line.number)?),
    })
}

// {
//   責務: [parse_field_assignment: メンバー代入行をフィールド設定文として解析する]
//   引数: [line: 代入候補と元の行番号]
//   戻り値: [Result<Option<Statement>, ParseError>: 対象なら設定文、対象外ならNone]
//   エラー: [ParseError: 識別子または右辺式が不正]
// }
pub(super) fn parse_field_assignment(line: &SourceLine) -> Result<Option<Statement>, ParseError> {
    let Some((left, right)) = split_top_level_once(&line.text, '=') else {
        return Ok(None);
    };
    let Some((target, field)) = split_member(left.trim()) else {
        return Ok(None);
    };
    validate_identifier(target, line.number)?;
    validate_identifier(field, line.number)?;
    Ok(Some(Statement::SetField {
        target: target.to_owned(),
        field: field.to_owned(),
        value: parse_expression(right.trim(), line.number)?,
    }))
}

// {
//   責務: [parse_column_statement: 列型設定または日本語チェック文を解析する]
//   引数: [line: 列文と元の行番号]
//   戻り値: [Result<Statement, ParseError>: 列操作文AST]
//   エラー: [ParseError: 列選択子、型名、または操作形式が不正]
// }
pub(super) fn parse_column_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    let (selector, remainder) = parse_column_target(&line.text, line.number)?;
    if let Some(rest) = strip_prefix_ci(remainder, ".type") {
        let value = parse_assignment_scalar(rest, line.number)?;
        let column_type = parse_column_type(&value).ok_or_else(|| {
            parse_error(
                line.number,
                "column type must be String, Integer, Decimal, or Boolean",
            )
        })?;
        return Ok(Statement::ValidateColumnType {
            selector,
            column_type,
        });
    }
    if eq_ci(remainder, ".check.japanese") {
        return Ok(Statement::CheckJapanese { selector });
    }
    Err(parse_error(
        line.number,
        "supported column statements are `.Type = ...` and `.Check.Japanese`",
    ))
}

// {
//   責務: [parse_cell_statement: Cell(...).Value.Set代入を範囲設定文へ変換する]
//   引数: [line: セル代入文と元の行番号]
//   戻り値: [Result<Statement, ParseError>: セル範囲設定文AST]
//   エラー: [ParseError: 対象範囲、代入形式、または値が不正]
// }
pub(super) fn parse_cell_statement(line: &SourceLine) -> Result<Statement, ParseError> {
    let (argument, remainder) = parse_call(&line.text, "this.worksheet.editor.cell(", line.number)?;
    if !starts_with_ci(remainder, ".value.set") {
        return Err(parse_error(
            line.number,
            "expected `.Value.Set = <value>` after Cell(...) ",
        ));
    }
    let value = parse_assignment_expression(&remainder[".value.set".len()..], line.number)?;
    let range = parse_range_argument(argument, line.number)?
        .parse::<CellRange>()
        .map_err(|error| parse_error(line.number, error.to_string()))?;
    Ok(Statement::SetRangeValue { range, value })
}

// {
//   責務: [parse_column_target: Column(...)対象を選択子と後続メンバーに分ける]
//   引数: [text: 列対象文字列, line: 元の1始まり行番号]
//   戻り値: [Result<(ColumnSelector, &str), ParseError>: 列選択子と残り文字列]
//   エラー: [ParseError: 呼び出しまたは列選択子が不正]
// }
pub(super) fn parse_column_target(
    text: &str,
    line: usize,
) -> Result<(ColumnSelector, &str), ParseError> {
    let (argument, remainder) = parse_call(text, "this.worksheet.column(", line)?;
    Ok((parse_column_selector(argument, line)?, remainder))
}
