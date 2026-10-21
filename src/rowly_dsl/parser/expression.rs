//! 算術式と関数呼び出し式を解析する。

use super::*;

// {
//   責務: [parse_expression: 入力式を演算子優先順位に従って解析する]
//   引数: [text: 式文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 式AST]
//   エラー: [ParseError: 式の構文が不正]
// }
pub(super) fn parse_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    parse_additive_expression(text.trim(), line)
}

// {
//   責務: [parse_additive_expression: 加算・減算を含む式を解析する]
//   引数: [text: 式文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 加減算ASTまたは次の優先順位の式]
//   エラー: [ParseError: 左辺または右辺が不正]
// }
pub(super) fn parse_additive_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    if let Some((left, operator, right)) = split_top_level_arithmetic(text, &['+', '-']) {
        let operator = match operator {
            '+' => ArithmeticOperator::Add,
            '-' => ArithmeticOperator::Subtract,
            _ => unreachable!(),
        };
        return Ok(Expression::Arithmetic {
            left: Box::new(parse_additive_expression(left.trim(), line)?),
            operator,
            right: Box::new(parse_multiplicative_expression(right.trim(), line)?),
        });
    }
    parse_multiplicative_expression(text, line)
}

// {
//   責務: [parse_multiplicative_expression: 乗算・除算を含む式を解析する]
//   引数: [text: 式文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 乗除算ASTまたは次の優先順位の式]
//   エラー: [ParseError: 左辺または右辺が不正]
// }
pub(super) fn parse_multiplicative_expression(
    text: &str,
    line: usize,
) -> Result<Expression, ParseError> {
    if let Some((left, operator, right)) = split_top_level_arithmetic(text, &['*', '/']) {
        let operator = match operator {
            '*' => ArithmeticOperator::Multiply,
            '/' => ArithmeticOperator::Divide,
            _ => unreachable!(),
        };
        return Ok(Expression::Arithmetic {
            left: Box::new(parse_multiplicative_expression(left.trim(), line)?),
            operator,
            right: Box::new(parse_unary_expression(right.trim(), line)?),
        });
    }
    parse_unary_expression(text, line)
}

// {
//   責務: [parse_unary_expression: 単項マイナスまたは基本式を解析する]
//   引数: [text: 式文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 単項演算ASTまたは基本式]
//   エラー: [ParseError: 単項演算子の後に値がない、または式が不正]
// }
pub(super) fn parse_unary_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if let Some(rest) = text.strip_prefix('-') {
        if rest.trim().is_empty() {
            return Err(parse_error(line, "expected a value after unary '-'"));
        }
        return Ok(Expression::Unary {
            operator: UnaryOperator::Negate,
            operand: Box::new(parse_unary_expression(rest.trim(), line)?),
        });
    }
    parse_primary_expression(text, line)
}

// {
//   責務: [parse_primary_expression: リテラル、変数、呼び出し、メンバー等を解析する]
//   引数: [text: 基本式文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 基本式AST]
//   エラー: [ParseError: 値がない、識別子または呼び出し形式が不正]
// }
pub(super) fn parse_primary_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(parse_error(line, "expected a value"));
    }
    let stripped = strip_outer_parentheses(text);
    if stripped.len() != text.len() {
        return parse_expression(stripped, line);
    }
    if text.starts_with('"') {
        return Ok(Expression::Literal(parse_scalar(text, line)?));
    }
    if let Some(rest) = strip_prefix_ci(text, "new ") {
        let (class_name, arguments) = parse_named_call(rest.trim(), line)?;
        return Ok(Expression::New {
            class_name,
            arguments: parse_argument_expressions(arguments, line)?,
        });
    }
    if let Some((target, member)) = split_member(text) {
        validate_identifier(target, line)?;
        if looks_like_named_call(member) {
            let (name, arguments) = parse_named_call(member, line)?;
            let arguments = parse_argument_expressions(arguments, line)?;
            if let Some(namespace) = parse_standard_namespace(target) {
                return Ok(Expression::StandardCall {
                    namespace,
                    name,
                    arguments,
                });
            }
            return Ok(Expression::MethodCall {
                target: target.to_owned(),
                name,
                arguments,
            });
        }
        validate_identifier(member, line)?;
        return Ok(Expression::Field {
            target: target.to_owned(),
            field: member.to_owned(),
        });
    }
    if looks_like_named_call(text) {
        let (name, arguments) = parse_named_call(text, line)?;
        return Ok(Expression::Call {
            name,
            arguments: parse_argument_expressions(arguments, line)?,
        });
    }
    if eq_ci(text, "true") || eq_ci(text, "false") {
        return Ok(Expression::Literal(text.to_ascii_lowercase()));
    }
    if is_identifier(text) {
        return Ok(Expression::Variable(text.to_owned()));
    }
    Ok(Expression::Literal(text.to_owned()))
}

// {
//   責務: [split_top_level_arithmetic: 括弧と引用符の外にある演算子で式を分割する]
//   引数: [text: 式文字列, operators: 分割対象の演算子]
//   戻り値: [Option<(&str, char, &str)>: 左辺、演算子、右辺]
// }
pub(super) fn split_top_level_arithmetic<'a>(
    text: &'a str,
    operators: &[char],
) -> Option<(&'a str, char, &'a str)> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut candidate = None;

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
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 && operators.contains(&ch) => {
                if (ch == '+' || ch == '-') && is_unary_sign(text, index) {
                    continue;
                }
                candidate = Some((index, ch));
            }
            _ => {}
        }
    }

    candidate.map(|(index, operator)| {
        let right = index + operator.len_utf8();
        (&text[..index], operator, &text[right..])
    })
}

// {
//   責務: [is_unary_sign: 指定位置の符号が単項演算子か判定する]
//   引数: [text: 式文字列, index: 符号のバイト位置]
//   戻り値: [bool: 左辺を持たない符号ならtrue]
// }
pub(super) fn is_unary_sign(text: &str, index: usize) -> bool {
    let before = text[..index].trim_end();
    if before.is_empty() {
        return true;
    }
    matches!(
        before.chars().next_back(),
        Some('(' | ',' | '+' | '-' | '*' | '/' | '=' | '<' | '>')
    )
}

// {
//   責務: [parse_argument_expressions: 引数一覧をそれぞれ式ASTへ変換する]
//   引数: [text: 括弧内の引数文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Vec<Expression>, ParseError>: 引数式一覧]
//   エラー: [ParseError: 引数分割または式解析が不正]
// }
pub(super) fn parse_argument_expressions(
    text: &str,
    line: usize,
) -> Result<Vec<Expression>, ParseError> {
    split_arguments(text, line)?
        .into_iter()
        .map(|argument| parse_expression(argument.trim(), line))
        .collect()
}

// {
//   責務: [parse_call: 指定接頭辞の呼び出しから引数部分と後続文字列を切り出す]
//   引数: [text: 呼び出し文字列, prefix: 必須接頭辞, line: 元の行番号]
//   戻り値: [Result<(&str, &str), ParseError>: 引数部分と閉じ括弧後の文字列]
//   エラー: [ParseError: 接頭辞または閉じ括弧が不正]
// }
pub(super) fn parse_call<'a>(
    text: &'a str,
    prefix: &str,
    line: usize,
) -> Result<(&'a str, &'a str), ParseError> {
    if !starts_with_ci(text, prefix) {
        return Err(parse_error(line, format!("expected `{prefix}...`")));
    }
    let start = prefix.len();
    let close = find_closing_parenthesis(text, start)
        .ok_or_else(|| parse_error(line, "missing closing `)`"))?;
    Ok((&text[start..close], text[close + 1..].trim()))
}

// {
//   責務: [parse_named_call: 名前付き呼び出しから関数名と引数部分を切り出す]
//   引数: [text: 呼び出し文字列, line: 元の1始まり行番号]
//   戻り値: [Result<(String, &str), ParseError>: 関数名と括弧内文字列]
//   エラー: [ParseError: 名前、括弧、または閉じ括弧後の文字が不正]
// }
pub(super) fn parse_named_call(text: &str, line: usize) -> Result<(String, &str), ParseError> {
    let open = text
        .find('(')
        .ok_or_else(|| parse_error(line, "expected call parentheses"))?;
    let name = text[..open].trim();
    validate_identifier(name, line)?;
    let close = find_closing_parenthesis(text, open + 1)
        .ok_or_else(|| parse_error(line, "missing closing `)`"))?;
    if !text[close + 1..].trim().is_empty() {
        return Err(parse_error(line, "unexpected text after call"));
    }
    Ok((name.to_owned(), &text[open + 1..close]))
}

// {
//   責務: [find_closing_parenthesis: 引用符内を除いて対応する閉じ括弧位置を探す]
//   引数: [text: 全体文字列, start: 開き括弧の後の開始バイト位置]
//   戻り値: [Option<usize>: 対応する閉じ括弧のバイト位置]
// }
pub(super) fn find_closing_parenthesis(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
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
            ')' if depth == 0 => return Some(start + offset),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

// {
//   責務: [looks_like_named_call: 識別子名と括弧で構成される呼び出し形か判定する]
//   引数: [text: 判定対象文字列]
//   戻り値: [bool: 呼び出し形ならtrue]
// }
pub(super) fn looks_like_named_call(text: &str) -> bool {
    text.find('(')
        .is_some_and(|open| is_identifier(text[..open].trim()) && text.trim_end().ends_with(')'))
}
