use std::collections::HashMap;

use crate::process::{CellRange, ColumnType, ColumnTypeReport, JapaneseCheckReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub(super) classes: Vec<ClassDefinition>,
    pub(super) functions: Vec<FunctionDefinition>,
    pub(super) statements: Vec<Statement>,
}

impl Program {
    pub fn classes(&self) -> &[ClassDefinition] {
        &self.classes
    }

    pub fn functions(&self) -> &[FunctionDefinition] {
        &self.functions
    }

    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassDefinition {
    pub(super) name: String,
    pub(super) fields: Vec<FieldDefinition>,
    pub(super) methods: Vec<FunctionDefinition>,
}

impl ClassDefinition {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fields(&self) -> &[FieldDefinition] {
        &self.fields
    }

    pub fn methods(&self) -> &[FunctionDefinition] {
        &self.methods
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinition {
    pub(super) name: String,
    pub(super) default: Expression,
}

impl FieldDefinition {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn default(&self) -> &Expression {
        &self.default
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
        else_body: Vec<Statement>,
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
    MethodCall {
        target: String,
        name: String,
        arguments: Vec<Expression>,
    },
    SetField {
        target: String,
        field: String,
        value: Expression,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expression {
    Literal(String),
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Arithmetic {
        left: Box<Expression>,
        operator: ArithmeticOperator,
        right: Box<Expression>,
    },
    Variable(String),
    Call {
        name: String,
        arguments: Vec<Expression>,
    },
    New {
        class_name: String,
    },
    Field {
        target: String,
        field: String,
    },
    MethodCall {
        target: String,
        name: String,
        arguments: Vec<Expression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
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
    Compare {
        left: Expression,
        operator: ComparisonOperator,
        right: Expression,
    },
    Expression(Expression),
    Not(Box<Condition>),
    And(Box<Condition>, Box<Condition>),
    Or(Box<Condition>, Box<Condition>),
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
    pub(super) object_fields: HashMap<String, HashMap<String, String>>,
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

    pub fn object_field(&self, variable: &str, field: &str) -> Option<&str> {
        self.object_fields
            .get(&normalize_identifier(variable))?
            .get(&normalize_identifier(field))
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
    ObjectCreated {
        class_name: String,
    },
    FieldSet {
        target: String,
        field: String,
        value: String,
    },
    FunctionCalled {
        name: String,
        arguments: Vec<String>,
        return_value: Option<String>,
    },
    MethodCalled {
        class_name: String,
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
