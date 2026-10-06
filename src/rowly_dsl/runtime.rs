use std::cmp::Ordering;
use std::collections::HashMap;

use thiserror::Error;

use crate::process::{ColumnError, CsvDocument, DocumentError, ValidationTarget};

use super::ast::{
    ArithmeticOperator, ColumnSelector, ComparisonOperator, Condition, DeclarationKind,
    ExecutionEvent, ExecutionReport, Expression, Program, StandardNamespace, Statement,
    UnaryOperator, ValidationRuleDefinition, normalize_identifier,
};

// {
//   責務: [MAX_CALL_DEPTH: Rowly DSL関数呼び出しの最大ネスト数を定義する]
//   値: [64: 再帰による実行の過剰な積み上がりを抑える上限]
// }
const MAX_CALL_DEPTH: usize = 64;
// {
//   責務: [ObjectId: Runtime内のobjects配列を参照する識別子型]
//   型: [usize: objects配列の添字]
// }
type ObjectId = usize;

// {
//   責務: [Value: DSL実行時に評価される値の型を表す]
//   バリアント: [Text / Integer / Decimal / Boolean: スカラー値, Object: 実行時オブジェクトID]
// }
#[derive(Debug, Clone)]
enum Value {
    Text(String),
    Integer(i64),
    Decimal(f64),
    Boolean(bool),
    Object(ObjectId),
}

// {
//   責務: [Binding: スコープ内の値とVAR/CONST宣言種別を保持する]
//   フィールド: [value: 現在の実行時値, kind: 再代入可否を決める宣言種別]
// }
#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    kind: DeclarationKind,
}

impl Binding {
    // {
    //   責務: [variable: 可変VAR bindingを作成する]
    //   引数: [value: 初期実行時値]
    //   戻り値: [Binding: DeclarationKind::Varを持つ束縛]
    // }
    fn variable(value: Value) -> Self {
        Self {
            value,
            kind: DeclarationKind::Var,
        }
    }
}

// {
//   責務: [ObjectInstance: DSL実行中だけ存在するclass instanceの状態を保持する]
//   フィールド: [class_name: class名, fields: 正規化したfield名と実行時値]
// }
#[derive(Debug, Clone)]
struct ObjectInstance {
    class_name: String,
    fields: HashMap<String, Value>,
}

/// ```text
/// 責務: [execute: DSL ProgramをCsvDocumentに対して実行する]
/// 処理: [Runtimeを構築してtop-level statementを評価し、実行レポートを返す]
/// 引数: [program: parserが生成したAST, document: 操作対象のCSV document]
/// 戻り値: [ExecutionReport: events、global scalar variable、global object直下のscalar field、validation rule]
/// エラー: [ExecutionError: DSL実行またはCsvDocument操作の失敗]
/// ```
pub fn execute(
    program: &Program,
    document: &mut CsvDocument,
) -> Result<ExecutionReport, ExecutionError> {
    Runtime::new(program, document).execute()
}

// {
//   責務: [ExecutionError: DSL評価、型検査、呼び出し、document操作の失敗を表す]
//   バリアント: [原因ごとに区別可能なruntime errorとprocess error]
// }
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

// {
//   責務: [Flow: 文列実行の継続または関数からの戻りを伝える]
//   バリアント: [Continue: 次の文へ進む, Return: 関数戻り値の有無を保持する]
// }
#[derive(Debug, Clone)]
enum Flow {
    Continue,
    Return(Option<Value>),
}

// {
//   責務: [Runtime: DSL programの評価状態とdocumentへの操作を管理する]
//   フィールド: [program/document: 実行入力と編集対象, scopes: lexical binding stack]
//   フィールド: [objects: 実行時instance, events: 実行event, validation_rules: 設定履歴]
//   フィールド: [call_depth: 再帰深度, method_context: 現在のmethod class履歴]
// }
struct Runtime<'a> {
    program: &'a Program,
    document: &'a mut CsvDocument,
    scopes: Vec<HashMap<String, Binding>>,
    objects: Vec<ObjectInstance>,
    events: Vec<ExecutionEvent>,
    validation_rules: Vec<ValidationRuleDefinition>,
    call_depth: usize,
    method_context: Vec<String>,
}

impl<'a> Runtime<'a> {
    // {
    //   責務: [new: 空のglobal scopeと実行用stateでRuntimeを初期化する]
    //   引数: [program: 実行AST, document: 操作対象のmutable document]
    //   戻り値: [Runtime: 指定programとdocumentを保持した実行state]
    // }
    fn new(program: &'a Program, document: &'a mut CsvDocument) -> Self {
        Self {
            program,
            document,
            scopes: vec![HashMap::new()],
            objects: Vec::new(),
            events: Vec::new(),
            validation_rules: Vec::new(),
            call_depth: 0,
            method_context: Vec::new(),
        }
    }

    // {
    //   責務: [execute: top-level文を実行し、公開用ExecutionReportを構築する]
    //   処理: [return位置を検査し、global scalar値とglobal object直下のscalar fieldを抽出する]
    //   引数: [self: 初期化済みの実行state]
    //   戻り値: [Result<ExecutionReport, ExecutionError>: 実行eventと公開可能な結果]
    //   エラー: [ExecutionError: statement失敗またはtop-level Return]
    // }
    fn execute(mut self) -> Result<ExecutionReport, ExecutionError> {
        let program = self.program;
        if !matches!(
            self.execute_statements(&program.statements)?,
            Flow::Continue
        ) {
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
            validation_rules: self.validation_rules,
        })
    }

    // {
    //   責務: [execute_statements: 文列を順に評価し制御flowを伝播する]
    //   処理: [条件分岐、loop、宣言、代入、呼び出し、document操作を実行する]
    //   引数: [statements: 評価するAST文列]
    //   戻り値: [Result<Flow, ExecutionError>: 継続またはreturn値]
    //   エラー: [ExecutionError: 式評価、binding、object、document操作の失敗]
    // }
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
                Statement::SetValidationRule { selector, rule } => {
                    let column = resolve_column(selector, self.document)?;
                    let target = match selector {
                        ColumnSelector::Index(_) => ValidationTarget::Index(column),
                        ColumnSelector::Header(header) => ValidationTarget::Header(header.clone()),
                    };
                    self.document.set_validation_rule(target, rule.clone())?;
                    let definition = ValidationRuleDefinition {
                        selector: selector.clone(),
                        rule: rule.clone(),
                    };
                    self.validation_rules.push(definition.clone());
                    self.events
                        .push(ExecutionEvent::ValidationRuleSet { definition });
                }
            }
        }
        Ok(Flow::Continue)
    }

    // {
    //   責務: [evaluate_condition: 条件ASTをBooleanへ評価する]
    //   処理: [and/orは短絡評価し、column条件は現時点のdocument状態を参照する]
    //   引数: [condition: 評価する条件AST]
    //   戻り値: [Result<bool, ExecutionError>: 条件結果]
    //   エラー: [ExecutionError: 式型、列参照、または比較が不正]
    // }
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

    // {
    //   責務: [compare_values: DSL値を指定比較演算子で比較する]
    //   処理: [数値はInteger/Decimal間を比較し、等値比較では同型object identityも扱う]
    //   引数: [left: 左値, operator: 比較演算子, right: 右値]
    //   戻り値: [Result<bool, ExecutionError>: 比較結果]
    //   エラー: [ExecutionError: 順序比較できない値の組み合わせ]
    // }
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

    // {
    //   責務: [evaluate_expression: 式ASTを実行時Valueへ評価する]
    //   処理: [variable、call、standard namespace、object、field、arithmeticを解決する]
    //   引数: [expression: 評価する式AST]
    //   戻り値: [Result<Value, ExecutionError>: 式の評価値]
    //   エラー: [ExecutionError: 未定義名、戻り値なし、型不一致、または評価失敗]
    // }
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

    // {
    //   責務: [evaluate_unary: 単項演算子を実行時値へ適用する]
    //   引数: [operator: 単項演算子, value: 演算対象]
    //   戻り値: [Result<Value, ExecutionError>: 演算結果]
    //   エラー: [ExecutionError: 非数値への適用またはInteger符号反転overflow]
    // }
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

    // {
    //   責務: [evaluate_arithmetic: 算術演算子を数値値へ適用する]
    //   処理: [Integer演算はchecked arithmetic、Decimalを含む演算はfinite値を検査する]
    //   引数: [left: 左値, operator: 算術演算子, right: 右値]
    //   戻り値: [Result<Value, ExecutionError>: IntegerまたはDecimalの結果]
    //   エラー: [ExecutionError: 型不一致、0除算、または演算overflow]
    // }
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

    // {
    //   責務: [call_statement_builtin: transaction制御関数を文として処理する]
    //   処理: [引数なしを確認し、CsvDocumentのbegin/commit/rollbackへ委譲する]
    //   引数: [name: 呼び出し名, arguments: DSL引数式列]
    //   戻り値: [Result<bool, ExecutionError>: 対応builtinとして処理したか]
    //   エラー: [ExecutionError: 引数数またはdocument transaction操作の失敗]
    // }
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

    // {
    //   責務: [call_standard_namespace: Text/Number/Boolean標準関数を評価する]
    //   引数: [namespace: 標準namespace, name: 関数名, arguments: 引数式列]
    //   戻り値: [Result<Value, ExecutionError>: 標準関数の結果]
    //   エラー: [ExecutionError: 未知関数、引数数、引数型、または変換の失敗]
    // }
    fn call_standard_namespace(
        &mut self,
        namespace: StandardNamespace,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Value, ExecutionError> {
        let namespace_name = match namespace {
            StandardNamespace::Text => "Text",
            StandardNamespace::Number => "Number",
            StandardNamespace::Boolean => "Boolean",
        };
        let canonical = match namespace {
            StandardNamespace::Text => {
                if name.eq_ignore_ascii_case("contains") {
                    Some(("Contains", 2))
                } else if name.eq_ignore_ascii_case("startswith") {
                    Some(("StartsWith", 2))
                } else if name.eq_ignore_ascii_case("endswith") {
                    Some(("EndsWith", 2))
                } else if name.eq_ignore_ascii_case("isjapanese") {
                    Some(("IsJapanese", 1))
                } else {
                    None
                }
            }
            StandardNamespace::Number => {
                if name.eq_ignore_ascii_case("isinteger") {
                    Some(("IsInteger", 1))
                } else if name.eq_ignore_ascii_case("isdecimal") {
                    Some(("IsDecimal", 1))
                } else {
                    None
                }
            }
            StandardNamespace::Boolean => {
                if name.eq_ignore_ascii_case("isvalid") {
                    Some(("IsValid", 1))
                } else {
                    None
                }
            }
        };
        let (canonical, expected) =
            canonical.ok_or_else(|| ExecutionError::UnknownStandardFunction {
                namespace: namespace_name,
                name: name.to_owned(),
            })?;
        let full_name = format!("{namespace_name}.{canonical}");
        if arguments.len() != expected {
            return Err(ExecutionError::ArgumentCount {
                name: full_name,
                expected,
                actual: arguments.len(),
            });
        }

        let mut values = self.evaluate_arguments(arguments)?.into_iter();
        match (namespace, canonical) {
            (StandardNamespace::Text, "Contains") => {
                let haystack = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.Contains first argument",
                )?;
                let needle = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.Contains second argument",
                )?;
                Ok(Value::Boolean(haystack.contains(&needle)))
            }
            (StandardNamespace::Text, "StartsWith") => {
                let value = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.StartsWith first argument",
                )?;
                let prefix = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.StartsWith second argument",
                )?;
                Ok(Value::Boolean(value.starts_with(&prefix)))
            }
            (StandardNamespace::Text, "EndsWith") => {
                let value = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.EndsWith first argument",
                )?;
                let suffix = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.EndsWith second argument",
                )?;
                Ok(Value::Boolean(value.ends_with(&suffix)))
            }
            (StandardNamespace::Text, "IsJapanese") => {
                let value = self.expect_text(
                    values.next().expect("validated standard function arity"),
                    "Text.IsJapanese argument",
                )?;
                Ok(Value::Boolean(contains_japanese(&value)))
            }
            (StandardNamespace::Number, "IsInteger") => {
                let value = values.next().expect("validated standard function arity");
                Ok(Value::Boolean(is_integer_value(&value)))
            }
            (StandardNamespace::Number, "IsDecimal") => {
                let value = values.next().expect("validated standard function arity");
                Ok(Value::Boolean(is_decimal_value(&value)))
            }
            (StandardNamespace::Boolean, "IsValid") => {
                let value = values.next().expect("validated standard function arity");
                Ok(Value::Boolean(is_boolean_value(&value)))
            }
            _ => unreachable!(),
        }
    }

    // {
    //   責務: [call_builtin: 型変換、CSV読み書き、行列数等の組み込み関数を評価する]
    //   処理: [大文字小文字を無視して名前を解決し、引数評価後にdocument操作を行う]
    //   引数: [name: 呼び出し名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: builtin結果。対象外の名前はNone]
    //   エラー: [ExecutionError: 引数数、型、参照、変換、またはdocument操作の失敗]
    // }
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

        let mut values = self.evaluate_arguments(arguments)?.into_iter();
        Ok(Some(match canonical {
            "Integer" => {
                self.convert_integer(values.next().expect("validated builtin function arity"))?
            }
            "Decimal" => {
                self.convert_decimal(values.next().expect("validated builtin function arity"))?
            }
            "Boolean" => {
                self.convert_boolean(values.next().expect("validated builtin function arity"))?
            }
            "String" => {
                self.convert_string(values.next().expect("validated builtin function arity"))?
            }
            "CellValue" => {
                let reference = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "CellValue argument",
                )?;
                let value = self
                    .document
                    .cell_a1(&reference)?
                    .map(str::to_owned)
                    .ok_or(ExecutionError::MissingCell(reference))?;
                Value::Text(value)
            }
            "RowCount" => Value::Integer(self.document.row_count() as i64),
            "ColumnCount" => Value::Integer(self.document.column_count() as i64),
            "CellValueAt" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "CellValueAt row",
                )?;
                let column = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "CellValueAt column",
                )?;
                let value = self.document.cell(row - 1, column - 1).ok_or_else(|| {
                    ExecutionError::MissingCell(format!("row {row}, column {column}"))
                })?;
                Value::Text(value.to_owned())
            }
            "SetCellValueAt" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueAt row",
                )?;
                let column = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueAt column",
                )?;
                let value =
                    self.cell_text(values.next().expect("validated builtin function arity"))?;
                self.document.set_cell(row - 1, column - 1, value.clone())?;
                Value::Text(value)
            }
            "ColumnIndex" => {
                let header = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "ColumnIndex header",
                )?;
                let column = self.document.column_index_by_header(&header)?;
                Value::Integer((column + 1) as i64)
            }
            "CellValueByHeader" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "CellValueByHeader row",
                )?;
                let header = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "CellValueByHeader header",
                )?;
                let column = self.document.column_index_by_header(&header)?;
                let value = self.document.cell(row - 1, column).ok_or_else(|| {
                    ExecutionError::MissingCell(format!("row {row}, header {header}"))
                })?;
                Value::Text(value.to_owned())
            }
            "SetCellValueByHeader" => {
                let row = self.expect_one_based_index(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueByHeader row",
                )?;
                let header = self.expect_text(
                    values.next().expect("validated builtin function arity"),
                    "SetCellValueByHeader header",
                )?;
                let column = self.document.column_index_by_header(&header)?;
                let value =
                    self.cell_text(values.next().expect("validated builtin function arity"))?;
                self.document.set_cell(row - 1, column, value.clone())?;
                Value::Text(value)
            }
            _ => unreachable!(),
        }))
    }

    // {
    //   責務: [convert_integer: 値をIntegerへ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Integer値]
    //   エラー: [ExecutionError: Integerとして解釈できない値]
    // }
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

    // {
    //   責務: [convert_decimal: 値を有限Decimalへ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Decimal値]
    //   エラー: [ExecutionError: 数値化できない値または非有限数]
    // }
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

    // {
    //   責務: [convert_boolean: Boolean値またはtrue/false文字列をBooleanへ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Boolean値]
    //   エラー: [ExecutionError: true/falseとして解釈できない値]
    // }
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

    // {
    //   責務: [convert_string: object以外の値を表示文字列へ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Result<Value, ExecutionError>: Text値]
    //   エラー: [ExecutionError: objectは暗黙に文字列化しない]
    // }
    fn convert_string(&self, value: Value) -> Result<Value, ExecutionError> {
        match value {
            Value::Object(_) => Err(ExecutionError::ExpectedText {
                context: "String conversion".to_owned(),
            }),
            value => Ok(Value::Text(self.describe_value(&value))),
        }
    }

    // {
    //   責務: [instantiate_class: class instanceを生成し、親から子のfield初期化とInitを実行する]
    //   引数: [class_name: 生成するclass名, arguments: constructor引数式列]
    //   戻り値: [Result<Value, ExecutionError>: 新しいinstanceを指すObject値]
    //   エラー: [ExecutionError: class/継承解決、引数数、初期値式、constructorの失敗]
    // }
    fn instantiate_class(
        &mut self,
        class_name: &str,
        arguments: &[Expression],
    ) -> Result<Value, ExecutionError> {
        let class = self.class_by_name(class_name)?;
        let lineage = self.class_lineage(&class.name)?;
        let constructor = self.find_method_in_hierarchy(&class.name, "Init")?;
        let expected = constructor
            .as_ref()
            .map(|(_, constructor)| constructor.parameters.len())
            .unwrap_or(0);
        if expected != arguments.len() {
            return Err(ExecutionError::ConstructorArgumentCount {
                class_name: class.name.clone(),
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
                self.invoke_method_with_values(object_id, defining_class, constructor, values)?;
        }

        Ok(Value::Object(object_id))
    }

    // {
    //   責務: [call_function: top-level functionをlocal scopeで呼び出す]
    //   処理: [引数を評価し、呼び出しeventを記録してscopeと深度を復元する]
    //   引数: [name: 関数名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: Return値。明示ReturnがなければNone]
    //   エラー: [ExecutionError: 関数解決、引数数、深度上限、またはbody実行の失敗]
    // }
    fn call_function(
        &mut self,
        name: &str,
        arguments: &[Expression],
    ) -> Result<Option<Value>, ExecutionError> {
        self.ensure_call_depth()?;
        let program = self.program;
        let function = program
            .functions
            .iter()
            .find(|function| function.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| ExecutionError::UnknownFunction(name.to_owned()))?;
        if function.parameters.len() != arguments.len() {
            return Err(ExecutionError::ArgumentCount {
                name: function.name.clone(),
                expected: function.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        let argument_descriptions = values
            .iter()
            .map(|value| self.describe_value(value))
            .collect();
        let scope = function
            .parameters
            .iter()
            .zip(values)
            .map(|(parameter, value)| (normalize_identifier(parameter), Binding::variable(value)))
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
            name: function.name.clone(),
            arguments: argument_descriptions,
            return_value: return_value
                .as_ref()
                .map(|value| self.describe_value(value)),
        });
        Ok(return_value)
    }

    // {
    //   責務: [call_method: instanceのclass階層からmethodを解決して呼び出す]
    //   引数: [object_id: 呼び出し対象ID, name: method名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: Return値またはNone]
    //   エラー: [ExecutionError: instance、method、引数数、または実行失敗]
    // }
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
                name: method.name.clone(),
                expected: method.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        self.invoke_method_with_values(object_id, defining_class, method, values)
    }

    // {
    //   責務: [call_super_method: 現在のmethod定義元より上位からSuper methodを解決する]
    //   引数: [name: 親側method名, arguments: 引数式列]
    //   戻り値: [Result<Option<Value>, ExecutionError>: 親methodのReturn値]
    //   エラー: [ExecutionError: method外、親なし、method不明、引数数またはbodyの失敗]
    // }
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
            .as_deref()
            .ok_or_else(|| ExecutionError::NoSuperClass {
                class_name: current_class.clone(),
            })?;
        let object_id = self.resolve_object("Self")?;
        let (defining_class, method) =
            self.find_method_in_hierarchy(parent, name)?
                .ok_or_else(|| ExecutionError::UnknownSuperMethod {
                    class_name: current_class,
                    method: name.to_owned(),
                })?;
        if method.parameters.len() != arguments.len() {
            return Err(ExecutionError::MethodArgumentCount {
                class_name: defining_class.to_owned(),
                name: method.name.clone(),
                expected: method.parameters.len(),
                actual: arguments.len(),
            });
        }
        let values = self.evaluate_arguments(arguments)?;
        self.invoke_method_with_values(object_id, defining_class, method, values)
    }

    // {
    //   責務: [invoke_method_with_values: 評価済み引数でmethod本体を実行する]
    //   処理: [SelfをCONSTで束縛し、scope / method context / call depthを復元する]
    //   引数: [object_id: 対象instance, defining_class: method定義class, method: method AST, values: 引数値]
    //   戻り値: [Result<Option<Value>, ExecutionError>: methodのReturn値またはNone]
    //   エラー: [ExecutionError: 呼び出し深度またはmethod bodyの失敗]
    // }
    fn invoke_method_with_values(
        &mut self,
        object_id: ObjectId,
        defining_class: &str,
        method: &super::ast::FunctionDefinition,
        values: Vec<Value>,
    ) -> Result<Option<Value>, ExecutionError> {
        self.ensure_call_depth()?;
        let argument_descriptions = values
            .iter()
            .map(|value| self.describe_value(value))
            .collect();
        let mut scope: HashMap<String, Binding> = method
            .parameters
            .iter()
            .zip(values)
            .map(|(parameter, value)| (normalize_identifier(parameter), Binding::variable(value)))
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
            arguments: argument_descriptions,
            return_value: return_value
                .as_ref()
                .map(|value| self.describe_value(value)),
        });
        Ok(return_value)
    }

    // {
    //   責務: [class_by_name: 大文字小文字を無視してProgramからclassを検索する]
    //   引数: [name: 検索するclass名]
    //   戻り値: [Result<&ClassDefinition, ExecutionError>: class定義参照]
    //   エラー: [ExecutionError: classが存在しない]
    // }
    fn class_by_name(&self, name: &str) -> Result<&'a super::ast::ClassDefinition, ExecutionError> {
        self.program
            .classes
            .iter()
            .find(|class| class.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| ExecutionError::UnknownClass(name.to_owned()))
    }

    // {
    //   責務: [class_lineage: class自身からroot parentまでの継承列を構築する]
    //   引数: [class_name: 起点class名]
    //   戻り値: [Result<Vec<&ClassDefinition>, ExecutionError>: rootからchild順のclass列]
    //   エラー: [ExecutionError: class/parent不明または継承cycle]
    // }
    fn class_lineage(
        &self,
        class_name: &str,
    ) -> Result<Vec<&'a super::ast::ClassDefinition>, ExecutionError> {
        let mut lineage = Vec::new();
        let mut seen = Vec::new();
        let mut current = self.class_by_name(class_name)?;

        loop {
            let key = normalize_identifier(&current.name);
            if seen.iter().any(|name| name == &key) {
                return Err(ExecutionError::InheritanceCycle(current.name.clone()));
            }
            seen.push(key);
            lineage.push(current);

            let Some(parent_name) = current.parent.as_deref() else {
                break;
            };
            current = self
                .program
                .classes
                .iter()
                .find(|class| class.name.eq_ignore_ascii_case(parent_name))
                .ok_or_else(|| ExecutionError::UnknownParentClass {
                    class_name: current.name.clone(),
                    parent: parent_name.to_owned(),
                })?;
        }

        lineage.reverse();
        Ok(lineage)
    }

    // {
    //   責務: [find_method_in_hierarchy: 最も派生したclassからmethodを探す]
    //   引数: [class_name: 検索開始class名, method_name: method名]
    //   戻り値: [Result<Option<(&str, &FunctionDefinition)>, ExecutionError>: 定義元classとmethod]
    //   エラー: [ExecutionError: 継承列を解決できない]
    // }
    fn find_method_in_hierarchy(
        &self,
        class_name: &str,
        method_name: &str,
    ) -> Result<Option<(&'a str, &'a super::ast::FunctionDefinition)>, ExecutionError> {
        let lineage = self.class_lineage(class_name)?;
        Ok(lineage.iter().rev().find_map(|class| {
            class
                .methods
                .iter()
                .find(|method| method.name.eq_ignore_ascii_case(method_name))
                .map(|method| (class.name.as_str(), method))
        }))
    }

    // {
    //   責務: [evaluate_arguments: 引数式列を左から順番に評価する]
    //   引数: [arguments: 引数式列]
    //   戻り値: [Result<Vec<Value>, ExecutionError>: 評価済み引数値列]
    //   エラー: [ExecutionError: いずれかの引数評価が失敗]
    // }
    fn evaluate_arguments(
        &mut self,
        arguments: &[Expression],
    ) -> Result<Vec<Value>, ExecutionError> {
        arguments
            .iter()
            .map(|argument| self.evaluate_expression(argument))
            .collect()
    }

    // {
    //   責務: [ensure_call_depth: 次のfunction/method呼び出しが深度上限内か検査する]
    //   引数: [self: 現在のcall depthを持つruntime]
    //   戻り値: [Result<(), ExecutionError>: 上限未満ならOk(())]
    //   エラー: [ExecutionError: MAX_CALL_DEPTHに達している]
    // }
    fn ensure_call_depth(&self) -> Result<(), ExecutionError> {
        if self.call_depth >= MAX_CALL_DEPTH {
            Err(ExecutionError::CallDepthExceeded {
                limit: MAX_CALL_DEPTH,
            })
        } else {
            Ok(())
        }
    }

    // {
    //   責務: [resolve_object: variable名をobject instance IDへ解決する]
    //   引数: [variable: objectを保持するvariable名]
    //   戻り値: [Result<ObjectId, ExecutionError>: object配列の識別子]
    //   エラー: [ExecutionError: variableが不明またはobjectでない]
    // }
    fn resolve_object(&self, variable: &str) -> Result<ObjectId, ExecutionError> {
        match self.lookup_variable(variable) {
            Some(Value::Object(object_id)) => Ok(*object_id),
            Some(_) => Err(ExecutionError::ExpectedObject(variable.to_owned())),
            None => Err(ExecutionError::UnknownVariable(variable.to_owned())),
        }
    }

    // {
    //   責務: [get_field: objectから大小文字を無視してfield値を取得する]
    //   引数: [object_id: 対象object ID, field: field名]
    //   戻り値: [Result<Value, ExecutionError>: cloneしたfield値]
    //   エラー: [ExecutionError: object IDまたはfield名が不正]
    // }
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

    // {
    //   責務: [set_field: 既存fieldの値を更新する]
    //   引数: [object_id: 対象object ID, field: field名, value: 新しい値]
    //   戻り値: [Result<(), ExecutionError>: 更新成功時Ok(())]
    //   エラー: [ExecutionError: object IDまたはfield名が不正]
    // }
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

    // {
    //   責務: [object: IDからimmutableなinstance参照を取得する]
    //   引数: [object_id: objects配列の識別子]
    //   戻り値: [Result<&ObjectInstance, ExecutionError>: instance参照]
    //   エラー: [ExecutionError: IDがobjects配列の範囲外]
    // }
    fn object(&self, object_id: ObjectId) -> Result<&ObjectInstance, ExecutionError> {
        self.objects
            .get(object_id)
            .ok_or_else(|| ExecutionError::ExpectedObject(format!("object#{object_id}")))
    }

    // {
    //   責務: [object_mut: IDからmutableなinstance参照を取得する]
    //   引数: [object_id: objects配列の識別子]
    //   戻り値: [Result<&mut ObjectInstance, ExecutionError>: instanceのmutable参照]
    //   エラー: [ExecutionError: IDがobjects配列の範囲外]
    // }
    fn object_mut(&mut self, object_id: ObjectId) -> Result<&mut ObjectInstance, ExecutionError> {
        self.objects
            .get_mut(object_id)
            .ok_or_else(|| ExecutionError::ExpectedObject(format!("object#{object_id}")))
    }

    // {
    //   責務: [expect_integer: 値がIntegerか検査してi64を取り出す]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<i64, ExecutionError>: Integer値]
    //   エラー: [ExecutionError: 値がIntegerでない]
    // }
    fn expect_integer(&self, value: Value, context: &str) -> Result<i64, ExecutionError> {
        match value {
            Value::Integer(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedInteger {
                context: context.to_owned(),
            }),
        }
    }

    // {
    //   責務: [expect_one_based_index: 正のInteger値をusizeの1-based indexとして検証する]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<usize, ExecutionError>: 1以上のindex]
    //   エラー: [ExecutionError: Integerでない、1未満、またはusize範囲外]
    // }
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

    // {
    //   責務: [expect_text: 値がTextか検査してStringを取り出す]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<String, ExecutionError>: Text内容]
    //   エラー: [ExecutionError: 値がTextでない]
    // }
    fn expect_text(&self, value: Value, context: &str) -> Result<String, ExecutionError> {
        match value {
            Value::Text(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedText {
                context: context.to_owned(),
            }),
        }
    }

    // {
    //   責務: [expect_boolean: 値がBooleanか検査してboolを取り出す]
    //   引数: [value: 検査対象値, context: errorに示す用途]
    //   戻り値: [Result<bool, ExecutionError>: Boolean値]
    //   エラー: [ExecutionError: 値がBooleanでない]
    // }
    fn expect_boolean(&self, value: Value, context: &str) -> Result<bool, ExecutionError> {
        match value {
            Value::Boolean(value) => Ok(value),
            _ => Err(ExecutionError::ExpectedBoolean {
                context: context.to_owned(),
            }),
        }
    }

    // {
    //   責務: [cell_text: scalar値をCSV cellへ書けるtextへ変換する]
    //   引数: [value: cellへ書き込む値]
    //   戻り値: [Result<String, ExecutionError>: cell文字列]
    //   エラー: [ExecutionError: object値はCSV cellへ書き込めない]
    // }
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

    // {
    //   責務: [scalar_text: scalar値を実行レポート用文字列へ変換する]
    //   引数: [value: 変換対象値]
    //   戻り値: [Option<String>: scalarの文字列表現。objectはNone]
    // }
    fn scalar_text(&self, value: &Value) -> Option<String> {
        match value {
            Value::Text(value) => Some(value.clone()),
            Value::Integer(value) => Some(value.to_string()),
            Value::Decimal(value) => Some(value.to_string()),
            Value::Boolean(value) => Some(value.to_string()),
            Value::Object(_) => None,
        }
    }

    // {
    //   責務: [describe_value: eventとerrorに使う実行時値の文字列表現を作る]
    //   引数: [value: 説明する値]
    //   戻り値: [String: scalar表現またはclass名付きobject表現]
    // }
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

    // {
    //   責務: [lookup_variable: 現在scopeから外側へbindingを検索する]
    //   引数: [name: 検索するvariable名]
    //   戻り値: [Option<&Value>: 最も近いscopeの値]
    // }
    fn lookup_variable(&self, name: &str) -> Option<&Value> {
        let key = normalize_identifier(name);
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&key).map(|binding| &binding.value))
    }

    // {
    //   責務: [current_scope_mut: 宣言・代入対象の現在scopeを取得する]
    //   引数: [self: scope stackを持つruntime]
    //   戻り値: [&mut HashMap<String, Binding>: 最内側scope]
    //   前提: [Runtimeはglobal scopeを常に最低1つ保持する]
    // }
    fn current_scope_mut(&mut self) -> &mut HashMap<String, Binding> {
        self.scopes
            .last_mut()
            .expect("runtime always has at least the global scope")
    }
}

// {
//   責務: [contains_japanese: 文字列に対象の日本語Unicode範囲が含まれるか調べる]
//   引数: [value: 検査する文字列]
//   戻り値: [bool: かな、漢字等の対象範囲が含まれればtrue]
// }
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

// {
//   責務: [is_integer_value: 値がInteger、またはi64にparse可能なTextか判定する]
//   引数: [value: 検査対象値]
//   戻り値: [bool: Integerとして解釈できればtrue]
// }
fn is_integer_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) => true,
        Value::Text(value) => value.parse::<i64>().is_ok(),
        _ => false,
    }
}

// {
//   責務: [is_decimal_value: 値が有限数値型、またはfinite f64にparse可能なTextか判定する]
//   引数: [value: 検査対象値]
//   戻り値: [bool: Decimalとして解釈できればtrue]
// }
fn is_decimal_value(value: &Value) -> bool {
    match value {
        Value::Integer(_) | Value::Decimal(_) => true,
        Value::Text(value) => value.parse::<f64>().is_ok_and(|parsed| parsed.is_finite()),
        _ => false,
    }
}

// {
//   責務: [is_boolean_value: 値がBoolean、またはtrue/false Textか判定する]
//   引数: [value: 検査対象値]
//   戻り値: [bool: Booleanとして解釈できればtrue]
// }
fn is_boolean_value(value: &Value) -> bool {
    match value {
        Value::Boolean(_) => true,
        Value::Text(value) => {
            value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
        }
        _ => false,
    }
}

// {
//   責務: [compare_f64: 有限f64値を比較順序へ変換する]
//   引数: [left: 左値, right: 右値]
//   戻り値: [Ordering: 数値の比較順序]
//   前提: [Decimal値は生成時に有限値として検証される]
// }
fn compare_f64(left: f64, right: f64) -> Ordering {
    left.partial_cmp(&right)
        .expect("Rowly DSL decimal values are always finite")
}

// {
//   責務: [value_type: runtime値の型名をerror表示向けに返す]
//   引数: [value: 型名を調べる値]
//   戻り値: [&'static str: Text、Integer、Decimal、Boolean、Objectの型名]
// }
fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Text(_) => "String",
        Value::Integer(_) => "Integer",
        Value::Decimal(_) => "Decimal",
        Value::Boolean(_) => "Boolean",
        Value::Object(_) => "Object",
    }
}

// {
//   責務: [arithmetic_numbers: 数値value対をf64へ変換する]
//   引数: [left: 左値, right: 右値, operator: error表示用演算子]
//   戻り値: [Result<(f64, f64), ExecutionError>: 両辺の数値]
//   エラー: [ExecutionError: いずれかのoperandがInteger/Decimalでない]
// }
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

// {
//   責務: [arithmetic_operator_text: 算術演算子を記号表現へ変換する]
//   引数: [operator: DSL算術演算子]
//   戻り値: [&'static str: +、-、*、/のいずれか]
// }
fn arithmetic_operator_text(operator: ArithmeticOperator) -> &'static str {
    match operator {
        ArithmeticOperator::Add => "+",
        ArithmeticOperator::Subtract => "-",
        ArithmeticOperator::Multiply => "*",
        ArithmeticOperator::Divide => "/",
    }
}

// {
//   責務: [comparison_operator_text: 比較演算子を記号表現へ変換する]
//   引数: [operator: DSL比較演算子]
//   戻り値: [&'static str: 比較演算子の記号]
// }
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

// {
//   責務: [conversion_error: 型変換失敗の詳細を持つExecutionErrorを作成する]
//   引数: [value_type: 入力型名, value: 入力表示値, target: 変換先型名]
//   戻り値: [ExecutionError: 変換元と変換先を含むConversion error]
// }
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

// {
//   責務: [resolve_column: 列選択子をdocument内の0-based列indexへ解決する]
//   引数: [selector: 0-based indexまたはheader選択子, document: 列情報の参照元]
//   戻り値: [Result<usize, ColumnError>: document列index]
//   エラー: [ColumnError: indexまたはheaderが解決できない]
// }
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
