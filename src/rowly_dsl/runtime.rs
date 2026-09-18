use std::cmp::Ordering;
use std::collections::HashMap;

use thiserror::Error;

use crate::process::{ColumnError, CsvDocument, DocumentError};

use super::ast::{
    ArithmeticOperator, ColumnSelector, ComparisonOperator, Condition, ExecutionEvent,
    ExecutionReport, Expression, Program, Statement, UnaryOperator, normalize_identifier,
};

const MAX_CALL_DEPTH: usize = 64;
type ObjectId = usize;

#[derive(Debug, Clone)]
enum Value {
    Text(String),
    Integer(i64),
    Decimal(f64),
    Boolean(bool),
    Object(ObjectId),
}

#[derive(Debug, Clone)]
struct ObjectInstance {
    class_name: String,
    fields: HashMap<String, Value>,
}

pub fn execute(
    program: &Program,
    document: &mut CsvDocument,
) -> Result<ExecutionReport, ExecutionError> {
    Runtime::new(program, document).execute()
}

#[derive(Debug, Error)]
pub enum ExecutionError {
    #[error(transparent)]
    Column(#[from] ColumnError),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error("unknown Rowly DSL variable `{0}`")]
    UnknownVariable(String),
    #[error("unknown Rowly DSL function `{0}`")]
    UnknownFunction(String),
    #[error("unknown Rowly DSL class `{0}`")]
    UnknownClass(String),
    #[error("cell `{0}` is outside the existing CSV table")]
    MissingCell(String),
    #[error("unknown field `{field}` on class `{class_name}`")]
    UnknownField { class_name: String, field: String },
    #[error("unknown method `{method}` on class `{class_name}`")]
    UnknownMethod { class_name: String, method: String },
    #[error("`{0}` is not an object")]
    ExpectedObject(String),
    #[error("{context} requires a text value")]
    ExpectedText { context: String },
    #[error("{context} requires a Boolean value")]
    ExpectedBoolean { context: String },
    #[error("cannot convert {value_type} value `{value}` to {target}")]
    Conversion {
        value_type: &'static str,
        value: String,
        target: &'static str,
    },
    #[error("cannot compare {left_type} and {right_type} with `{operator}`")]
    IncomparableValues {
        left_type: &'static str,
        right_type: &'static str,
        operator: &'static str,
    },
    #[error("cannot apply arithmetic operator `{operator}` to {left_type} and {right_type}")]
    IncomparableArithmetic {
        left_type: &'static str,
        right_type: &'static str,
        operator: &'static str,
    },
    #[error("cannot apply unary operator `{operator}` to {value_type}")]
    InvalidUnaryArithmetic {
        value_type: &'static str,
        operator: &'static str,
    },
    #[error("division by zero")]
    DivisionByZero,
    #[error("arithmetic overflow while applying `{operator}`")]
    ArithmeticOverflow { operator: &'static str },
    #[error("function `{name}` expects {expected} arguments but received {actual}")]
    ArgumentCount {
        name: String,
        expected: usize,
        actual: usize,
    },
    #[error("method `{class_name}.{name}` expects {expected} arguments but received {actual}")]
    MethodArgumentCount {
        class_name: String,
        name: String,
        expected: usize,
        actual: usize,
    },
    #[error("call `{0}` was used as a value but did not return one")]
    MissingReturnValue(String),
    #[error("{context} requires an Integer value")]
    ExpectedInteger { context: String },
    #[error("{context} must be at least 1")]
    InvalidOneBasedIndex { context: String },
    #[error("For loop Step cannot be zero")]
    ZeroLoopStep,
    #[error("For loop counter overflowed")]
    LoopCounterOverflow,
    #[error("`Return` can only be used inside a function or method")]
    ReturnOutsideFunction,
    #[error("Rowly DSL call depth exceeded the limit of {limit}")]
    CallDepthExceeded { limit: usize },
}

#[derive(Debug, Clone)]
enum Flow {
    Continue,
    Return(Option<Value>),
}

struct Runtime<'a> {
    program: &'a Program,
    document: &'a mut CsvDocument,
    scopes: Vec<HashMap<String, Value>>,
    objects: Vec<ObjectInstance>,
    events: Vec<ExecutionEvent>,
    call_depth: usize,
}

impl<'a> Runtime<'a> {
    fn new(program: &'a Program, document: &'a mut CsvDocument) -> Self {
        Self {
            program,
            document,
            scopes: vec![HashMap::new()],
            objects: Vec::new(),
            events: Vec::new(),
            call_depth: 0,
        }
    }

    fn execute(mut self) -> Result<ExecutionReport, ExecutionError> {
        let statements = self.program.statements.clone();
        if !matches!(self.execute_statements(&statements)?, Flow::Continue) {
            return Err(ExecutionError::ReturnOutsideFunction);
        }

        let global = self.scopes.pop().unwrap_or_default();
        let mut variables = HashMap::new();
        let mut object_fields = HashMap::new();
        for (name, value) in global {
            match value {
                Value::Object(object_id) => {
                    let fields = self
                        .objects
                        .get(object_id)
                        .map(|object| {
                            object
                                .fields
                                .iter()
                                .filter_map(|(field, value)| {
                                    self.scalar_text(value).map(|value| (field.clone(), value))
                                })
                                .collect::<HashMap<_, _>>()
                        })
                        .unwrap_or_default();
                    object_fields.insert(name, fields);
                }
                value => {
                    if let Some(value) = self.scalar_text(&value) {
                        variables.insert(name, value);
                    }
                }
            }
        }

        Ok(ExecutionReport {
            events: self.events,
            variables,
            object_fields,
        })
    }

    fn execute_statements(&mut self, statements: &[Statement]) -> Result<Flow, ExecutionError> {
        for statement in statements {
            match statement {
                Statement::If {
                    condition,
                    body,
                    else_body,
                } => {
                    let result = self.evaluate_condition(condition)?;
                    self.events.push(ExecutionEvent::ConditionEvaluated {
                        condition: condition.clone(),
                        result,
                    });
                    let branch = if result { body } else { else_body };
                    let flow = self.execute_statements(branch)?;
                    if !matches!(flow, Flow::Continue) {
                        return Ok(flow);
                    }
                }
                Statement::For {
                    variable,
                    start,
                    end,
                    step,
                    body,
                } => {
                    let start = self.evaluate_expression(start)?;
                    let end = self.evaluate_expression(end)?;
                    let step = step
                        .as_ref()
                        .map(|expression| self.evaluate_expression(expression))
                        .transpose()?;
                    let mut current = self.expect_integer(start, "For start")?;
                    let end = self.expect_integer(end, "For end")?;
                    let step = match step {
                        Some(value) => self.expect_integer(value, "For Step")?,
                        None => 1,
                    };
                    if step == 0 {
                        return Err(ExecutionError::ZeroLoopStep);
                    }

                    self.scopes.push(HashMap::new());
                    let execution = (|| -> Result<Flow, ExecutionError> {
                        loop {
                            let in_range = if step > 0 {
                                current <= end
                            } else {
                                current >= end
                            };
                            if !in_range {
                                break;
                            }

                            self.current_scope_mut()
                                .insert(normalize_identifier(variable), Value::Integer(current));
                            let flow = self.execute_statements(body)?;
                            if !matches!(flow, Flow::Continue) {
                                return Ok(flow);
                            }

                            current = current
                                .checked_add(step)
                                .ok_or(ExecutionError::LoopCounterOverflow)?;
                        }
                        Ok(Flow::Continue)
                    })();
                    self.scopes.pop();
                    let flow = execution?;
                    if !matches!(flow, Flow::Continue) {
                        return Ok(flow);
                    }
                }
                Statement::Let { name, value } => {
                    let value = self.evaluate_expression(value)?;
                    let event_value = self.describe_value(&value);
                    self.current_scope_mut()
                        .insert(normalize_identifier(name), value);
                    self.events.push(ExecutionEvent::VariableSet {
                        name: name.clone(),
                        value: event_value,
                    });
                }
                Statement::Return { value } => {
                    let value = value
                        .as_ref()
                        .map(|expression| self.evaluate_expression(expression))
                        .transpose()?;
                    return Ok(Flow::Return(value));
                }
                Statement::Call { name, arguments } => {
                    if self.call_builtin(name, arguments)?.is_none() {
                        let _ = self.call_function(name, arguments)?;
                    }
                }
                Statement::MethodCall {
                    target,
                    name,
                    arguments,
                } => {
                    let object_id = self.resolve_object(target)?;
                    let _ = self.call_method(object_id, name, arguments)?;
                }
                Statement::SetField {
                    target,
                    field,
                    value,
                } => {
                    let value = self.evaluate_expression(value)?;
                    let object_id = self.resolve_object(target)?;
                    self.set_field(object_id, field, value.clone())?;
                    self.events.push(ExecutionEvent::FieldSet {
                        target: target.clone(),
                        field: field.clone(),
                        value: self.describe_value(&value),
                    });
                }
                Statement::SetRangeValue { range, value } => {
                    let value = self.evaluate_expression(value)?;
                    let value = self.cell_text(value)?;
                    self.document.set_range_value(*range, value.clone())?;
                    self.events.push(ExecutionEvent::RangeValueSet {
                        range: *range,
                        value,
                    });
                }
                Statement::ValidateColumnType {
                    selector,
                    column_type,
                } => {
                    let column = resolve_column(selector, self.document)?;
                    let report = self.document.validate_column_type(column, *column_type)?;
                    self.events.push(ExecutionEvent::ColumnTypeChecked {
                        selector: selector.clone(),
                        report,
                    });
                }
                Statement::CheckJapanese { selector } => {
                    let column = resolve_column(selector, self.document)?;
                    let report = self.document.check_column_japanese(column)?;
                    self.events.push(ExecutionEvent::JapaneseChecked {
                        selector: selector.clone(),
                        report,
                    });
                }
            }
        }
        Ok(Flow::Continue)
    }

    fn evaluate_condition(&mut self, condition: &Condition) -> Result<bool, ExecutionError> {
        match condition {
            Condition::ColumnExists { selector } => Ok(match selector {
                ColumnSelector::Index(column) => *column < self.document.column_count(),
                ColumnSelector::Header(header) => {
                    !self.document.column_indices_by_header(header).is_empty()
                }
            }),
            Condition::ColumnTitleEquals { selector, expected } => {
                let column = resolve_column(selector, self.document)?;
                let expected = self.evaluate_expression(expected)?;
                let expected = self.expect_text(expected, "column title comparison")?;
                Ok(self.document.cell(0, column) == Some(expected.as_str()))
            }
            Condition::Compare {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate_expression(left)?;
                let right = self.evaluate_expression(right)?;
                self.compare_values(left, *operator, right)
            }
            Condition::Expression(expression) => {
                let value = self.evaluate_expression(expression)?;
                self.expect_boolean(value, "condition")
            }
            Condition::Not(inner) => Ok(!self.evaluate_condition(inner)?),
            Condition::And(left, right) => {
                if !self.evaluate_condition(left)? {
                    return Ok(false);
                }
                self.evaluate_condition(right)
            }
            Condition::Or(left, right) => {
                if self.evaluate_condition(left)? {
                    return Ok(true);
                }
                self.evaluate_condition(right)
            }
        }
    }

    fn compare_values(
        &self,
        left: Value,
        operator: ComparisonOperator,
        right: Value,
    ) -> Result<bool, ExecutionError> {
        use ComparisonOperator::{Equal, Greater, GreaterOrEqual, Less, LessOrEqual, NotEqual};

        if matches!(operator, Equal | NotEqual) {
            let equal = match (&left, &right) {
                (Value::Text(left), Value::Text(right)) => left == right,
                (Value::Integer(left), Value::Integer(right)) => left == right,
                (Value::Decimal(left), Value::Decimal(right)) => left == right,
                (Value::Integer(left), Value::Decimal(right)) => (*left as f64) == *right,
                (Value::Decimal(left), Value::Integer(right)) => *left == (*right as f64),
                (Value::Boolean(left), Value::Boolean(right)) => left == right,
                (Value::Object(left), Value::Object(right)) => left == right,
                _ => false,
            };
            return Ok(if matches!(operator, Equal) {
                equal
            } else {
                !equal
            });
        }

        let ordering = match (&left, &right) {
            (Value::Text(left), Value::Text(right)) => left.cmp(right),
            (Value::Integer(left), Value::Integer(right)) => left.cmp(right),
            (Value::Integer(left), Value::Decimal(right)) => compare_f64(*left as f64, *right),
            (Value::Decimal(left), Value::Integer(right)) => compare_f64(*left, *right as f64),
            (Value::Decimal(left), Value::Decimal(right)) => compare_f64(*left, *right),
            _ => {
                return Err(ExecutionError::IncomparableValues {
                    left_type: value_type(&left),
                    right_type: value_type(&right),
                    operator: comparison_operator_text(operator),
                });
            }
        };

        Ok(match operator {
            Less => ordering == Ordering::Less,
            LessOrEqual => ordering != Ordering::Greater,
            Greater => ordering == Ordering::Greater,
            GreaterOrEqual => ordering != Ordering::Less,
            Equal | NotEqual => unreachable!(),
        })
    }

    fn evaluate_expression(&mut self, expression: &Expression) -> Result<Value, ExecutionError> {
        match expression {
            Expression::Literal(value) => Ok(Value::Text(value.clone())),
            Expression::Unary { operator, operand } => {
                let value = self.evaluate_expression(operand)?;
                self.evaluate_unary(*operator, value)
            }
            Expression::Arithmetic {
                left,
                operator,
                right,
            } => {
                let left = self.evaluate_expression(left)?;
                let right = self.evaluate_expression(right)?;
                self.evaluate_arithmetic(left, *operator, right)
            }
            Expression::Variable(name) => self
                .lookup_variable(name)
                .cloned()
                .ok_or_else(|| ExecutionError::UnknownVariable(name.clone())),
            Expression::Call { name, arguments } => {
                if let Some(value) = self.call_builtin(name, arguments)? {
                    return Ok(value);
                }
                self.call_function(name, arguments)?
                    .ok_or_else(|| ExecutionError::MissingReturnValue(name.clone()))
            }
            Expression::New { class_name } => self.instantiate_class(class_name),
            Expression::Field { target, field } => {
                let object_id = self.resolve_object(target)?;
                self.get_field(object_id, field)
            }
            Expression::MethodCall {
                target,
                name,
                arguments,
            } => {
                let object_id = self.resolve_object(target)?;
                self.call_method(object_id, name, arguments)?
                    .ok_or_else(|| ExecutionError::MissingReturnValue(format!("{target}.{name}")))
            }
        }
    }

    fn evaluate_unary(
        &self,
        operator: UnaryOperator,
        value: Value,
    ) -> Result<Value, ExecutionError> {
        match operator {
            UnaryOperator::Negate => match value {
                Value::Integer(value) => value
                    .checked_neg()
                    .map(Value::Integer)
                    .ok_or(ExecutionError::ArithmeticOverflow { operator: "-" }),
                Value::Decimal(value) => Ok(Value::Decimal(-value)),
                other => Err(ExecutionError::InvalidUnaryArithmetic {
                    value_type: value_type(&other),
                    operator: "-",
                }),
            },
        }
    }

    fn evaluate_arithmetic(
        &self,
        left: Value,
        operator: ArithmeticOperator,
        right: Value,
    ) -> Result<Value, ExecutionError> {
        use ArithmeticOperator::{Add, Divide, Multiply, Subtract};

        if matches!(operator, Divide) {
            let (left_number, right_number) =
                arithmetic_numbers(&left, &right, arithmetic_operator_text(operator))?;
            if right_number == 0.0 {
                return Err(ExecutionError::DivisionByZero);
            }
            let result = left_number / right_number;
            if !result.is_finite() {
                return Err(ExecutionError::ArithmeticOverflow {
                    operator: arithmetic_operator_text(operator),
                });
            }
            return Ok(Value::Decimal(result));
        }

        match (&left, &right) {
            (Value::Integer(left), Value::Integer(right)) => {
                let result = match operator {
                    Add => left.checked_add(*right),
                    Subtract => left.checked_sub(*right),
                    Multiply => left.checked_mul(*right),
                    Divide => unreachable!(),
                };
                result
                    .map(Value::Integer)
                    .ok_or(ExecutionError::ArithmeticOverflow {
                        operator: arithmetic_operator_text(operator),
                    })
            }
            (Value::Integer(_), Value::Decimal(_))
            | (Value::Decimal(_), Value::Integer(_))
            | (Value::Decimal(_), Value::Decimal(_)) => {
                let (left_number, right_number) =
                    arithmetic_numbers(&left, &right, arithmetic_operator_text(operator))?;
                let result = match operator {
                    Add => left_number + right_number,
                    Subtract => left_number - right_number,
                    Multiply => left_number * right_number,
                    Divide => unreachable!(),
                };
                if result.is_finite() {
                    Ok(Value::Decimal(result))
                } else {
                    Err(ExecutionError::ArithmeticOverflow {
                        operator: arithmetic_operator_text(operator),
                    })
                }
            }
            _ => Err(ExecutionError::IncomparableArithmetic {
                left_type: value_type(&left),
                right_type: value_type(&right),
                operator: arithmetic_operator_text(operator),
            }),
        }
    }

    fn call_builtin(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        let canonical = if name.eq_ignore_ascii_case("integer") {
            Some("Integer")
        } else if name.eq_ignore_ascii_case("decimal") {
            Some("Decimal")
        } else if name.eq_ignore_ascii_case("boolean") {
            Some("Boolean")
        } else if name.eq_ignore_ascii_case("string") {
            Some("String")
        } else if name.eq_ignore_ascii_case("contains") {
            Some("Contains")
        } else if name.eq_ignore_ascii_case("startswith") {
            Some("StartsWith")
        } else if name.eq_ignore_ascii_case("endswith") {
            Some("EndsWith")
        } else if name.eq_ignore_ascii_case("isjapanese") {
            Some("IsJapanese")
        } else if name.eq_ignore_ascii_case("isinteger") {
            Some("IsInteger")
        } else if name.eq_ignore_ascii_case("isdecimal") {
            Some("IsDecimal")
        } else if name.eq_ignore_ascii_case("isboolean") {
            Some("IsBoolean")
        } else if name.eq_ignore_ascii_case("cellvalue") {
            Some("CellValue")
        } else if name.eq_ignore_ascii_case("rowcount") {
            Some("RowCount")
        } else if name.eq_ignore_ascii_case("columncount") {
            Some("ColumnCount")
        } else if name.eq_ignore_ascii_case("cellvalueat") {
            Some("CellValueAt")
        } else if name.eq_ignore_ascii_case("setcellvalueat") {
            Some("SetCellValueAt")
        } else {
            None
        };

        let Some(canonical) = canonical else {
            return Ok(None);
        };
        let expected = match canonical {
            "RowCount" | "ColumnCount" => 0,
            "Contains" | "StartsWith" | "EndsWith" | "CellValueAt" => 2,
            "SetCellValueAt" => 3,
            _ => 1,
        };
        if arguments.len() != expected {
            return Err(ExecutionError::ArgumentCount {
                name: canonical.to_owned(),
                expected,
                actual: arguments.len(),
            });
        }

        let values = self.evaluate_arguments(arguments)?;
        Ok(Some(match canonical {
            "Integer" => self.convert_integer(values[0].clone())?,
            "Decimal" => self.convert_decimal(values[0].clone())?,
            "Boolean" => self.convert_boolean(values[0].clone())?,
            "String" => self.convert_string(values[0].clone())?,
            "Contains" => {
                let haystack = self.expect_text(values[0].clone(), "Contains first argument")?;
                let needle = self.expect_text(values[1].clone(), "Contains second argument")?;
                Value::Boolean(haystack.contains(&needle))
            }
            "StartsWith" => {
                let value = self.expect_text(values[0].clone(), "StartsWith first argument")?;
                let prefix = self.expect_text(values[1].clone(), "StartsWith second argument")?;
                Value::Boolean(value.starts_with(&prefix))
            }
            "EndsWith" => {
                let value = self.expect_text(values[0].clone(), "EndsWith first argument")?;
                let suffix = self.expect_text(values[1].clone(), "EndsWith second argument")?;
                Value::Boolean(value.ends_with(&suffix))
            }
            "IsJapanese" => {
                let value = self.expect_text(values[0].clone(), "IsJapanese argument")?;
                Value::Boolean(contains_japanese(&value))
            }
            "IsInteger" => Value::Boolean(is_integer_value(&values[0])),
            "IsDecimal" => Value::Boolean(is_decimal_value(&values[0])),
            "IsBoolean" => Value::Boolean(is_boolean_value(&values[0])),
            "CellValue" => {
                let reference = self.expect_text(values[0].clone(), "CellValue argument")?;
                let value = self
                    .document
                    .cell_a1(&reference)?
                    .ok_or_else(|| ExecutionError::MissingCell(reference.clone()))?;
                Value::Text(value.to_owned())
            }
            "RowCount" => Value::Integer(self.document.row_count() as i64),
            "ColumnCount" => Value::Integer(self.document.column_count() as i64),
            "CellValueAt" => {
                let row = self.expect_one_based_index(values[0].clone(), "CellValueAt row")?;
                let column =
                    self.expect_one_based_index(values[1].clone(), "CellValueAt column")?;
                let value = self.document.cell(row - 1, column - 1).ok_or_else(|| {
                    ExecutionError::MissingCell(format!("row {row}, column {column}"))
                })?;
                Value::Text(value.to_owned())
            }
            "SetCellValueAt" => {
                let row = self.expect_one_based_index(values[0].clone(), "SetCellValueAt row")?;
                let column =
                    self.expect_one_based_index(values[1].clone(), "SetCellValueAt column")?;
                let value = self.cell_text(values[2].clone())?;
                self.document.set_cell(row - 1, column - 1, value.clone())?;
                Value::Text(value)
            }
            _ => unreachable!(),
        }))
    }

    fn convert_integer(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Integer(value) => Ok(Value::Integer(value)),
            Value::Text(value) => value
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| conversion_error("String", value, "Integer")),
            other => Err(conversion_error(
                value_type(&other),
                self.describe_value(&other),
                "Integer",
            )),
        }
    }

    fn convert_decimal(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Decimal(value) => Ok(Value::Decimal(value)),
            Value::Integer(value) => Ok(Value::Decimal(value as f64)),
            Value::Text(value) => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(Value::Decimal)
                .ok_or_else(|| conversion_error("String", value, "Decimal")),
            other => Err(conversion_error(
                value_type(&other),
                self.describe_value(&other),
                "Decimal",
            )),
        }
    }

    fn convert_boolean(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Boolean(value) => Ok(Value::Boolean(value)),
            Value::Text(value) if value.eq_ignore_ascii_case("true") => Ok(Value::Boolean(true)),
            Value::Text(value) if value.eq_ignore_ascii_case("false") => Ok(Value::Boolean(false)),
            Value::Text(value) => Err(conversion_error("String", value, "Boolean")),
            other => Err(conversion_error(
                value_type(&other),
                self.describe_value(&other),
                "Boolean",
            )),
        }
    }

    fn convert_string(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Object(_) => Err(ExecutionError::ExpectedText {
                context: "String conversion".to_owned(),
            }),
            value => Ok(Value::Text(self.describe_value(&value))),
        }
    }

    fn instantiate_class(&mut self, class_name: &str) -> Result<Value, ExecutionError> {
        let class = self
            .program
            .classes
            .iter()
            .find(|class| class.name.eq_ignore_ascii_case(class_name))
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownClass(class_name.to_owned()))?;
        let mut fields = HashMap::new();
        for field in &class.fields {
            let value = self.evaluate_expression(&field.default)?;
            fields.insert(normalize_identifier(&field.name), value);
        }
        let object_id = self.objects.len();
        self.objects.push(ObjectInstance {
            class_name: class.name.clone(),
            fields,
        });
        self.events.push(ExecutionEvent::ObjectCreated {
            class_name: class.name,
        });
        Ok(Value::Object(object_id))
    }

    fn call_function(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        self.ensure_call_depth()?;
        let function = self
            .program
            .functions
            .iter()
            .find(|function| function.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownFunction(name.to_owned()))?;
        if function.parameters.len() != arguments.len() {
            return Err(ExecutionError::ArgumentCount {
                name: function.name,
                expected: function.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        let scope = function
            .parameters
            .iter()
            .zip(values.iter())
            .map(|(parameter, value)| (normalize_identifier(parameter), value.clone()))
            .collect();
        self.scopes.push(scope);
        self.call_depth += 1;
        let execution = self.execute_statements(&function.body);
        self.call_depth -= 1;
        self.scopes.pop();
        let return_value = match execution? {
            Flow::Continue => None,
            Flow::Return(value) => value,
        };
        self.events.push(ExecutionEvent::FunctionCalled {
            name: function.name,
            arguments: values
                .iter()
                .map(|value| self.describe_value(value))
                .collect(),
            return_value: return_value
                .as_ref()
                .map(|value| self.describe_value(value)),
        });
        Ok(return_value)
    }

    fn call_method(
        &mut self,
        object_id: ObjectId,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        self.ensure_call_depth()?;
        let class_name = self.object(object_id)?.class_name.clone();
        let class = self
            .program
            .classes
            .iter()
            .find(|class| class.name.eq_ignore_ascii_case(&class_name))
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownClass(class_name.clone()))?;
        let method = class
            .methods
            .iter()
            .find(|method| method.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownMethod {
                class_name: class.name.clone(),
                method: name.to_owned(),
            })?;
        if method.parameters.len() != arguments.len() {
            return Err(ExecutionError::MethodArgumentCount {
                class_name: class.name,
                name: method.name,
                expected: method.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        let mut scope: HashMap<String, Value> = method
            .parameters
            .iter()
            .zip(values.iter())
            .map(|(parameter, value)| (normalize_identifier(parameter), value.clone()))
            .collect();
        scope.insert("self".to_owned(), Value::Object(object_id));
        self.scopes.push(scope);
        self.call_depth += 1;
        let execution = self.execute_statements(&method.body);
        self.call_depth -= 1;
        self.scopes.pop();
        let return_value = match execution? {
            Flow::Continue => None,
            Flow::Return(value) => value,
        };
        self.events.push(ExecutionEvent::MethodCalled {
            class_name,
            name: method.name,
            arguments: values
                .iter()
                .map(|value| self.describe_value(value))
                .collect(),
            return_value: return_value
                .as_ref()
                .map(|value| self.describe_value(value)),
        });
        Ok(return_value)
    }

    fn evaluate_arguments(
        &mut self,
        arguments: &[Expression],
    ) -> Result<Vec<Value>, ExecutionError> {
        arguments
            .iter()
            .map(|argument| self.evaluate_expression(argument))
            .collect()
    }

    fn ensure_call_depth(&self) -> Result<(), ExecutionError> {
        if self.call_depth >= MAX_CALL_DEPTH {
            Err(ExecutionError::CallDepthExceeded {
                limit: MAX_CALL_DEPTH,
            })
        } else {
            Ok(())
        }
    }

    fn resolve_object(&self, variable: &str) -> Result<ObjectId, ExecutionError> {
        match self.lookup_variable(variable) {
            Some(Value::Object(object_id)) => Ok(*object_id),
            Some(_) => Err(ExecutionError::ExpectedObject(variable.to_owned())),
            None => Err(ExecutionError::UnknownVariable(variable.to_owned())),
        }
    }

    fn get_field(&self, object_id: ObjectId, field: &str) -> Result<Value, ExecutionError> {
        let object = self.object(object_id)?;
        object
            .fields
            .get(&normalize_identifier(field))
            .cloned()
            .ok_or_else(|| ExecutionError::UnknownField {
                class_name: object.class_name.clone(),
                field: field.to_owned(),
            })
    }

    fn set_field(
        &mut self,
        object_id: ObjectId,
        field: &str,
        value: Value,
    ) -> Result<(), ExecutionError> {
        let key = normalize_identifier(field);
        let object = self.object_mut(object_id)?;
        if !object.fields.contains_key(&key) {
            return Err(ExecutionError::UnknownField {
                class_name: object.class_name.clone(),
                field: field.to_owned(),
            });
        }
        object.fields.insert(key, value);
        Ok(())
    }

    fn object(&self, object_id: ObjectId) -> Result<&ObjectInstance, ExecutionError> {
        self.objects
            .get(object_id)
            .ok_or_else(|| ExecutionError::ExpectedObject(format!("object#{object_id}")))
    }

    fn object_mut(&mut self, object_id: ObjectId) -> Result<&mut ObjectInstance, ExecutionError> {
        self.objects
            .get_mut(object_id)
            .ok_or_else(|| ExecutionError::ExpectedObject(format!("object#{object_id}")))
    }

    fn expect_integer(&self, value: Value, context: &str) -> Result<i64, ExecutionError> {
        match value {
            Value::Integer(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedInteger {
                context: context.to_owned(),
            }),
        }
    }

    fn expect_one_based_index(&self, value: Value, context: &str) -> Result<usize, ExecutionError> {
        let value = self.expect_integer(value, context)?;
        if value < 1 {
            return Err(ExecutionError::InvalidOneBasedIndex {
                context: context.to_owned(),
            });
        }
        usize::try_from(value).map_err(|_| ExecutionError::InvalidOneBasedIndex {
            context: context.to_owned(),
        })
    }

    fn expect_text(&self, value: Value, context: &str) -> Result<String, ExecutionError> {
        match value {
            Value::Text(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedText {
                context: context.to_owned(),
            }),
        }
    }

    fn expect_boolean(&self, value: Value, context: &str) -> Result<bool, ExecutionError> {
        match value {
            Value::Boolean(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedBoolean {
                context: context.to_owned(),
            }),
        }
    }

    fn cell_text(&self, value: Value) -> Result<String, ExecutionError> {
        match value {
            Value::Text(value) => Ok(value),
            Value::Integer(value) => Ok(value.to_string()),
            Value::Decimal(value) => Ok(value.to_string()),
            Value::Boolean(value) => Ok(value.to_string()),
            Value::Object(_) => Err(ExecutionError::ExpectedText {
                context: "cell assignment".to_owned(),
            }),
        }
    }

    fn scalar_text(&self, value: &Value) -> Option<String> {
        match value {
            Value::Text(value) => Some(value.clone()),
            Value::Integer(value) => Some(value.to_string()),
            Value::Decimal(value) => Some(value.to_string()),
            Value::Boolean(value) => Some(value.to_string()),
            Value::Object(_) => None,
        }
    }

    fn describe_value(&self, value: &Value) -> String {
        match value {
            Value::Text(value) => value.clone(),
            Value::Integer(value) => value.to_string(),
            Value::Decimal(value) => value.to_string(),
            Value::Boolean(value) => value.to_string(),
            Value::Object(object_id) => self
                .objects
                .get(*object_id)
                .map(|object| format!("<{} object>", object.class_name))
                .unwrap_or_else(|| "<object>".to_owned()),
        }
    }

    fn lookup_variable(&self, name: &str) -> Option<&Value> {
        let key = normalize_identifier(name);
        self.scopes.iter().rev().find_map(|scope| scope.get(&key))
    }

    fn current_scope_mut(&mut self) -> &mut HashMap<String, Value> {
        self.scopes
            .last_mut()
            .expect("runtime always has at least the global scope")
    }
}

fn contains_japanese(value: &str) -> bool {
    value.chars().any(|ch| {
        matches!(
            ch,
            '\u{3040}'..='\u{309f}'
                | '\u{30a0}'..='\u{30ff}'
                | '\u{31f0}'..='\u{31ff}'
                | '\u{3400}'..='\u{4dbf}'
                | '\u{4e00}'..='\u{9fff}'
                | '\u{f900}'..='\u{faff}'
                | '\u{ff66}'..='\u{ff9f}'
        )
    })
}

fn is_integer_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) => true,
        Value::Text(value) => value.parse::<i64>().is_ok(),
        _ => false,
    }
}

fn is_decimal_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Decimal(_) => true,
        Value::Text(value) => value.parse::<f64>().is_ok_and(|parsed| parsed.is_finite()),
        _ => false,
    }
}

fn is_boolean_value(value: &Value) -> bool {
    match value {
        Value::Boolean(_) => true,
        Value::Text(value) => {
            value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
        }
        _ => false,
    }
}

fn compare_f64(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right)
        .expect("Rowly DSL decimal values are always finite")
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Text(_) => "String",
        Value::Integer(_) => "Integer",
        Value::Decimal(_) => "Decimal",
        Value::Boolean(_) => "Boolean",
        Value::Object(_) => "Object",
    }
}

fn arithmetic_numbers(
    left: &Value,
    right: &Value,
    operator: &'static str,
) -> Result<(f64, f64), ExecutionError> {
    let number = |value: &Value| match value {
        Value::Integer(value) => Some(*value as f64),
        Value::Decimal(value) => Some(*value),
        _ => None,
    };
    match (number(left), number(right)) {
        (Some(left), Some(right)) => Ok((left, right)),
        _ => Err(ExecutionError::IncomparableArithmetic {
            left_type: value_type(left),
            right_type: value_type(right),
            operator,
        }),
    }
}

fn arithmetic_operator_text(operator: ArithmeticOperator) -> &'static str {
    match operator {
        ArithmeticOperator::Add => "+",
        ArithmeticOperator::Subtract => "-",
        ArithmeticOperator::Multiply => "*",
        ArithmeticOperator::Divide => "/",
    }
}

fn comparison_operator_text(operator: ComparisonOperator) -> &'static str {
    match operator {
        ComparisonOperator::Equal => "=",
        ComparisonOperator::NotEqual => "!=",
        ComparisonOperator::Less => "<",
        ComparisonOperator::LessOrEqual => "<=",
        ComparisonOperator::Greater => ">",
        ComparisonOperator::GreaterOrEqual => ">=",
    }
}

fn conversion_error(
    value_type: &'static str,
    value: String,
    target: &'static str,
) -> ExecutionError {
    ExecutionError::Conversion {
        value_type,
        value,
        target,
    }
}

fn resolve_column(selector: &ColumnSelector, document: &CsvDocument) -> Result<usize, ColumnError> {
    match selector {
        ColumnSelector::Index(column) => {
            let column_count = document.column_count();
            if *column >= column_count {
                return Err(ColumnError::ColumnOutOfBounds {
                    column: *column,
                    column_count,
                });
            }
            Ok(*column)
        }
        ColumnSelector::Header(header) => document.column_index_by_header(header),
    }
}
