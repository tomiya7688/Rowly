//! DSL実行結果のreportとeventを定義する。

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [ExecutionReport: 記録対象のDSL eventと最終global scalar stateのread-only snapshot]
/// フィールド: [events: 実行順の記録event, variables: 最終global scalar variable値, object_fields: 最終global objectのscalar field値, validation_rules: 実行時に適用されたrule宣言, calculation_bindings: 宣言された計算規則]
/// ```
pub struct ExecutionReport {
    /// statement実行中に生成されたeventをsource実行順で保持する。
    pub(in crate::rowly_dsl) events: Vec<ExecutionEvent>,
    /// 実行完了時に可視のvariable値。
    pub(in crate::rowly_dsl) variables: HashMap<String, String>,
    /// global bindingされたobjectごとの実行完了時scalar field値。
    pub(in crate::rowly_dsl) object_fields: HashMap<String, HashMap<String, String>>,
    /// validation ruleをstatement実行順で保持する。
    pub(in crate::rowly_dsl) validation_rules: Vec<ValidationRuleDefinition>,
    /// calculation binding宣言をsource順に保持する。
    pub(in crate::rowly_dsl) calculation_bindings: Vec<CalculationBindingDefinition>,
}

impl ExecutionReport {
    /// DSL実行eventを実行順で返す。
    pub fn events(&self) -> &[ExecutionEvent] {
        &self.events
    }

    /// 実行されたvalidation ruleをstatement実行順で返す。
    pub fn validation_rules(&self) -> &[ValidationRuleDefinition] {
        &self.validation_rules
    }

    /// 実行されたcalculation binding宣言をsource順に返す。
    pub fn calculation_bindings(&self) -> &[CalculationBindingDefinition] {
        &self.calculation_bindings
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
    CalculationBindingSet {
        definition: CalculationBindingDefinition,
    },
    CalculationBindingTriggerSet {
        target: CalculationTarget,
        trigger: CalculationTrigger,
    },
}
