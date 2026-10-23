//! 変数・class宣言と標準namespaceを解析する。

use super::*;

// {
//   責務: [parse_declaration: VARまたはCONST宣言から名前と初期値を解析する]
//   引数: [rest: キーワード後の文字列, kind: 宣言種別, line: 元の行番号]
//   戻り値: [Result<Statement, ParseError>: 宣言文AST]
//   エラー: [ParseError: 区切り、名前、または初期値が不正]
// }
pub(super) fn parse_declaration(
    rest: &str,
    kind: DeclarationKind,
    line: usize,
) -> Result<Statement, ParseError> {
    let (name, value) = split_top_level_once(rest, '=')
        .ok_or_else(|| parse_error(line, "expected `VAR name = value` or `CONST name = value`"))?;
    let name = name.trim();
    validate_binding_name(name, line)?;
    Ok(Statement::Declare {
        kind,
        name: name.to_owned(),
        value: parse_binding_value(value, line)?,
    })
}

// {
//   責務: [parse_binding_value: 宣言・代入の右辺を式として解析する]
//   引数: [text: 右辺, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 式AST]
//   エラー: [ParseError: 値がない、余分な等号、または式が不正]
// }
pub(super) fn parse_binding_value(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if text.starts_with('=') {
        return Err(parse_error(line, "expected a value after a single `=`"));
    }
    parse_expression(text, line)
}

// {
//   責務: [validate_binding_name: 束縛名の識別子形式と予約語使用を検証する]
//   引数: [name: 束縛名, line: 元の1始まり行番号]
//   戻り値: [Result<(), ParseError>: 有効ならOk(())]
//   エラー: [ParseError: 識別子形式が不正、または予約語と重複]
// }
pub(super) fn validate_binding_name(name: &str, line: usize) -> Result<(), ParseError> {
    validate_identifier(name, line)?;
    if [
        "self", "super", "true", "false", "var", "const", "let", "dim", "text", "number", "boolean",
    ]
    .iter()
    .any(|reserved| name.eq_ignore_ascii_case(reserved))
    {
        return Err(parse_error(line, format!("reserved binding name `{name}`")));
    }
    Ok(())
}

// {
//   責務: [parse_standard_namespace: 標準名前空間名を列挙型へ対応付ける]
//   引数: [name: 名前空間名]
//   戻り値: [Option<StandardNamespace>: Text、Number、Booleanのいずれか]
// }
pub(super) fn parse_standard_namespace(name: &str) -> Option<StandardNamespace> {
    if name.eq_ignore_ascii_case("Text") {
        Some(StandardNamespace::Text)
    } else if name.eq_ignore_ascii_case("Number") {
        Some(StandardNamespace::Number)
    } else if name.eq_ignore_ascii_case("Boolean") {
        Some(StandardNamespace::Boolean)
    } else {
        None
    }
}
