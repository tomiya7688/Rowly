//! 関数・class定義の見出しを解析する。

use super::*;

// {
//   責務: [parse_function_signature: Def行から関数名と引数名を解析する]
//   処理: [引数名を検証し、大文字小文字を無視した重複を拒否する]
//   引数: [line: Def行と元の行番号]
//   戻り値: [Result<(String, Vec<String>), ParseError>: 関数名と引数名一覧]
//   エラー: [ParseError: 署名、引数名、または重複引数が不正]
// }
pub(super) fn parse_function_signature(
    line: &SourceLine,
) -> Result<(String, Vec<String>), ParseError> {
    let signature = strip_prefix_ci(&line.text, "def ")
        .ok_or_else(|| parse_error(line.number, "expected `Def name(...)`"))?;
    let (name, arguments) = parse_named_call(signature.trim(), line.number)?;
    let parameters = split_arguments(arguments, line.number)?
        .into_iter()
        .map(|item| {
            let item = item.trim();
            validate_binding_name(item, line.number)?;
            Ok(item.to_owned())
        })
        .collect::<Result<Vec<_>, ParseError>>()?;
    for (index, parameter) in parameters.iter().enumerate() {
        if parameters[..index]
            .iter()
            .any(|item| item.eq_ignore_ascii_case(parameter))
        {
            return Err(parse_error(
                line.number,
                format!("duplicate parameter `{parameter}`"),
            ));
        }
    }
    Ok((name, parameters))
}

// {
//   責務: [parse_field_definition: Field行からフィールド定義を作る]
//   引数: [line: Field行と元の行番号]
//   戻り値: [Result<FieldDefinition, ParseError>: 名前と初期値式]
//   エラー: [ParseError: 名前、区切り、または初期値式が不正]
// }
pub(super) fn parse_field_definition(line: &SourceLine) -> Result<FieldDefinition, ParseError> {
    let rest = strip_prefix_ci(&line.text, "field ")
        .ok_or_else(|| parse_error(line.number, "expected `Field name = value`"))?;
    let (name, value) = split_top_level_once(rest, '=')
        .ok_or_else(|| parse_error(line.number, "expected `Field name = value`"))?;
    let name = name.trim();
    validate_identifier(name, line.number)?;
    Ok(FieldDefinition {
        name: name.to_owned(),
        default: parse_expression(value.trim(), line.number)?,
    })
}
