use super::CellRef;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValidationTarget {
    Index(usize),
    Header(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationOperand {
    Value,
    Literal(String),
}

impl ValidationOperand {
    fn resolve<'a>(&'a self, candidate: &'a str) -> &'a str {
        match self {
            Self::Value => candidate,
            Self::Literal(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationExpression {
    Compare {
        left: ValidationOperand,
        operator: ValidationComparisonOperator,
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
                let ordering = left.resolve(candidate).cmp(right.resolve(candidate));
                match operator {
                    ValidationComparisonOperator::Equal => ordering.is_eq(),
                    ValidationComparisonOperator::NotEqual => !ordering.is_eq(),
                    ValidationComparisonOperator::Less => ordering.is_lt(),
                    ValidationComparisonOperator::LessOrEqual => !ordering.is_gt(),
                    ValidationComparisonOperator::Greater => ordering.is_gt(),
                    ValidationComparisonOperator::GreaterOrEqual => !ordering.is_lt(),
                }
            }
            Self::Not(inner) => !inner.evaluate(candidate),
            Self::And(left, right) => left.evaluate(candidate) && right.evaluate(candidate),
            Self::Or(left, right) => left.evaluate(candidate) || right.evaluate(candidate),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationRule {
    AllowedValues(Vec<String>),
    Expression(ValidationExpression),
}

impl ValidationRule {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationViolation {
    pub(super) cell: CellRef,
    pub(super) target: ValidationTarget,
    pub(super) value: String,
    pub(super) rule: ValidationRule,
}

impl ValidationViolation {
    pub fn cell(&self) -> CellRef {
        self.cell
    }

    pub fn target(&self) -> &ValidationTarget {
        &self.target
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn rule(&self) -> &ValidationRule {
        &self.rule
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationReport {
    pub(super) checked_cells: usize,
    pub(super) violations: Vec<ValidationViolation>,
}

impl ValidationReport {
    pub fn checked_cells(&self) -> usize {
        self.checked_cells
    }

    pub fn violations(&self) -> &[ValidationViolation] {
        &self.violations
    }

    pub fn is_valid(&self) -> bool {
        self.violations.is_empty()
    }
}
