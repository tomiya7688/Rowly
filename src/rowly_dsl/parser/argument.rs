//! 引数・scalar・selectorを解析する。

use super::*;

// {
//   責務: [parse_column_selector: 列番号または引用符付き見出しを列選択子に変換する]
//   引数: [argument: Column引数, line: 元の1始まり行番号]
//   戻り値: [Result<ColumnSelector, ParseError>: 0始まり内部番号または見出し]
//   エラー: [ParseError: 引数形式が不正、または列番号が1未満]
// }
pub(super) fn parse_column_selector(
    argument: &str,
    line: usize,
) -> Result<ColumnSelector, ParseError> {
    let argument = argument.trim();
    if argument.starts_with('"') {
        return Ok(ColumnSelector::Header(parse_scalar(argument, line)?));
    }
    let one_based = argument.parse::<usize>().map_err(|_| {
        parse_error(
            line,
            "Column(...) expects a 1-based column number or quoted header",
        )
    })?;
    if one_based == 0 {
        return Err(parse_error(line, "column numbers start at 1"));
    }
    Ok(ColumnSelector::Index(one_based - 1))
}

// {
//   責務: [parse_range_argument: Cell引数をA1セルまたは範囲文字列へ変換する]
//   引数: [argument: Cell引数, line: 元の1始まり行番号]
//   戻り値: [Result<String, ParseError>: セル参照文字列]
//   エラー: [ParseError: 空引数または引用文字列が不正]
// }
pub(super) fn parse_range_argument(argument: &str, line: usize) -> Result<String, ParseError> {
    let argument = argument.trim();
    if argument.starts_with('"') {
        return parse_scalar(argument, line);
    }
    if let Some((start, end)) = split_once_ci(argument, " to ") {
        return Ok(format!("{}:{}", start.trim(), end.trim()));
    }
    if argument.is_empty() {
        return Err(parse_error(line, "Cell(...) requires an A1 cell or range"));
    }
    Ok(argument.to_owned())
}

// {
//   責務: [parse_assignment_scalar: 代入右辺の先頭等号を除きスカラー値を解析する]
//   引数: [text: 代入文字列, line: 元の1始まり行番号]
//   戻り値: [Result<String, ParseError>: スカラー値]
//   エラー: [ParseError: 等号または値が不正]
// }
pub(super) fn parse_assignment_scalar(text: &str, line: usize) -> Result<String, ParseError> {
    let value = text
        .trim()
        .strip_prefix('=')
        .ok_or_else(|| parse_error(line, "expected `=`"))?;
    parse_scalar(value.trim(), line)
}

// {
//   責務: [parse_assignment_expression: 代入右辺の先頭等号を除き式を解析する]
//   引数: [text: 代入文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 式AST]
//   エラー: [ParseError: 等号または式が不正]
// }
pub(super) fn parse_assignment_expression(
    text: &str,
    line: usize,
) -> Result<Expression, ParseError> {
    let value = text
        .trim()
        .strip_prefix('=')
        .ok_or_else(|| parse_error(line, "expected `=`"))?;
    parse_expression(value.trim(), line)
}

// {
//   責務: [parse_scalar: スカラー文字列を解析し、引用符内のエスケープを復元する]
//   引数: [text: スカラー文字列, line: 元の1始まり行番号]
//   戻り値: [Result<String, ParseError>: デコード済み文字列]
//   エラー: [ParseError: 空値、閉じない引用符、または後続文字が不正]
// }
pub(super) fn parse_scalar(text: &str, line: usize) -> Result<String, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(parse_error(line, "expected a value"));
    }
    if !text.starts_with('"') {
        return Ok(text.to_owned());
    }
    let mut value = String::new();
    let mut escaped = false;
    let mut closed = false;
    let mut trailing = String::new();
    for ch in text.chars().skip(1) {
        if closed {
            trailing.push(ch);
            continue;
        }
        if escaped {
            value.push(match ch {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '"' => '"',
                '\\' => '\\',
                other => other,
            });
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            closed = true;
            continue;
        }
        value.push(ch);
    }
    if escaped || !closed || !trailing.trim().is_empty() {
        return Err(parse_error(line, "invalid quoted string"));
    }
    Ok(value)
}

// {
//   責務: [split_arguments: 括弧と引用符の入れ子を考慮して引数を分割する]
//   引数: [text: 括弧内引数文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Vec<&str>, ParseError>: 空項目を含まない引数一覧]
//   エラー: [ParseError: 空項目、引用符、または括弧の対応が不正]
// }
pub(super) fn split_arguments(text: &str, line: usize) -> Result<Vec<&str>, ParseError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => return Err(parse_error(line, "unexpected `)` in argument list")),
            ')' => depth -= 1,
            ',' if depth == 0 => {
                let part = text[start..index].trim();
                if part.is_empty() {
                    return Err(parse_error(line, "empty argument"));
                }
                parts.push(part);
                start = index + 1;
            }
            _ => {}
        }
    }
    if quoted {
        return Err(parse_error(line, "unterminated quoted string"));
    }
    if depth != 0 {
        return Err(parse_error(line, "unbalanced parentheses in argument list"));
    }
    let part = text[start..].trim();
    if part.is_empty() {
        return Err(parse_error(line, "empty argument"));
    }
    parts.push(part);
    Ok(parts)
}

// {
//   責務: [parse_column_type: 型名文字列を列型へ変換する]
//   引数: [value: 型名]
//   戻り値: [Option<ColumnType>: String、Integer、Decimal、Booleanのいずれか]
// }
pub(super) fn parse_column_type(value: &str) -> Option<ColumnType> {
    match value.to_ascii_lowercase().as_str() {
        "string" => Some(ColumnType::String),
        "integer" => Some(ColumnType::Integer),
        "decimal" => Some(ColumnType::Decimal),
        "boolean" => Some(ColumnType::Boolean),
        _ => None,
    }
}
