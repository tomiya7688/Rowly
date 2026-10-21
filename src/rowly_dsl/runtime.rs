use std::cmp::Ordering;
use std::collections::HashMap;

use thiserror::Error;

use crate::process::calculation::{
    CalculationBindingId, CalculationEngine, CalculationError, CalculationExpression,
    CalculationOperator, CalculationTarget, CalculationTrigger, CalculationValue,
    PureCalculationFunction,
};
use crate::process::{CellRef, ColumnError, CsvDocument, DocumentError, ValidationTarget};

use super::ast::{
    ArithmeticOperator, CalculationBindingDefinition, ColumnSelector, ComparisonOperator,
    Condition, DeclarationKind, ExecutionEvent, ExecutionReport, Expression, Program,
    StandardNamespace, Statement, UnaryOperator, ValidationRuleDefinition, normalize_identifier,
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
    Runtime::new(program, document, None).execute()
}

/// ```text
/// 責務: [execute_with_calculation_engine: DSLを実行し計算規則をengineへ登録する]
/// 処理: [通常のdocument commandを実行し、pure calculation宣言をCSV外のengineへ追加する]
/// 引数: [program: 実行AST, document: canonical CSV, engine: bindingを保持するcalculation engine]
/// 戻り値: [Result<ExecutionReport, ExecutionError>: 実行eventと宣言snapshot]
/// 副作用: [document commandとengine bindingを更新する。計算結果は再評価しない]
/// エラー: [ExecutionError: DSL、documentまたはcalculation bindingの失敗]
/// ```
pub fn execute_with_calculation_engine(
    program: &Program,
    document: &mut CsvDocument,
    engine: &mut CalculationEngine,
) -> Result<ExecutionReport, ExecutionError> {
    Runtime::new(program, document, Some(engine)).execute()
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
    #[error(transparent)]
    Calculation(#[from] CalculationError),
    #[error("invalid calculation expression: {0}")]
    InvalidCalculationExpression(String),
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
//   フィールド: [objects: 実行時instance, events: 実行event, validation_rules: validation宣言, calculation_bindings: calculation宣言]
//   フィールド: [call_depth: 再帰深度, method_context: 現在のmethod class履歴]
// }
struct Runtime<'a> {
    program: &'a Program,
    document: &'a mut CsvDocument,
    calculation_engine: Option<&'a mut CalculationEngine>,
    scopes: Vec<HashMap<String, Binding>>,
    objects: Vec<ObjectInstance>,
    events: Vec<ExecutionEvent>,
    validation_rules: Vec<ValidationRuleDefinition>,
    calculation_bindings: Vec<CalculationBindingDefinition>,
    call_depth: usize,
    method_context: Vec<String>,
}

impl<'a> Runtime<'a> {
    // {
    //   責務: [new: 空のglobal scopeと実行用stateでRuntimeを初期化する]
    //   引数: [program: 実行AST, document: 操作対象のmutable document]
    //   戻り値: [Runtime: 指定programとdocumentを保持した実行state]
    // }
    fn new(
        program: &'a Program,
        document: &'a mut CsvDocument,
        calculation_engine: Option<&'a mut CalculationEngine>,
    ) -> Self {
        Self {
            program,
            document,
            calculation_engine,
            scopes: vec![HashMap::new()],
            objects: Vec::new(),
            events: Vec::new(),
            validation_rules: Vec::new(),
            calculation_bindings: Vec::new(),
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
            calculation_bindings: self.calculation_bindings,
        })
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

mod builtin;
mod calculation;
mod expression;
mod object;
mod statement;
mod value;

use calculation::*;
use value::*;
