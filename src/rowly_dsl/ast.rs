use std::collections::HashMap;

use crate::process::{CellRange, ColumnType, ColumnTypeReport, JapaneseCheckReport};

pub use crate::process::{ValidationExpression, ValidationOperand, ValidationRule};

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [Program: parserが生成するDSL program全体のAST]
/// フィールド: [classes: class宣言, functions: function宣言, statements: 実行statement列]
/// ```
pub struct Program {
    /// source順に収集したclass宣言。
    pub(super) classes: Vec<ClassDefinition>,
    /// source順に収集したfunction宣言。
    pub(super) functions: Vec<FunctionDefinition>,
    /// top-levelで実行するstatement列。
    pub(super) statements: Vec<Statement>,
}

impl Program {
    /// Program内のclass宣言をsource順で返す。
    pub fn classes(&self) -> &[ClassDefinition] {
        &self.classes
    }

    /// Program内のfunction宣言をsource順で返す。
    pub fn functions(&self) -> &[FunctionDefinition] {
        &self.functions
    }

    /// Program内のtop-level statement列を返す。
    pub fn statements(&self) -> &[Statement] {
        &self.statements
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [ClassDefinition: class名、親class、field初期値、method宣言を保持する]
/// フィールド: [name: class名, parent: 任意の親class名, fields: field宣言, methods: method宣言]
/// ```
pub struct ClassDefinition {
    /// classの識別名。
    pub(super) name: String,
    /// 継承元class名。継承しない場合はNone。
    pub(super) parent: Option<String>,
    /// classが宣言するfield初期値。
    pub(super) fields: Vec<FieldDefinition>,
    /// classが宣言するmethod。
    pub(super) methods: Vec<FunctionDefinition>,
}

impl ClassDefinition {
    /// class名を返す。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 継承元class名を返す。
    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    /// classのfield初期値宣言を返す。
    pub fn fields(&self) -> &[FieldDefinition] {
        &self.fields
    }

    /// classのmethod宣言を返す。
    pub fn methods(&self) -> &[FunctionDefinition] {
        &self.methods
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [FieldDefinition: field名と初期化expressionを保持する]
/// フィールド: [name: field名, default: instance生成時に評価する初期値expression]
/// ```
pub struct FieldDefinition {
    /// field名。
    pub(super) name: String,
    /// instance生成時に評価する初期値。
    pub(super) default: Expression,
}

impl FieldDefinition {
    /// field名を返す。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 初期値expressionを返す。
    pub fn default(&self) -> &Expression {
        &self.default
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [FunctionDefinition: function / method名、parameter、bodyを保持する]
/// フィールド: [name: 宣言名, parameters: parameter名列, body: 実行statement列]
/// ```
pub struct FunctionDefinition {
    /// functionまたはmethodの名前。
    pub(super) name: String,
    /// 宣言順のparameter名。
    pub(super) parameters: Vec<String>,
    /// functionまたはmethod body。
    pub(super) body: Vec<Statement>,
}

impl FunctionDefinition {
    /// functionまたはmethod名を返す。
    pub fn name(&self) -> &str {
        &self.name
    }

    /// 宣言順のparameter名を返す。
    pub fn parameters(&self) -> &[String] {
        &self.parameters
    }

    /// functionまたはmethod bodyを返す。
    pub fn body(&self) -> &[Statement] {
        &self.body
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [DeclarationKind: variable bindingの再代入可否を表す宣言種別]
/// 補足: [CONSTはbindingを保護する。object fieldを再帰的に凍結しない]
/// ```
pub enum DeclarationKind {
    /// 再代入可能なvariable binding。
    Var,
    /// 再代入できないvariable binding。
    Const,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [StandardNamespace: DSL組込みpredicateの名前空間]
/// 補足: [名前空間内の関数解決はuser object method呼出しと区別される]
/// ```
pub enum StandardNamespace {
    /// text値の判定関数。
    Text,
    /// numeric値の判定関数。
    Number,
    /// boolean値の判定関数。
    Boolean,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [Statement: DSLが実行する制御、binding、object、document commandを表すAST node]
/// 補足: [各variantのpayloadはparser/runtime間で共有する意味情報]
/// ```
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
/// ```text
/// 責務: [UnaryOperator: 単項expressionに適用する演算子]
/// ```
pub enum UnaryOperator {
    /// 数値expressionの符号を反転する。
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [ArithmeticOperator: 左右のnumeric expressionを結合する演算子]
/// ```
pub enum ArithmeticOperator {
    /// numeric addition。
    Add,
    /// numeric subtraction。
    Subtract,
    /// numeric multiplication。
    Multiply,
    /// numeric division。
    Divide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [Expression: DSLが評価するliteral、binding参照、演算、関数、object参照を表すAST]
/// 補足: [StandardCallは予約namespace経由の組込みpredicate呼出しを表す]
/// ```
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
/// ```text
/// 責務: [ComparisonOperator: 2つの比較可能なexpressionを比較する演算子]
/// ```
pub enum ComparisonOperator {
    /// 左右の値が等しいか比較する。
    Equal,
    /// 左右の値が異なるか比較する。
    NotEqual,
    /// 左辺が右辺より小さいか比較する。
    Less,
    /// 左辺が右辺以下か比較する。
    LessOrEqual,
    /// 左辺が右辺より大きいか比較する。
    Greater,
    /// 左辺が右辺以上か比較する。
    GreaterOrEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [Condition: statement分岐に使うcolumn状態 / comparison / boolean conditionを表す]
/// ```
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
/// ```text
/// 責務: [ColumnSelector: DSLからcolumnを指定するindexまたはheader名を表す]
/// 補足: [DSL column番号は1-based。Index payloadはCsvDocument APIへ渡すzero-based index]
/// ```
pub enum ColumnSelector {
    /// zero-based column index。parserがDSLの1-based番号から変換する。
    Index(usize),
    /// exact matchで検索するheader text。
    Header(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [ValidationRuleDefinition: column selectorと設定ruleをまとめる]
/// フィールド: [selector: 対象column, rule: 適用するvalidation rule]
/// ```
pub struct ValidationRuleDefinition {
    /// ruleを適用するcolumn。
    pub(super) selector: ColumnSelector,
    /// columnへ設定するrule。
    pub(super) rule: ValidationRule,
}

impl ValidationRuleDefinition {
    /// validation ruleの対象columnを返す。
    pub fn selector(&self) -> &ColumnSelector {
        &self.selector
    }

    /// 適用するvalidation ruleを返す。
    pub fn rule(&self) -> &ValidationRule {
        &self.rule
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [ExecutionReport: 記録対象のDSL eventと最終global scalar stateのread-only snapshot]
/// フィールド: [events: 実行順の記録event, variables: 最終global scalar variable値, object_fields: 最終global objectのscalar field値, validation_rules: source順rule宣言]
/// ```
pub struct ExecutionReport {
    /// statement実行中に生成されたeventをsource実行順で保持する。
    pub(super) events: Vec<ExecutionEvent>,
    /// 実行完了時に可視のvariable値。
    pub(super) variables: HashMap<String, String>,
    /// global bindingされたobjectごとの実行完了時scalar field値。
    pub(super) object_fields: HashMap<String, HashMap<String, String>>,
    /// validation rule宣言をsource実行順で保持する。
    pub(super) validation_rules: Vec<ValidationRuleDefinition>,
}

impl ExecutionReport {
    /// DSL実行eventを実行順で返す。
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    /// validation declarationをsource実行順で返す。
    pub fn validation_rules(&self) -> &[ValidationRuleDefinition] {
        &self.validation_rules
    }

    /// variable名をASCII case-insensitiveで検索し、final global scalar値を返す。
    pub fn variable(&self, name: &str) -> Option<&str> {
        self.variables
            .get(&normalize_identifier(name))
            .map(String::as_str)
    }

    /// global object variable名とfield名をASCII case-insensitiveで検索し、scalar field値を返す。nested object fieldは含まない。
    pub fn object_field(&self, variable: &str, field: &str) -> Option<&str> {
        self.object_fields
            .get(&normalize_identifier(variable))?
            .get(&normalize_identifier(field))
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [ExecutionEvent: runtimeが実行順で記録するDSL eventのsubsetを表す]
/// 補足: [eventはExecutionReport内に格納される。transaction controlはこのenumでは記録しない]
/// ```
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

// {
//   責務: [normalize_identifier: DSL identifierをASCII case-insensitive lookup用に正規化する]
//   引数: [identifier: 正規化するidentifier]
//   戻り値: [String: ASCII lowercaseへ変換したidentifier]
// }
pub(super) fn normalize_identifier(identifier: &str) -> String {
    identifier.to_ascii_lowercase()
}
