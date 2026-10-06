use super::CellRef;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// ```text
/// 責務: [
/// ValidationTarget: validation ruleの対象列をindexまたはheader名で指定する
/// ]
/// 補足: [
/// Indexはzero-based column index、Headerは一意な列名で対象を示す
/// ]
/// ```
pub enum ValidationTarget {
    Index(usize),
    Header(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ValidationComparisonOperator: 文字列比較式で使う比較演算を表す
/// ]
/// 補足: [
/// Equal/NotEqual: 一致・不一致
/// Less/LessOrEqual: 辞書順で小さい・以下
/// Greater/GreaterOrEqual: 辞書順で大きい・以上
/// ]
/// ```
pub enum ValidationComparisonOperator {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ValidationOperand: 比較式内で候補cell値または固定文字列を指定する
/// ]
/// ```
pub enum ValidationOperand {
    Value,
    Literal(String),
}

impl ValidationOperand {
    // ```text
    // 責務: [
    // resolve: operandを候補cell値または保持するliteralへ解決する
    // ]
    // 処理: [
    // 1: Valueならcandidate、Literalなら保持文字列を返す
    // ]
    // 引数: [
    // self: 解決する式operand
    // candidate: 検証対象cell値
    // ]
    // 戻り値: [
    // &str: 候補値またはliteral値へのborrow
    // ]
    // ```
    fn resolve<'a>(&'a self, candidate: &'a str) -> &'a str {
        match self {
            Self::Value => candidate,
            Self::Literal(value) => value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ValidationExpression: cell文字列値に適用する比較・論理式を表す
/// ]
/// 補足: [
/// Compare: 2 operandの文字列順序を比較する
/// Not: 子式の結果を反転する
/// And / Or: 左右の式結果を論理結合する
/// ]
/// ```
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
    /// ```text
    /// 責務: [
    /// evaluate: candidate文字列に式を適用し、一致可否を返す
    /// ]
    /// 処理: [
    /// 1: 比較式のoperandを候補値またはliteralへ解決する
    /// 2: 文字列の辞書順比較または論理演算を再帰評価する
    /// ]
    /// 引数: [
    /// self: 評価する式木
    /// candidate: 検証対象のcell文字列
    /// ]
    /// 戻り値: [
    /// bool: 式が成立すればtrue
    /// ]
    /// ```
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
/// ```text
/// 責務: [
/// ValidationRule: 候補cell値を許可値一覧または式で検証する規則
/// ]
/// ```
pub enum ValidationRule {
    AllowedValues(Vec<String>),
    Expression(ValidationExpression),
}

impl ValidationRule {
    /// ```text
    /// 責務: [
    /// matches: 候補値が許可値一覧または式を満たすか判定する
    /// ]
    /// 処理: [
    /// 1: 許可値と完全一致するか、式をcandidateへ適用して判定する
    /// ]
    /// 引数: [
    /// self: 適用するrule
    /// candidate: 検証するcell文字列
    /// ]
    /// 戻り値: [
    /// bool: ruleを満たす場合true
    /// ]
    /// ```
    pub fn matches(&self, candidate: &str) -> bool {
        match self {
            Self::AllowedValues(values) => values.iter().any(|value| value == candidate),
            Self::Expression(expression) => expression.evaluate(candidate),
        }
    }

    /// ```text
    /// 責務: [
    /// allowed_values: AllowedValues ruleの許可文字列一覧を返す
    /// ]
    /// 処理: [
    /// 1: AllowedValuesならslice、ExpressionならNoneを返す
    /// ]
    /// 引数: [
    /// self: 対象rule
    /// ]
    /// 戻り値: [
    /// Option<&[String]>: 許可値slice、別形式のruleならNone
    /// ]
    /// ```
    pub fn allowed_values(&self) -> Option<&[String]> {
        match self {
            Self::AllowedValues(values) => Some(values),
            Self::Expression(_) => None,
        }
    }

    /// ```text
    /// 責務: [
    /// expression: Expression ruleの式を返す
    /// ]
    /// 処理: [
    /// 1: Expressionなら式への参照、AllowedValuesならNoneを返す
    /// ]
    /// 引数: [
    /// self: 対象rule
    /// ]
    /// 戻り値: [
    /// Option<&ValidationExpression>: 式への参照、別形式のruleならNone
    /// ]
    /// ```
    pub fn expression(&self) -> Option<&ValidationExpression> {
        match self {
            Self::AllowedValues(_) => None,
            Self::Expression(expression) => Some(expression),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ValidationViolation: ruleに適合しなかったcellと対象規則の情報を保持する
/// ]
/// フィールド: [
/// cell: 違反cellの位置
/// target: 適用された列対象指定
/// value: 違反時点のcell文字列値
/// rule: cellに適用されたvalidation rule
/// ]
/// ```
pub struct ValidationViolation {
    pub(super) cell: CellRef,
    pub(super) target: ValidationTarget,
    pub(super) value: String,
    pub(super) rule: ValidationRule,
}

impl ValidationViolation {
    /// ```text
    /// 責務: [
    /// cell: 違反したcellのzero-based位置を返す
    /// ]
    /// 処理: [
    /// 1: 保持しているcell参照を返す
    /// ]
    /// 引数: [
    /// self: 対象違反
    /// ]
    /// 戻り値: [
    /// CellRef: 違反cell位置
    /// ]
    /// ```
    pub fn cell(&self) -> CellRef {
        self.cell
    }

    /// ```text
    /// 責務: [
    /// target: 違反cellに適用された列指定を返す
    /// ]
    /// 処理: [
    /// 1: 保持しているtargetへのborrowを返す
    /// ]
    /// 引数: [
    /// self: 対象違反
    /// ]
    /// 戻り値: [
    /// &ValidationTarget: column indexまたはheader指定
    /// ]
    /// ```
    pub fn target(&self) -> &ValidationTarget {
        &self.target
    }

    /// ```text
    /// 責務: [
    /// value: 違反時点のcell文字列値を返す
    /// ]
    /// 処理: [
    /// 1: 保持している値へのborrowを返す
    /// ]
    /// 引数: [
    /// self: 対象違反
    /// ]
    /// 戻り値: [
    /// &str: ruleに適合しなかった値
    /// ]
    /// ```
    pub fn value(&self) -> &str {
        &self.value
    }

    /// ```text
    /// 責務: [
    /// rule: 違反cellへ適用されたruleを返す
    /// ]
    /// 処理: [
    /// 1: 保持しているruleへのborrowを返す
    /// ]
    /// 引数: [
    /// self: 対象違反
    /// ]
    /// 戻り値: [
    /// &ValidationRule: 違反原因となったrule
    /// ]
    /// ```
    pub fn rule(&self) -> &ValidationRule {
        &self.rule
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// ```text
/// 責務: [
/// ValidationReport: validationで確認したcell数と違反一覧を保持する
/// ]
/// フィールド: [
/// checked_cells: rule判定したcell数
/// violations: ruleに適合しなかったcell一覧
/// ]
/// ```
pub struct ValidationReport {
    pub(super) checked_cells: usize,
    pub(super) violations: Vec<ValidationViolation>,
}

impl ValidationReport {
    /// ```text
    /// 責務: [
    /// checked_cells: rule判定したcell数を返す
    /// ]
    /// 処理: [
    /// 1: 保持している判定件数を返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// usize: 判定対象cell数
    /// ]
    /// ```
    pub fn checked_cells(&self) -> usize {
        self.checked_cells
    }

    /// ```text
    /// 責務: [
    /// violations: validation違反の一覧を返す
    /// ]
    /// 処理: [
    /// 1: 違反一覧へのborrowを返す
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// &[ValidationViolation]: 違反cellと適用ruleの参照slice
    /// ]
    /// ```
    pub fn violations(&self) -> &[ValidationViolation] {
        &self.violations
    }

    /// ```text
    /// 責務: [
    /// is_valid: validation違反がないかを判定する
    /// ]
    /// 処理: [
    /// 1: 違反一覧が空かを調べる
    /// ]
    /// 引数: [
    /// self: 対象レポート
    /// ]
    /// 戻り値: [
    /// bool: 違反一覧が空ならtrue
    /// ]
    /// ```
    pub fn is_valid(&self) -> bool {
        self.violations.is_empty()
    }
}
