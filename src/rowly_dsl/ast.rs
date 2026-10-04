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
    pub(super) parent: Option<String>,
    pub(super) fields: Vec<FieldDefinition>,
    pub(super) methods: Vec<FunctionDefinition>,
}

impl ClassDefinition {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
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

/// VAR は再代入可能、CONST は束縛の再代入を禁止する。
/// オブジェクトのフィールドを再帰的に凍結する指定ではない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationKind {
    Var,
    Const,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StandardNamespace {
    Text,
    Number,
    Boolean,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Statement {
    If {
        condition: Condition,
        body: Vec<Statement>,
        else_body: Vec<Statement>,
    },
    For {
        variable: String,
        start: Expression,
        end: Expression,
        step: Option<Expression>,
        body: Vec<Statement>,
    },
    Declare {
        kind: DeclarationKind,
        name: String,
        value: Expression,
    },
    Assign {
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
    SetValidationRule {
        selector: ColumnSelector,
        rule: ValidationRule,
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
    StandardCall {
        namespace: StandardNamespace,
        name: String,
        arguments: Vec<Expression>,
    },
    New {
        class_name: String,
        arguments: Vec<Expression>,
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

/// A pure validation rule declared by a DSL `SET` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationRule {
    AllowedValues(Vec<String>),
    Expression(ValidationExpression),
}

impl ValidationRule {
    /// Returns whether a candidate text value satisfies this rule.
    pub fn matches(&self, candidate: &str) -> bool {
        match self {
            Self::AllowedValues(values) => values.iter().any(|value| value == candidate),
            Self::Expression(expression) => expression.evaluate(candidate),
        }
    }

    pub fn allowed_values(&self) -> Option<&[String]> {
        match self {
            Self::AllowedValues(values) => Some(values),
            Self::Expression(_) => None,
        }
    }

    pub fn expression(&self) -> Option<&ValidationExpression> {
        match self {
            Self::AllowedValues(_) => None,
            Self::Expression(expression) => Some(expression),
        }
    }
}

/// Boolean expression over the candidate `Value` and string literals only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationExpression {
    Compare {
        left: ValidationOperand,
        operator: ComparisonOperator,
        right: ValidationOperand,
    },
    Not(Box<ValidationExpression>),
    And(Box<ValidationExpression>, Box<ValidationExpression>),
    Or(Box<ValidationExpression>, Box<ValidationExpression>),
}

impl ValidationExpression {
    pub fn evaluate(&self, candidate: &str) -> bool {
        match self {
            Self::Compare {
                left,
                operator,
                right,
            } => {
                let left = left.value(candidate);
                let right = right.value(candidate);
                let ordering = left.cmp(right);
                match operator {
                    ComparisonOperator::Equal => ordering.is_eq(),
                    ComparisonOperator::NotEqual => !ordering.is_eq(),
                    ComparisonOperator::Less => ordering.is_lt(),
                    ComparisonOperator::LessOrEqual => !ordering.is_gt(),
                    ComparisonOperator::Greater => ordering.is_gt(),
                    ComparisonOperator::GreaterOrEqual => !ordering.is_lt(),
                }
            }
            Self::Not(inner) => !inner.evaluate(candidate),
            Self::And(left, right) => left.evaluate(candidate) && right.evaluate(candidate),
            Self::Or(left, right) => left.evaluate(candidate) || right.evaluate(candidate),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOperand {
    Value,
    Literal(String),
}

impl ValidationOperand {
    fn value<'a>(&'a self, candidate: &'a str) -> &'a str {
        match self {
            Self::Value => candidate,
            Self::Literal(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationRuleDefinition {
    pub(super) selector: ColumnSelector,
    pub(super) rule: ValidationRule,
}

impl ValidationRuleDefinition {
    pub fn selector(&self) -> &ColumnSelector {
        &self.selector
    }

    pub fn rule(&self) -> &ValidationRule {
        &self.rule
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub(super) events: Vec<ExecutionEvent>,
    pub(super) variables: HashMap<String, String>,
    pub(super) object_fields: HashMap<String, HashMap<String, String>>,
    pub(super) validation_rules: Vec<ValidationRuleDefinition>,
}

impl ExecutionReport {
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    /// Validation declarations in source execution order.
    pub fn validation_rules(&self) -> &[ValidationRuleDefinition] {
        &self.validation_rules
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
    ValidationRuleSet {
        definition: ValidationRuleDefinition,
    },
}

pub(super) fn normalize_identifier(identifier: &str) -> String {
    identifier.to_ascii_lowercase()
}
