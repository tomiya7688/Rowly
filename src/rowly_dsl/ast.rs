use std::collections::HashMap;

use crate::process::{CellRange, ColumnType, ColumnTypeReport, JapaneseCheckReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub(super) functions: Vec<FunctionDefinition>,
    pub(super) statements: Vec<Statement>,
}

impl Program {
    pub fn functions(&self) -> &[FunctionDefinition] {
        &self.functions
    }

    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionDefinition {
    pub(super) name: String,
    pub(super) parameters: Vec<String>,
    pub(super) body: Vec<Statement>,
}

impl FunctionDefinition {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn parameters(&self) -> &[String] {
        &self.parameters
    }

    pub fn body(&self) -> &[Statement] {
        &self.body
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    If {
        condition: Condition,
        body: Vec<Statement>,
    },
    Let {
        name: String,
        value: Expression,
    },
    Return {
        value: Option<Expression>,
    },
    Call {
        name: String,
        arguments: Vec<Expression>,
    },
    SetRangeValue {
        range: CellRange,
        value: Expression,
    },
    ValidateColumnType {
        selector: ColumnSelector,
        column_type: ColumnType,
    },
    CheckJapanese {
        selector: ColumnSelector,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression {
    Literal(String),
    Variable(String),
    Call {
        name: String,
        arguments: Vec<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    ColumnExists {
        selector: ColumnSelector,
    },
    ColumnTitleEquals {
        selector: ColumnSelector,
        expected: Expression,
    },
    ValueEquals {
        left: Expression,
        right: Expression,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnSelector {
    Index(usize),
    Header(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub(super) events: Vec<ExecutionEvent>,
    pub(super) variables: HashMap<String, String>,
}

impl ExecutionReport {
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables
            .get(&normalize_identifier(name))
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionEvent {
    ConditionEvaluated {
        condition: Condition,
        result: bool,
    },
    VariableSet {
        name: String,
        value: String,
    },
    FunctionCalled {
        name: String,
        arguments: Vec<String>,
        return_value: Option<String>,
    },
    RangeValueSet {
        range: CellRange,
        value: String,
    },
    ColumnTypeChecked {
        selector: ColumnSelector,
        report: ColumnTypeReport,
    },
    JapaneseChecked {
        selector: ColumnSelector,
        report: JapaneseCheckReport,
    },
}

pub(super) fn normalize_identifier(identifier: &str) -> String {
    identifier.to_ascii_lowercase()
}
