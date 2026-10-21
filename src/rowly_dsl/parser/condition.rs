//! if条件と比較式を解析する。

use super::*;

// {
//   責務: [parse_if_condition: If ... Then行から条件式を解析する]
//   引数: [line: Ifヘッダーと元の行番号]
//   戻り値: [Result<Condition, ParseError>: 条件式AST]
//   エラー: [ParseError: If/Then形式または条件式が不正]
// }
pub(super) fn parse_if_condition(line: &SourceLine) -> Result<Condition, ParseError> {
    let lower = line.text.to_ascii_lowercase();
    if !lower.starts_with("if ") || !lower.ends_with(" then") {
        return Err(parse_error(line.number, "expected `If <condition> Then`"));
    }
    parse_condition(line.text[3..line.text.len() - 5].trim(), line.number)
}

// {
//   責務: [parse_condition: 論理演算、比較、列条件を条件式ASTへ変換する]
//   処理: [Or、And、Notの順に分解し、括弧と元の行番号を保つ]
//   引数: [text: 条件式, line: 元の1始まり行番号]
//   戻り値: [Result<Condition, ParseError>: 条件式AST]
//   エラー: [ParseError: 内包する式の構文が不正]
// }
pub(super) fn parse_condition(text: &str, line: usize) -> Result<Condition, ParseError> {
    let text = strip_outer_parentheses(text.trim());
    if let Some((left, right)) = split_keyword_top_level(text, " or ") {
        return Ok(Condition::Or(
            Box::new(parse_condition(left, line)?),
            Box::new(parse_condition(right, line)?),
        ));
    }
    if let Some((left, right)) = split_keyword_top_level(text, " and ") {
        return Ok(Condition::And(
            Box::new(parse_condition(left, line)?),
            Box::new(parse_condition(right, line)?),
        ));
    }
    if let Some(rest) = strip_prefix_ci(text, "not ") {
        return Ok(Condition::Not(Box::new(parse_condition(
            rest.trim(),
            line,
        )?)));
    }
    if starts_with_ci(text, "this.worksheet.column(") {
        let (selector, remainder) = parse_column_target(text, line)?;
        if eq_ci(remainder, ".exists") {
            return Ok(Condition::ColumnExists { selector });
        }
        if let Some(rest) = strip_prefix_ci(remainder, ".title") {
            let value = rest.trim();
            if let Some(value) = value.strip_prefix('=') {
                return Ok(Condition::ColumnTitleEquals {
                    selector,
                    expected: parse_expression(value.trim(), line)?,
                });
            }
        }
    }
    if let Some((left, operator, right)) = split_comparison(text) {
        return Ok(Condition::Compare {
            left: parse_expression(left.trim(), line)?,
            operator,
            right: parse_expression(right.trim(), line)?,
        });
    }
    Ok(Condition::Expression(parse_expression(text, line)?))
}

// {
//   責務: [split_comparison: 最上位の比較演算子で式を分割する]
//   引数: [text: 分割対象の条件式]
//   戻り値: [Option<(&str, ComparisonOperator, &str)>: 左辺、演算子、右辺]
// }
pub(super) fn split_comparison(text: &str) -> Option<(&str, ComparisonOperator, &str)> {
    for (token, operator) in [
        ("!=", ComparisonOperator::NotEqual),
        ("<=", ComparisonOperator::LessOrEqual),
        (">=", ComparisonOperator::GreaterOrEqual),
        ("=", ComparisonOperator::Equal),
        ("<", ComparisonOperator::Less),
        (">", ComparisonOperator::Greater),
    ] {
        if let Some((left, right)) = split_top_level_token(text, token) {
            return Some((left, operator, right));
        }
    }
    None
}
