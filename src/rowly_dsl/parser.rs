use thiserror::Error;

use crate::process::{CellRange, ColumnType, ValidationComparisonOperator};

use super::ast::{
    ArithmeticOperator, ClassDefinition, ColumnSelector, ComparisonOperator, Condition,
    DeclarationKind, Expression, FieldDefinition, FunctionDefinition, Program, StandardNamespace,
    Statement, UnaryOperator, ValidationExpression, ValidationOperand, ValidationRule,
};

/// ```text
/// 責務: [parse: DSL sourceをProgram ASTへparseする]
/// 処理: [Parserが空行とコメント行を除いてblock / statement / expressionをAST化する]
/// 引数: [source: parse対象DSL text]
/// 戻り値: [Program: 検証済みDSL AST]
/// エラー: [ParseError: 構文、識別子、宣言、block構造またはargumentの不正]
/// ```
pub fn parse(source: &str) -> Result<Program, ParseError> {
    Parser::new(source).parse_program()
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Rowly DSL parse error on line {line}: {message}")]
/// ```text
/// 責務: [ParseError: DSL sourceのparse失敗箇所と理由を保持する]
/// フィールド: [line: 1-based source line number, message: parserが報告する失敗理由]
/// ```
pub struct ParseError {
    line: usize,
    message: String,
}

impl ParseError {
    /// parse errorが指す1-based source line numberを返す。
    pub fn line(&self) -> usize {
        self.line
    }

    /// parse errorの説明文を返す。
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Clone)]
// {
//   責務: [SourceLine: parse対象のtrim済みsource lineと元の行番号を対応付ける]
//   フィールド: [number: 1-based source line number, text: trim済みline text]
// }
struct SourceLine {
    number: usize,
    text: String,
}

// {
//   責務: [Parser: filtered source line列をpositionで追跡しながらASTへparseする]
//   フィールド: [lines: 空行 / comment行を除いたsource, position: 次にparseするline index]
// }
struct Parser {
    lines: Vec<SourceLine>,
    position: usize,
}

impl Parser {
    // {
    //   責務: [new: source textをParser stateへ変換する]
    //   処理: [空行、apostrophe comment、REM commentを除き、元の行番号を保持する]
    //   引数: [source: parse対象DSL text]
    //   戻り値: [Parser: 最初のlineから読み始めるparser state]
    // }
    fn new(source: &str) -> Self {
        let lines = source
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                let text = line.trim();
                if text.is_empty() || text.starts_with('\'') || is_rem_comment(text) {
                    None
                } else {
                    Some(SourceLine {
                        number: index + 1,
                        text: text.to_owned(),
                    })
                }
            })
            .collect();
        Self { lines, position: 0 }
    }

    // {
    //   責務: [parse_program: top-level宣言とstatementからProgram ASTを構築する]
    //   処理: [class / function宣言の重複とblock terminatorの位置を検査する]
    //   引数: [self: sourceを消費するparser state]
    //   戻り値: [Program: 宣言群とtop-level statement列]
    //   エラー: [ParseError: 重複宣言、不正terminatorまたはstatement parse失敗]
    // }
    fn parse_program(mut self) -> Result<Program, ParseError> {
        let mut classes = Vec::new();
        let mut functions = Vec::new();
        let mut statements = Vec::new();

        while let Some(line) = self.lines.get(self.position).cloned() {
            if starts_with_ci(&line.text, "class ") {
                let class = self.parse_class(&line)?;
                if classes
                    .iter()
                    .any(|item: &ClassDefinition| item.name.eq_ignore_ascii_case(&class.name))
                {
                    return Err(parse_error(
                        line.number,
                        format!("duplicate class `{}`", class.name),
                    ));
                }
                classes.push(class);
                continue;
            }
            if starts_with_ci(&line.text, "def ") {
                let function = self.parse_function(&line)?;
                if functions
                    .iter()
                    .any(|item: &FunctionDefinition| item.name.eq_ignore_ascii_case(&function.name))
                {
                    return Err(parse_error(
                        line.number,
                        format!("duplicate function `{}`", function.name),
                    ));
                }
                functions.push(function);
                continue;
            }
            if is_terminator(&line.text) || eq_ci(&line.text, "else") {
                return Err(parse_error(line.number, "unexpected block terminator"));
            }
            statements.push(self.parse_statement_or_if()?);
        }

        Ok(Program {
            classes,
            functions,
            statements,
        })
    }

    // {
    //   責務: [parse_class: class header、field、methodからClassDefinitionを構築する]
    //   処理: [親class名とidentifierを検査し、End Classまでbodyをparseする]
    //   引数: [line: class headerと元の行番号]
    //   戻り値: [ClassDefinition: class宣言AST]
    //   エラー: [ParseError: header / body不正、重複field / method、またはEnd Class不足]
    // }
    fn parse_class(&mut self, line: &SourceLine) -> Result<ClassDefinition, ParseError> {
        let header = strip_prefix_ci(&line.text, "class ")
            .map(str::trim)
            .ok_or_else(|| parse_error(line.number, "expected `Class name`"))?;
        let (name, parent) =
            if let Some((name, parent)) = split_keyword_top_level(header, " extends ") {
                let name = name.trim();
                let parent = parent.trim();
                validate_identifier(name, line.number)?;
                validate_identifier(parent, line.number)?;
                (name.to_owned(), Some(parent.to_owned()))
            } else {
                validate_identifier(header, line.number)?;
                (header.to_owned(), None)
            };
        self.position += 1;

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_end_class(&current.text) {
                self.position += 1;
                return Ok(ClassDefinition {
                    name,
                    parent,
                    fields,
                    methods,
                });
            }
            if starts_with_ci(&current.text, "field ") {
                let field = parse_field_definition(&current)?;
                if fields
                    .iter()
                    .any(|item: &FieldDefinition| item.name.eq_ignore_ascii_case(&field.name))
                {
                    return Err(parse_error(
                        current.number,
                        format!("duplicate field `{}`", field.name),
                    ));
                }
                fields.push(field);
                self.position += 1;
                continue;
            }
            if starts_with_ci(&current.text, "def ") {
                let method = self.parse_function(&current)?;
                if methods
                    .iter()
                    .any(|item: &FunctionDefinition| item.name.eq_ignore_ascii_case(&method.name))
                {
                    return Err(parse_error(
                        current.number,
                        format!("duplicate method `{}`", method.name),
                    ));
                }
                methods.push(method);
                continue;
            }
            return Err(parse_error(
                current.number,
                "class bodies only support `Field` and `Def` declarations",
            ));
        }

        Err(parse_error(line.number, "missing `End Class`"))
    }

    // {
    //   責務: [parse_function: function / method bodyをFunctionDefinitionへparseする]
    //   処理: [signatureを検証し、End Defまでstatementをparseする]
    //   引数: [line: Def headerと元の行番号]
    //   戻り値: [FunctionDefinition: name、parameter、bodyを持つAST]
    //   エラー: [ParseError: signature / body不正、unexpected terminatorまたはEnd Def不足]
    // }
    fn parse_function(&mut self, line: &SourceLine) -> Result<FunctionDefinition, ParseError> {
        let (name, parameters) = parse_function_signature(line)?;
        self.position += 1;
        let mut body = Vec::new();
        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_end_def(&current.text) {
                self.position += 1;
                return Ok(FunctionDefinition {
                    name,
                    parameters,
                    body,
                });
            }
            if is_end_class(&current.text)
                || eq_ci(&current.text, "else")
                || is_end_if(&current.text)
                || is_next(&current.text)
            {
                return Err(parse_error(current.number, "unexpected block terminator"));
            }
            body.push(self.parse_statement_or_if()?);
        }
        Err(parse_error(line.number, "missing `End Def`"))
    }

    // {
    //   責務: [parse_statement_or_if: 現在位置からstatementまたはIf blockをparseする]
    //   処理: [block形式と複数行validation declarationを処理し、positionを進める]
    //   引数: [self: 現在のline位置を持つparser state]
    //   戻り値: [Statement: parseしたAST node]
    //   エラー: [ParseError: statement構文、宣言位置またはvalidation構文の失敗]
    // }
    fn parse_statement_or_if(&mut self) -> Result<Statement, ParseError> {
        let line = self
            .lines
            .get(self.position)
            .cloned()
            .expect("parser position is valid");
        if starts_with_ci(&line.text, "if ") {
            return self.parse_if(&line);
        }
        if starts_with_ci(&line.text, "for ") {
            return self.parse_for(&line);
        }
        if starts_with_ci(&line.text, "set ") {
            let mut declaration = line.clone();
            let mut text = line.text.clone();
            while validation_declaration_needs_continuation(&text) {
                let Some(next) = self.lines.get(self.position + 1) else {
                    break;
                };
                text.push(' ');
                text.push_str(&next.text);
                declaration.text = text.clone();
                self.position += 1;
            }
            let statement = parse_validation_statement(&declaration)?;
            self.position += 1;
            return Ok(statement);
        }
        if starts_with_ci(&line.text, "def ") || starts_with_ci(&line.text, "class ") {
            return Err(parse_error(
                line.number,
                "definitions are only allowed at the top level or as class methods",
            ));
        }
        let statement = parse_statement(&line)?;
        self.position += 1;
        Ok(statement)
    }

    // {
    //   責務: [parse_for: For headerとNextまでのbodyからFor statementを構築する]
    //   処理: [loop variable、start / end / step expression、Next名を検証する]
    //   引数: [line: For headerと元の行番号]
    //   戻り値: [Statement::For: loop設定とbody]
    //   エラー: [ParseError: header / body不正、Next名不一致またはNext不足]
    // }
    fn parse_for(&mut self, line: &SourceLine) -> Result<Statement, ParseError> {
        let rest = strip_prefix_ci(&line.text, "for ")
            .ok_or_else(|| parse_error(line.number, "expected `For name = start To end`"))?;
        let (variable, range) = split_top_level_once(rest, '=')
            .ok_or_else(|| parse_error(line.number, "expected `For name = start To end`"))?;
        let variable = variable.trim();
        validate_binding_name(variable, line.number)?;

        let (start_text, end_and_step) = split_keyword_top_level(range.trim(), " to ")
            .ok_or_else(|| parse_error(line.number, "expected `To` in For loop"))?;
        let (end_text, step_text) =
            if let Some((end, step)) = split_keyword_top_level(end_and_step.trim(), " step ") {
                (end.trim(), Some(step.trim()))
            } else {
                (end_and_step.trim(), None)
            };

        let start = parse_expression(start_text.trim(), line.number)?;
        let end = parse_expression(end_text, line.number)?;
        let step = step_text
            .map(|text| parse_expression(text, line.number))
            .transpose()?;

        self.position += 1;
        let mut body = Vec::new();
        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_next(&current.text) {
                if let Some(name) = next_variable(&current.text) {
                    if !name.eq_ignore_ascii_case(variable) {
                        return Err(parse_error(
                            current.number,
                            format!(
                                "Next variable `{name}` does not match For variable `{variable}`"
                            ),
                        ));
                    }
                }
                self.position += 1;
                return Ok(Statement::For {
                    variable: variable.to_owned(),
                    start,
                    end,
                    step,
                    body,
                });
            }
            if is_end_def(&current.text) || is_end_class(&current.text) || is_end_if(&current.text)
            {
                return Err(parse_error(current.number, "unexpected block terminator"));
            }
            body.push(self.parse_statement_or_if()?);
        }

        Err(parse_error(line.number, "missing `Next`"))
    }

    // {
    //   責務: [parse_if: If condition、then body、optional Else bodyを構築する]
    //   処理: [nested statementをEnd Ifまでparseし、Else重複を拒否する]
    //   引数: [line: If headerと元の行番号]
    //   戻り値: [Statement::If: conditionと2つのbody]
    //   エラー: [ParseError: condition / body不正、duplicate Else、またはEnd If不足]
    // }
    fn parse_if(&mut self, line: &SourceLine) -> Result<Statement, ParseError> {
        let condition = parse_if_condition(line)?;
        self.position += 1;
        let mut body = Vec::new();
        let mut else_body = Vec::new();
        let mut in_else = false;

        while let Some(current) = self.lines.get(self.position).cloned() {
            if is_end_if(&current.text) {
                self.position += 1;
                return Ok(Statement::If {
                    condition,
                    body,
                    else_body,
                });
            }
            if eq_ci(&current.text, "else") {
                if in_else {
                    return Err(parse_error(current.number, "duplicate `Else`"));
                }
                in_else = true;
                self.position += 1;
                continue;
            }
            if is_end_def(&current.text) || is_end_class(&current.text) || is_next(&current.text) {
                return Err(parse_error(current.number, "unexpected block terminator"));
            }
            let statement = self.parse_statement_or_if()?;
            if in_else {
                else_body.push(statement);
            } else {
                body.push(statement);
            }
        }

        Err(parse_error(line.number, "missing `End If`"))
    }
}

// {
//   責務: [is_next: Next終端行かを判定する]
//   引数: [text: 判定する行]
//   戻り値: [bool: Next単独またはNext変数の行ならtrue]
// }
fn is_next(text: &str) -> bool {
    eq_ci(text.trim(), "next") || starts_with_ci(text.trim(), "next ")
}

// {
//   責務: [next_variable: Next行から任意のループ変数名を取り出す]
//   引数: [text: Next行]
//   戻り値: [Option<&str>: 空でない変数名。指定がなければNone]
// }
fn next_variable(text: &str) -> Option<&str> {
    strip_prefix_ci(text.trim(), "next ")
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

// {
//   責務: [parse_function_signature: Def行から関数名と引数名を解析する]
//   処理: [引数名を検証し、大文字小文字を無視した重複を拒否する]
//   引数: [line: Def行と元の行番号]
//   戻り値: [Result<(String, Vec<String>), ParseError>: 関数名と引数名一覧]
//   エラー: [ParseError: 署名、引数名、または重複引数が不正]
// }
fn parse_function_signature(line: &SourceLine) -> Result<(String, Vec<String>), ParseError> {
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
fn parse_field_definition(line: &SourceLine) -> Result<FieldDefinition, ParseError> {
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

// {
//   責務: [parse_if_condition: If ... Then行から条件式を解析する]
//   引数: [line: Ifヘッダーと元の行番号]
//   戻り値: [Result<Condition, ParseError>: 条件式AST]
//   エラー: [ParseError: If/Then形式または条件式が不正]
// }
fn parse_if_condition(line: &SourceLine) -> Result<Condition, ParseError> {
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
fn parse_condition(text: &str, line: usize) -> Result<Condition, ParseError> {
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
fn split_comparison(text: &str) -> Option<(&str, ComparisonOperator, &str)> {
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

// {
//   責務: [parse_statement: 1行の宣言、代入、呼び出し等を解析する]
//   引数: [line: 文のソース行と元の行番号]
//   戻り値: [Result<Statement, ParseError>: 文AST]
//   エラー: [ParseError: 未対応文または文の構文が不正]
// }
fn parse_statement(line: &SourceLine) -> Result<Statement, ParseError> {
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
//   責務: [parse_validation_statement: SET行から列検証ルールを解析する]
//   処理: [AllowedValuesのリテラル一覧またはValue参照式を検証する]
//   引数: [line: SET文と元の行番号]
//   戻り値: [Result<Statement, ParseError>: 列選択子と検証ルールを持つ文]
//   エラー: [ParseError: 対象、ルール形式、または式が不正]
// }
fn parse_validation_statement(line: &SourceLine) -> Result<Statement, ParseError> {
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
fn parse_validation_values(text: &str, line: usize) -> Result<Vec<String>, ParseError> {
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
fn parse_validation_value(text: &str, line: usize) -> Result<String, ParseError> {
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
fn compile_validation_expression(
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
fn compile_validation_operand(
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
fn validation_declaration_needs_continuation(text: &str) -> bool {
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
fn has_unclosed_square_bracket(text: &str) -> bool {
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

// {
//   責務: [parse_declaration: VARまたはCONST宣言から名前と初期値を解析する]
//   引数: [rest: キーワード後の文字列, kind: 宣言種別, line: 元の行番号]
//   戻り値: [Result<Statement, ParseError>: 宣言文AST]
//   エラー: [ParseError: 区切り、名前、または初期値が不正]
// }
fn parse_declaration(
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
fn parse_binding_value(text: &str, line: usize) -> Result<Expression, ParseError> {
    let text = text.trim();
    if text.starts_with('=') {
        return Err(parse_error(line, "expected a value after a single `=`"));
    }
    parse_expression(text, line)
}

// {
//   責務: [strip_keyword: 大文字小文字を無視して先頭キーワードを取り除く]
//   引数: [text: 入力文字列, keyword: キーワード]
//   戻り値: [Option<&str>: 単語境界が一致した場合の残り文字列]
// }
fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = strip_prefix_ci(text, keyword)?;
    if rest.is_empty() || rest.starts_with(char::is_whitespace) {
        Some(rest.trim_start())
    } else {
        None
    }
}

// {
//   責務: [validate_binding_name: 束縛名の識別子形式と予約語使用を検証する]
//   引数: [name: 束縛名, line: 元の1始まり行番号]
//   戻り値: [Result<(), ParseError>: 有効ならOk(())]
//   エラー: [ParseError: 識別子形式が不正、または予約語と重複]
// }
fn validate_binding_name(name: &str, line: usize) -> Result<(), ParseError> {
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
fn parse_standard_namespace(name: &str) -> Option<StandardNamespace> {
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

// {
//   責務: [parse_return_statement: Return行を戻り値あり・なしの文へ変換する]
//   引数: [line: Return文と元の行番号]
//   戻り値: [Result<Statement, ParseError>: Return文AST]
//   エラー: [ParseError: 戻り値の式が不正]
// }
fn parse_return_statement(line: &SourceLine) -> Result<Statement, ParseError> {
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
fn parse_field_assignment(line: &SourceLine) -> Result<Option<Statement>, ParseError> {
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
fn parse_column_statement(line: &SourceLine) -> Result<Statement, ParseError> {
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
fn parse_cell_statement(line: &SourceLine) -> Result<Statement, ParseError> {
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
fn parse_column_target(text: &str, line: usize) -> Result<(ColumnSelector, &str), ParseError> {
    let (argument, remainder) = parse_call(text, "this.worksheet.column(", line)?;
    Ok((parse_column_selector(argument, line)?, remainder))
}

// {
//   責務: [parse_expression: 入力式を演算子優先順位に従って解析する]
//   引数: [text: 式文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 式AST]
//   エラー: [ParseError: 式の構文が不正]
// }
fn parse_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
    parse_additive_expression(text.trim(), line)
}

// {
//   責務: [parse_additive_expression: 加算・減算を含む式を解析する]
//   引数: [text: 式文字列, line: 元の1始まり行番号]
//   戻り値: [Result<Expression, ParseError>: 加減算ASTまたは次の優先順位の式]
//   エラー: [ParseError: 左辺または右辺が不正]
// }
fn parse_additive_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
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
fn parse_multiplicative_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
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
fn parse_unary_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
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
fn parse_primary_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
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
fn split_top_level_arithmetic<'a>(
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
fn is_unary_sign(text: &str, index: usize) -> bool {
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
fn parse_argument_expressions(text: &str, line: usize) -> Result<Vec<Expression>, ParseError> {
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
fn parse_call<'a>(
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
fn parse_named_call(text: &str, line: usize) -> Result<(String, &str), ParseError> {
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
fn find_closing_parenthesis(text: &str, start: usize) -> Option<usize> {
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
//   責務: [parse_column_selector: 列番号または引用符付き見出しを列選択子に変換する]
//   引数: [argument: Column引数, line: 元の1始まり行番号]
//   戻り値: [Result<ColumnSelector, ParseError>: 0始まり内部番号または見出し]
//   エラー: [ParseError: 引数形式が不正、または列番号が1未満]
// }
fn parse_column_selector(argument: &str, line: usize) -> Result<ColumnSelector, ParseError> {
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
fn parse_range_argument(argument: &str, line: usize) -> Result<String, ParseError> {
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
fn parse_assignment_scalar(text: &str, line: usize) -> Result<String, ParseError> {
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
fn parse_assignment_expression(text: &str, line: usize) -> Result<Expression, ParseError> {
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
fn parse_scalar(text: &str, line: usize) -> Result<String, ParseError> {
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
fn split_arguments(text: &str, line: usize) -> Result<Vec<&str>, ParseError> {
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
//   責務: [split_member: 最上位のドットで対象名とメンバー名を分割する]
//   引数: [text: メンバー参照文字列]
//   戻り値: [Option<(&str, &str)>: 対象とメンバー]
// }
fn split_member(text: &str) -> Option<(&str, &str)> {
    split_top_level_token(text, ".")
}

// {
//   責務: [split_top_level_once: 最上位にある指定文字の最初の位置で分割する]
//   引数: [text: 入力文字列, target: 分割文字]
//   戻り値: [Option<(&str, &str)>: 左側と右側]
// }
fn split_top_level_once(text: &str, target: char) -> Option<(&str, &str)> {
    let token = target.to_string();
    split_top_level_token(text, &token)
}

// {
//   責務: [split_top_level_token: 括弧・引用符の外の最初のトークンで分割する]
//   引数: [text: 入力文字列, token: 分割トークン]
//   戻り値: [Option<(&str, &str)>: トークン前後の部分文字列]
// }
fn split_top_level_token<'a>(text: &'a str, token: &str) -> Option<(&'a str, &'a str)> {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index + token.len() <= text.len() {
        let ch = text[index..].chars().next()?;
        if escaped {
            escaped = false;
            index += ch.len_utf8();
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            index += 1;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            index += 1;
            continue;
        }
        if !quoted {
            match ch {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                _ => {}
            }
            if depth == 0 && bytes[index..].starts_with(token.as_bytes()) {
                return Some((&text[..index], &text[index + token.len()..]));
            }
        }
        index += ch.len_utf8();
    }
    None
}

// {
//   責務: [split_keyword_top_level: 大文字小文字を無視し最上位キーワードで分割する]
//   引数: [text: 入力文字列, keyword: 分割キーワード]
//   戻り値: [Option<(&str, &str)>: キーワード前後の部分文字列]
// }
fn split_keyword_top_level<'a>(text: &'a str, keyword: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let keyword_lower = keyword.to_ascii_lowercase();
    let mut search_from = 0usize;
    while let Some(relative) = lower[search_from..].find(&keyword_lower) {
        let index = search_from + relative;
        if is_top_level_index(text, index) {
            return Some((&text[..index], &text[index + keyword.len()..]));
        }
        search_from = index + 1;
    }
    None
}

// {
//   責務: [is_top_level_index: 指定バイト位置が括弧・引用符の外か判定する]
//   引数: [text: 入力文字列, target: 判定位置]
//   戻り値: [bool: 最上位かつ引用符外ならtrue]
// }
fn is_top_level_index(text: &str, target: usize) -> bool {
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if index >= target {
            break;
        }
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
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth == 0 && !quoted
}

// {
//   責務: [strip_outer_parentheses: 式全体を囲う対応済み括弧を繰り返し取り除く]
//   引数: [text: 式文字列]
//   戻り値: [&str: 外側括弧を除いたトリム済み式]
// }
fn strip_outer_parentheses(mut text: &str) -> &str {
    loop {
        let trimmed = text.trim();
        if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return trimmed;
        }
        if find_closing_parenthesis(trimmed, 1) != Some(trimmed.len() - 1) {
            return trimmed;
        }
        text = &trimmed[1..trimmed.len() - 1];
    }
}

// {
//   責務: [parse_column_type: 型名文字列を列型へ変換する]
//   引数: [value: 型名]
//   戻り値: [Option<ColumnType>: String、Integer、Decimal、Booleanのいずれか]
// }
fn parse_column_type(value: &str) -> Option<ColumnType> {
    match value.to_ascii_lowercase().as_str() {
        "string" => Some(ColumnType::String),
        "integer" => Some(ColumnType::Integer),
        "decimal" => Some(ColumnType::Decimal),
        "boolean" => Some(ColumnType::Boolean),
        _ => None,
    }
}

// {
//   責務: [looks_like_named_call: 識別子名と括弧で構成される呼び出し形か判定する]
//   引数: [text: 判定対象文字列]
//   戻り値: [bool: 呼び出し形ならtrue]
// }
fn looks_like_named_call(text: &str) -> bool {
    text.find('(')
        .is_some_and(|open| is_identifier(text[..open].trim()) && text.trim_end().ends_with(')'))
}

// {
//   責務: [validate_identifier: 識別子形式を検証し不正時に行番号付きエラーを返す]
//   引数: [identifier: 識別子, line: 元の1始まり行番号]
//   戻り値: [Result<(), ParseError>: 有効ならOk(())]
//   エラー: [ParseError: 識別子形式が不正]
// }
fn validate_identifier(identifier: &str, line: usize) -> Result<(), ParseError> {
    if is_identifier(identifier) {
        Ok(())
    } else {
        Err(parse_error(
            line,
            format!("invalid identifier `{identifier}`"),
        ))
    }
}

// {
//   責務: [is_identifier: 先頭が英字または下線の識別子形式を判定する]
//   引数: [identifier: 判定対象文字列]
//   戻り値: [bool: 以降も英数字または下線ならtrue]
// }
fn is_identifier(identifier: &str) -> bool {
    let mut chars = identifier.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic()) && chars.all(|ch| ch == '_' || ch.is_alphanumeric())
}

// {
//   責務: [is_terminator: End If、End Def、End Classのいずれかを判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: ブロック終端ならtrue]
// }
fn is_terminator(text: &str) -> bool {
    is_end_if(text) || is_end_def(text) || is_end_class(text)
}

// {
//   責務: [is_end_if: End IfまたはEndIf表記を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: Ifブロック終端ならtrue]
// }
fn is_end_if(text: &str) -> bool {
    eq_ci(text, "end if") || eq_ci(text, "endif")
}

// {
//   責務: [is_end_def: End DefまたはEndDef表記を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: 関数ブロック終端ならtrue]
// }
fn is_end_def(text: &str) -> bool {
    eq_ci(text, "end def") || eq_ci(text, "enddef")
}

// {
//   責務: [is_end_class: End ClassまたはEndClass表記を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: クラスブロック終端ならtrue]
// }
fn is_end_class(text: &str) -> bool {
    eq_ci(text, "end class") || eq_ci(text, "endclass")
}

// {
//   責務: [is_rem_comment: REM単独またはREMで始まる行を判定する]
//   引数: [text: ソース行]
//   戻り値: [bool: REMコメント行ならtrue]
// }
fn is_rem_comment(text: &str) -> bool {
    eq_ci(text, "rem") || starts_with_ci(text, "rem ")
}

// {
//   責務: [starts_with_ci: 大文字小文字を無視して接頭辞一致を判定する]
//   引数: [text: 入力文字列, prefix: 接頭辞]
//   戻り値: [bool: 接頭辞が一致すればtrue]
// }
fn starts_with_ci(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|value| value.eq_ignore_ascii_case(prefix))
}

// {
//   責務: [strip_prefix_ci: 大文字小文字を無視して一致した接頭辞を除去する]
//   引数: [text: 入力文字列, prefix: 接頭辞]
//   戻り値: [Option<&str>: 一致した場合の残り文字列]
// }
fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    starts_with_ci(text, prefix).then(|| &text[prefix.len()..])
}

// {
//   責務: [eq_ci: 2つの文字列を大文字小文字を無視して比較する]
//   引数: [left: 左辺文字列, right: 右辺文字列]
//   戻り値: [bool: ASCII大小文字を無視して等しければtrue]
// }
fn eq_ci(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

// {
//   責務: [split_once_ci: 最初の区切り文字列で大小文字を無視して分割する]
//   引数: [text: 入力文字列, separator: 区切り文字列]
//   戻り値: [Option<(&str, &str)>: 区切り前後の部分文字列]
// }
fn split_once_ci<'a>(text: &'a str, separator: &str) -> Option<(&'a str, &'a str)> {
    let lower = text.to_ascii_lowercase();
    let index = lower.find(&separator.to_ascii_lowercase())?;
    Some((&text[..index], &text[index + separator.len()..]))
}

// {
//   責務: [parse_error: 行番号とメッセージから解析エラーを作成する]
//   引数: [line: 元の1始まり行番号, message: エラーメッセージ]
//   戻り値: [ParseError: 行番号と本文を保持したエラー]
// }
fn parse_error(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}
