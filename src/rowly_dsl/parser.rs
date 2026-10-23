use thiserror::Error;

use crate::process::calculation::{CalculationTarget, CalculationTrigger};

use crate::process::{CellRange, CellRef, ColumnType, ValidationComparisonOperator};

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
            while validation_declaration_needs_continuation(&text) || text.trim_end().ends_with('=')
            {
                let Some(next) = self.lines.get(self.position + 1) else {
                    break;
                };
                text.push(' ');
                text.push_str(&next.text);
                declaration.text = text.clone();
                self.position += 1;
            }
            let statement = if is_calculation_declaration(&declaration.text) {
                parse_calculation_declaration(&declaration)?
            } else {
                parse_validation_statement(&declaration)?
            };
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

mod argument;
mod calculation;
mod condition;
mod declaration;
mod definition;
mod expression;
mod statement;
mod text;
mod validation;

use argument::*;
use calculation::*;
use condition::*;
use declaration::*;
use definition::*;
use expression::*;
use statement::*;
use text::*;
use validation::*;
