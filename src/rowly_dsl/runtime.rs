use std::collections::HashMap;

use thiserror::Error;

use crate::process::{ColumnError, CsvDocument, DocumentError};

use super::ast::{
    ColumnSelector, Condition, ExecutionEvent, ExecutionReport, Expression, Program, Statement,
    normalize_identifier,
};

const MAX_CALL_DEPTH: usize = 64;

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

    #[error("function `{name}` expects {expected} arguments but received {actual}")]
    ArgumentCount {
        name: String,
        expected: usize,
        actual: usize,
    },

    #[error("function `{0}` was used as a value but did not return one")]
    MissingReturnValue(String),

    #[error("`Return` can only be used inside a function")]
    ReturnOutsideFunction,

    #[error("Rowly DSL call depth exceeded the limit of {limit}")]
    CallDepthExceeded { limit: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Flow {
    Continue,
    Return(Option<String>),
}

struct Runtime<'a> {
    program: &'a Program,
    document: &'a mut CsvDocument,
    scopes: Vec<HashMap<String, String>>,
    events: Vec<ExecutionEvent>,
    call_depth: usize,
}

impl<'a> Runtime<'a> {
    fn new(program: &'a Program, document: &'a mut CsvDocument) -> Self {
        Self {
            program,
            document,
            scopes: vec![HashMap::new()],
            events: Vec::new(),
            call_depth: 0,
        }
    }

    fn execute(mut self) -> Result<ExecutionReport, ExecutionError> {
        let statements = self.program.statements.clone();
        match self.execute_statements(&statements)? {
            Flow::Continue => {}
            Flow::Return(_) => return Err(ExecutionError::ReturnOutsideFunction),
        }

        let variables = self.scopes.into_iter().next().unwrap_or_default();
        Ok(ExecutionReport {
            events: self.events,
            variables,
        })
    }

    fn execute_statements(&mut self, statements: &[Statement]) -> Result<Flow, ExecutionError> {
        for statement in statements {
            match statement {
                Statement::If { condition, body } => {
                    let result = self.evaluate_condition(condition)?;
                    self.events.push(ExecutionEvent::ConditionEvaluated {
                        condition: condition.clone(),
                        result,
                    });
                    if result {
                        let flow = self.execute_statements(body)?;
                        if !matches!(flow, Flow::Continue) {
                            return Ok(flow);
                        }
                    }
                }
                Statement::Let { name, value } => {
                    let value = self.evaluate_expression(value)?;
                    let key = normalize_identifier(name);
                    self.current_scope_mut().insert(key, value.clone());
                    self.events.push(ExecutionEvent::VariableSet {
                        name: name.clone(),
                        value,
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
                    let _ = self.call_function(name, arguments)?;
                }
                Statement::SetRangeValue { range, value } => {
                    let value = self.evaluate_expression(value)?;
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
                Ok(self.document.cell(0, column) == Some(expected.as_str()))
            }
            Condition::ValueEquals { left, right } => {
                Ok(self.evaluate_expression(left)? == self.evaluate_expression(right)?)
            }
        }
    }

    fn evaluate_expression(&mut self, expression: &Expression) -> Result<String, ExecutionError> {
        match expression {
            Expression::Literal(value) => Ok(value.clone()),
            Expression::Variable(name) => self
                .lookup_variable(name)
                .map(str::to_owned)
                .ok_or_else(|| ExecutionError::UnknownVariable(name.clone())),
            Expression::Call { name, arguments } => self
                .call_function(name, arguments)?
                .ok_or_else(|| ExecutionError::MissingReturnValue(name.clone())),
        }
    }

    fn call_function(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<String>, ExecutionError> {
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(ExecutionError::CallDepthExceeded {
                limit: MAX_CALL_DEPTH,
            });
        }

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

        let values = arguments
            .iter()
            .map(|argument| self.evaluate_expression(argument))
            .collect::<Result<Vec<_>, _>>()?;

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
            arguments: values,
            return_value: return_value.clone(),
        });

        Ok(return_value)
    }

    fn lookup_variable(&self, name: &str) -> Option<&str> {
        let key = normalize_identifier(name);
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&key))
            .map(String::as_str)
    }

    fn current_scope_mut(&mut self) -> &mut HashMap<String, String> {
        self.scopes
            .last_mut()
            .expect("runtime always has at least the global scope")
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
