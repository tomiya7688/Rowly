//! ```text
//! 責務: [rowly_dsl: Rowly DSLのpublic APIとparser / AST / runtimeのmodule境界を提供する]
//! 処理: [sourceをProgramへparseし、ProgramをCsvDocument上でexecuteする]
//! 補足: [parserが構文とASTを作り、runtimeがprocess API経由でdocumentを操作する]
//! ```
mod ast;
mod parser;
mod runtime;

pub use crate::process::{ValidationComparisonOperator, ValidationTarget};
pub use ast::{
    ArithmeticOperator, ClassDefinition, ColumnSelector, ComparisonOperator, Condition,
    DeclarationKind, ExecutionEvent, ExecutionReport, Expression, FieldDefinition,
    FunctionDefinition, Program, StandardNamespace, Statement, UnaryOperator, ValidationExpression,
    ValidationOperand, ValidationRuleDefinition,
};
pub use parser::ParseError;
pub use runtime::ExecutionError;

use crate::process::CsvDocument;

use thiserror::Error;

/// ```text
/// 責務: [parse: Rowly DSL sourceをProgram ASTへparseする]
/// 引数: [source: parse対象のDSL source]
/// 戻り値: [Program: 実行可能なAST]
/// エラー: [ParseError: sourceの構文または宣言検証に失敗した]
/// ```
pub fn parse(source: &str) -> Result<Program, ParseError> {
    parser::parse(source)
}

/// ```text
/// 責務: [execute: Program ASTをCsvDocument上で実行する]
/// 処理: [runtimeへASTとdocumentを渡してstatementを評価する]
/// 引数: [program: 実行するAST, document: DSL commandの対象document]
/// 戻り値: [ExecutionReport: 実行eventと最終runtime valueのsnapshot]
/// 副作用: [documentへのDSL編集を適用する]
/// エラー: [ExecutionError: runtimeまたはprocess commandが失敗した]
/// ```
pub fn execute(
    program: &Program,
    document: &mut CsvDocument,
) -> Result<ExecutionReport, ExecutionError> {
    runtime::execute(program, document)
}

/// ```text
/// 責務: [run: DSL sourceのparseとexecuteを続けて行う]
/// 引数: [source: 実行するDSL source, document: DSL commandの対象document]
/// 戻り値: [ExecutionReport: 実行結果のsnapshot]
/// 副作用: [documentへのDSL編集を適用する]
/// エラー: [DslError: parseまたはruntime実行の失敗]
/// ```
pub fn run(source: &str, document: &mut CsvDocument) -> Result<ExecutionReport, DslError> {
    let program = parse(source)?;
    Ok(execute(&program, document)?)
}

#[derive(Debug, Error)]
/// ```text
/// 責務: [DslError: parse / executeをまとめたDSL実行APIのerror]
/// 補足: [元のParseErrorまたはExecutionErrorをvariantで保持する]
/// ```
pub enum DslError {
    /// sourceからASTを作れなかった。
    #[error(transparent)]
    Parse(#[from] ParseError),

    /// ASTのruntime実行に失敗した。
    #[error(transparent)]
    Execute(#[from] ExecutionError),
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_arithmetic;
#[cfg(test)]
mod tests_bindings;
#[cfg(test)]
mod tests_cell_value;
#[cfg(test)]
mod tests_classes;
#[cfg(test)]
mod tests_conditions;
#[cfg(test)]
mod tests_for_loop;
#[cfg(test)]
mod tests_header_access;
#[cfg(test)]
mod tests_predicates;
#[cfg(test)]
mod tests_typed_values;
#[cfg(test)]
mod tests_validation_rules;
