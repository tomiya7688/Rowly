mod ast;
mod parser;
mod runtime;

pub use ast::{
    ClassDefinition, ColumnSelector, ComparisonOperator, Condition, ExecutionEvent,
    ExecutionReport, Expression, FieldDefinition, FunctionDefinition, Program, Statement,
};
pub use parser::ParseError;
pub use runtime::ExecutionError;

use crate::process::CsvDocument;

use thiserror::Error;

pub fn parse(source: &str) -> Result<Program, ParseError> {
    parser::parse(source)
}

pub fn execute(
    program: &Program,
    document: &mut CsvDocument,
) -> Result<ExecutionReport, ExecutionError> {
    runtime::execute(program, document)
}

pub fn run(source: &str, document: &mut CsvDocument) -> Result<ExecutionReport, DslError> {
    let program = parse(source)?;
    Ok(execute(&program, document)?)
}

#[derive(Debug, Error)]
pub enum DslError {
    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error(transparent)]
    Execute(#[from] ExecutionError),
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_classes;
#[cfg(test)]
mod tests_conditions;
