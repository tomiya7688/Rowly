use std::cmp::Ordering;
use std::collections::HashMap;

use thiserror::Error;

use crate::process::{ColumnError, CsvDocument, DocumentError};

use super::ast::{
    ArithmeticOperator, ColumnSelector, ComparisonOperator, Condition, DeclarationKind,
    ExecutionEvent, ExecutionReport, Expression, Program, StandardNamespace, Statement,
    UnaryOperator, normalize_identifier,
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
struct Binding {
    value: Value,
    kind: DeclarationKind,
}

impl Binding {
    fn variable(value: Value) -> Self {
        Self {
            value,
            kind: DeclarationKind::Var,
        }
    }
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
    #[error("cannot assign to Rowly DSL constant `{0}`")]
    ConstantAssignment(String),
    #[error("Rowly DSL variable `{0}` is already declared in this scope")]
    DuplicateVariable(String),
    #[error("unknown Rowly DSL function `{0}`")]
    UnknownFunction(String),
    #[error("unknown Rowly DSL standard function `{namespace}.{name}`")]
    UnknownStandardFunction {
        namespace: &'static str,
        name: String,
    },
    #[error("unknown Rowly DSL class `{0}`")]
    UnknownClass(String),
    #[error("class `{class_name}` extends unknown class `{parent}`")]
    UnknownParentClass { class_name: String, parent: String },
    #[error("inheritance cycle detected at class `{0}`")]
    InheritanceCycle(String),
    #[error("cell `{0}` is outside the existing CSV table")]
    MissingCell(String),
    #[error("unknown field `{field}` on class `{class_name}`")]
    UnknownField { class_name: String, field: String },
    #[error("unknown method `{method}` on class `{class_name}`")]
    UnknownMethod { class_name: String, method: String },
    #[error("`Super` can only be used inside a method or constructor")]
    SuperOutsideMethod,
    #[error("class `{class_name}` has no parent class for `Super`")]
    NoSuperClass { class_name: String },
    #[error("unknown super method `{method}` above class `{class_name}`")]
    UnknownSuperMethod { class_name: String, method: String },
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
    #[error(
        "constructor for class `{class_name}` expects {expected} arguments but received {actual}"
    )]
    ConstructorArgumentCount {
        class_name: String,
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
    scopes: Vec<HashMap<String, Binding>>,
    objects: Vec<ObjectInstance>,
    events: Vec<ExecutionEvent>,
    call_depth: usize,
    method_context: Vec<String>,
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
            method_context: Vec::new(),
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
        for (name, binding) in global {
            match binding.value {
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

                    loop {
                        let in_range = if step > 0 {
                            current <= end
                        } else {
                            current >= end
                        };
                        if !in_range {
                            break;
                        }

                        // 各反復の宣言を独立させ、CONST の再宣言や外側への漏出を防ぐ。
                        self.scopes.push(HashMap::from([(
                            normalize_identifier(variable),
                            Binding::variable(Value::Integer(current)),
                        )]));
                        let execution = self.execute_statements(body);
                        self.scopes.pop();
                        let flow = execution?;
                        if !matches!(flow, Flow::Continue) {
                            return Ok(flow);
                        }

                        current = current
                            .checked_add(step)
                            .ok_or(ExecutionError::LoopCounterOverflow)?;
                    }
                }
                Statement::Declare { kind, name, value } => {
                    let key = normalize_identifier(name);
                    // RHS の関数呼び出し等による副作用より先に、宣言先を検証する。
                    if self.current_scope_mut().contains_key(&key) {
                        return Err(ExecutionError::DuplicateVariable(name.clone()));
                    }
                    let value = self.evaluate_expression(value)?;
                    let event_value = self.describe_value(&value);
                    self.current_scope_mut()
                        .insert(key, Binding { value, kind: *kind });
                    self.events.push(ExecutionEvent::VariableSet {
                        name: name.clone(),
                        value: event_value,
                    });
                }
                Statement::Assign { name, value } => {
                    let key = normalize_identifier(name);
                    let scope = self
                        .scopes
                        .iter()
                        .rposition(|scope| scope.contains_key(&key))
                        .ok_or_else(|| ExecutionError::UnknownVariable(name.clone()))?;
                    if self.scopes[scope][&key].kind == DeclarationKind::Const {
                        return Err(ExecutionError::ConstantAssignment(name.clone()));
                    }
                    let value = self.evaluate_expression(value)?;
                    let event_value = self.describe_value(&value);
                    self.scopes[scope]
                        .get_mut(&key)
                        .expect("assignment target was resolved before evaluation")
                        .value = value;
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
                    if self.call_statement_builtin(name, arguments)? {
                        continue;
                    }
                    if self.call_builtin(name, arguments)?.is_none() {
                        let _ = self.call_function(name, arguments)?;
                    }
                }
                Statement::MethodCall {
                    target,
                    name,
                    arguments,
                } => {
                    if target.eq_ignore_ascii_case("Super") {
                        let _ = self.call_super_method(name, arguments)?;
                    } else {
                        let object_id = self.resolve_object(target)?;
                        let _ = self.call_method(object_id, name, arguments)?;
                    }
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
            Expression::StandardCall {
                namespace,
                name,
                arguments,
            } => self.call_standard_namespace(*namespace, name, arguments),
            Expression::New {
                class_name,
                arguments,
            } => self.instantiate_class(class_name, arguments),
            Expression::Field { target, field } => {
                let object_id = self.resolve_object(target)?;
                self.get_field(object_id, field)
            }
            Expression::MethodCall {
                target,
                name,
                arguments,
            } => {
                let return_value = if target.eq_ignore_ascii_case("Super") {
                    self.call_super_method(name, arguments)?
                } else {
                    let object_id = self.resolve_object(target)?;
                    self.call_method(object_id, name, arguments)?
                };
                return_value
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

    fn call_statement_builtin(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<bool, ExecutionError> {
        let canonical = if name.eq_ignore_ascii_case("begintransaction") {
            Some("BeginTransaction")
        } else if name.eq_ignore_ascii_case("committransaction") {
            Some("CommitTransaction")
        } else if name.eq_ignore_ascii_case("rollbacktransaction") {
            Some("RollbackTransaction")
        } else {
            None
        };

        let Some(canonical) = canonical else {
            return Ok(false);
        };
        if !arguments.is_empty() {
            return Err(ExecutionError::ArgumentCount {
                name: canonical.to_owned(),
                expected: 0,
                actual: arguments.len(),
            });
        }

        match canonical {
            "BeginTransaction" => self.document.begin_transaction()?,
            "CommitTransaction" => self.document.commit_transaction()?,
            "RollbackTransaction" => self.document.rollback_transaction()?,
            _ => unreachable!(),
        }
        Ok(true)
    }

    fn call_standard_namespace(
        &mut self,
        namespace: StandardNamespace,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Value, ExecutionError> {
        let (namespace_name, canonical, expected) = match namespace {
            StandardNamespace::Text => {
                let canonical = if name.eq_ignore_ascii_case("contains") {
                    Some(("Contains", 2))
                } else if name.eq_ignore_ascii_case("startswith") {
                    Some(("StartsWith", 2))
                } else if name.eq_ignore_ascii_case("endswith") {
                    Some(("EndsWith", 2))
                } else if name.eq_ignore_ascii_case("isjapanese") {
                    Some(("IsJapanese", 1))
                } else {
                    None
                };
                ("Text", canonical, ())
            }
            StandardNamespace::Number => {
                let canonical = if name.eq_ignore_ascii_case("isinteger") {
                    Some(("IsInteger", 1))
                } else if name.eq_ignore_ascii_case("isdecimal") {
                    Some(("IsDecimal", 1))
                } else {
                    None
                };
                ("Number", canonical, ())
            }
            StandardNamespace::Boolean => {
                let canonical = if name.eq_ignore_ascii_case("isvalid") {
                    Some(("IsValid", 1))
                } else {
                    None
                };
                ("Boolean", canonical, ())
            }
        };
        let _ = expected;
        let (canonical, expected) = canonical.ok_or_else(|| ExecutionError::UnknownStandardFunction {
            namespace: namespace_name,
            name: name.to_owned(),
        })?;
        let full_name = format!("{namespace_name}.{canonical}");
        if arguments.len() != expected {
            return Err(ExecutionError::ArgumentCount {
                name: full_name.clone(),
                expected,
                actual: arguments.len(),
            });
        }

        let values = self.evaluate_arguments(arguments)?;
        match (namespace, canonical) {
            (StandardNamespace::Text, "Contains") => {
                let haystack =
                    self.expect_text(values[0].clone(), "Text.Contains first argument")?;
                let needle =
                    self.expect_text(values[1].clone(), "Text.Contains second argument")?;
                Ok(Value::Boolean(haystack.contains(&needle)))
            }
            (StandardNamespace::Text, "StartsWith") => {
                let value =
                    self.expect_text(values[0].clone(), "Text.StartsWith first argument")?;
                let prefix =
                    self.expect_text(values[1].clone(), "Text.StartsWith second argument")?;
                Ok(Value::Boolean(value.starts_with(&prefix)))
            }
            (StandardNamespace::Text, "EndsWith") => {
                let value =
                    self.expect_text(values[0].clone(), "Text.EndsWith first argument")?;
                let suffix =
                    self.expect_text(values[1].clone(), "Text.EndsWith second argument")?;
                Ok(Value::Boolean(value.ends_with(&suffix)))
            }
            (StandardNamespace::Text, "IsJapanese") => {
                let value = self.expect_text(values[0].clone(), "Text.IsJapanese argument")?;
                Ok(Value::Boolean(contains_japanese(&value)))
            }
            (StandardNamespace::Number, "IsInteger") => {
                Ok(Value::Boolean(is_integer_value(&values[0])))
            }
            (StandardNamespace::Number, "IsDecimal") => {
                Ok(Value::Boolean(is_decimal_value(&values[0])))
            }
            (StandardNamespace::Boolean, "IsValid") => {
                Ok(Value::Boolean(is_boolean_value(&values[0])))
            }
            _ => unreachable!(),
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
        } else if name.eq_ignore_ascii_case("columnindex") {
            Some("ColumnIndex")
        } else if name.eq_ignore_ascii_case("cellvaluebyheader") {
            Some("CellValueByHeader")
        } else if name.eq_ignore_ascii_case("setcellvaluebyheader") {
            Some("SetCellValueByHeader")
        } else {
            None
        };

        let Some(canonical) = canonical else {
            return Ok(None);
        };
        let expected = match canonical {
            "RowCount" | "ColumnCount" => 0,
            "CellValueAt" | "CellValueByHeader" => 2,
            "SetCellValueAt" | "SetCellValueByHeader" => 3,
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
            "ColumnIndex" => {
                let header = self.expect_text(values[0].clone(), "ColumnIndex header")?;
                let column = self.document.column_index_by_header(&header)?;
                Value::Integer((column + 1) as i64)
            }
            "CellValueByHeader" => {
                let row =
                    self.expect_one_based_index(values[0].clone(), "CellValueByHeader row")?;
                let header = self.expect_text(values[1].clone(), "CellValueByHeader header")?;
                let column = self.document.column_index_by_header(&header)?;
                let value = self.document.cell(row - 1, column).ok_or_else(|| {
                    ExecutionError::MissingCell(format!("row {row}, header {header}"))
                })?;
                Value::Text(value.to_owned())
            }
            "SetCellValueByHeader" => {
                let row =
                    self.expect_one_based_index(values[0].clone(), "SetCellValueByHeader row")?;
                let header = self.expect_text(values[1].clone(), "SetCellValueByHeader header")?;
                let column = self.document.column_index_by_header(&header)?;
                let value = self.cell_text(values[2].clone())?;
                self.document.set_cell(row - 1, column, value.clone())?;
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

    fn instantiate_class(
        &mut self,
        class_name: &str,
        arguments: &[Expression],
    ) -> Result<Value, ExecutionError> {
        let class = self.class_by_name(class_name)?.clone();
        let lineage = self.class_lineage(&class.name)?;
        let constructor = self.find_method_in_hierarchy(&class.name, "Init")?;
        let expected = constructor
            .as_ref()
            .map(|(_, constructor)| constructor.parameters.len())
            .unwrap_or(0);
        if expected != arguments.len() {
            return Err(ExecutionError::ConstructorArgumentCount {
                class_name: class.name,
                expected,
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;

        let mut fields = HashMap::new();
        for current in &lineage {
            for field in &current.fields {
                let value = self.evaluate_expression(&field.default)?;
                fields.insert(normalize_identifier(&field.name), value);
            }
        }
        let object_id = self.objects.len();
        self.objects.push(ObjectInstance {
            class_name: class.name.clone(),
            fields,
        });
        self.events.push(ExecutionEvent::ObjectCreated {
            class_name: class.name.clone(),
        });

        if let Some((defining_class, constructor)) = constructor {
            let _ =
                self.invoke_method_with_values(object_id, &defining_class, &constructor, values)?;
        }

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
            .map(|(parameter, value)| {
                (
                    normalize_identifier(parameter),
                    Binding::variable(value.clone()),
                )
            })
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
        let class_name = self.object(object_id)?.class_name.clone();
        let (defining_class, method) = self
            .find_method_in_hierarchy(&class_name, name)?
            .ok_or_else(|| ExecutionError::UnknownMethod {
                class_name: class_name.clone(),
                method: name.to_owned(),
            })?;
        if method.parameters.len() != arguments.len() {
            return Err(ExecutionError::MethodArgumentCount {
                class_name,
                name: method.name,
                expected: method.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        self.invoke_method_with_values(object_id, &defining_class, &method, values)
    }

    fn call_super_method(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        let current_class = self
            .method_context
            .last()
            .cloned()
            .ok_or(ExecutionError::SuperOutsideMethod)?;
        let parent = self
            .class_by_name(&current_class)?
            .parent
            .clone()
            .ok_or_else(|| ExecutionError::NoSuperClass {
                class_name: current_class.clone(),
            })?;
        let object_id = self.resolve_object("Self")?;
        let (defining_class, method) =
            self.find_method_in_hierarchy(&parent, name)?
                .ok_or_else(|| ExecutionError::UnknownSuperMethod {
                    class_name: current_class,
                    method: name.to_owned(),
                })?;
        if method.parameters.len() != arguments.len() {
            return Err(ExecutionError::MethodArgumentCount {
                class_name: defining_class.clone(),
                name: method.name,
                expected: method.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        self.invoke_method_with_values(object_id, &defining_class, &method, values)
    }

    fn invoke_method_with_values(
        &mut self,
        object_id: ObjectId,
        defining_class: &str,
        method: &super::ast::FunctionDefinition,
        values: Vec<Value>,
    ) -> Result<Option<Value>, ExecutionError> {
        self.ensure_call_depth()?;
        let mut scope: HashMap<String, Binding> = method
            .parameters
            .iter()
            .zip(values.iter())
            .map(|(parameter, value)| {
                (
                    normalize_identifier(parameter),
                    Binding::variable(value.clone()),
                )
            })
            .collect();
        scope.insert(
            "self".to_owned(),
            Binding {
                value: Value::Object(object_id),
                kind: DeclarationKind::Const,
            },
        );
        self.scopes.push(scope);
        self.method_context.push(defining_class.to_owned());
        self.call_depth += 1;
        let execution = self.execute_statements(&method.body);
        self.call_depth -= 1;
        self.method_context.pop();
        self.scopes.pop();
        let return_value = match execution? {
            Flow::Continue => None,
            Flow::Return(value) => value,
        };
        self.events.push(ExecutionEvent::MethodCalled {
            class_name: defining_class.to_owned(),
            name: method.name.clone(),
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

    fn class_by_name(&self, name: &str) -> Result<&super::ast::ClassDefinition, ExecutionError> {
        self.program
            .classes
            .iter()
            .find(|class| class.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| ExecutionError::UnknownClass(name.to_owned()))
    }

    fn class_lineage(
        &self,
        class_name: &str,
    ) -> Result<Vec<super::ast::ClassDefinition>, ExecutionError> {
        let mut lineage = Vec::new();
        let mut seen = Vec::new();
        let mut current = self.class_by_name(class_name)?.clone();

        loop {
            let key = normalize_identifier(&current.name);
            if seen.iter().any(|name| name == &key) {
                return Err(ExecutionError::InheritanceCycle(current.name));
            }
            seen.push(key);
            lineage.push(current.clone());

            let Some(parent_name) = current.parent.as_deref() else {
                break;
            };
            current = self
                .program
                .classes
                .iter()
                .find(|class| class.name.eq_ignore_ascii_case(parent_name))
                .cloned()
                .ok_or_else(|| ExecutionError::UnknownParentClass {
                    class_name: current.name.clone(),
                    parent: parent_name.to_owned(),
                })?;
        }

        lineage.reverse();
        Ok(lineage)
    }

    fn find_method_in_hierarchy(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Result<Option<(String, super::ast::FunctionDefinition)>, ExecutionError> {
        let lineage = self.class_lineage(class_name)?;
        Ok(lineage.iter().rev().find_map(|class| {
            class
                .methods
                .iter()
                .find(|method| method.name.eq_ignore_ascii_case(method_name))
                .cloned()
                .map(|method| (class.name.clone(), method))
        }))
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
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&key).map(|binding| &binding.value))
    }

    fn current_scope_mut(&mut self) -> &mut HashMap<String, Binding> {
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
