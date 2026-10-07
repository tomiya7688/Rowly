use std::collections::BTreeSet;

use thiserror::Error;

use crate::process::{CellRange, CellRef};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationError: binding登録・更新などengine構成時の失敗理由を表す]
/// ```
pub enum CalculationError {
    /// binding IDが空文字だった。
    #[error("calculation binding ID cannot be empty")]
    EmptyBindingId,
    /// 未実装のtarget種別を登録しようとした。
    #[error("calculation target is not supported yet")]
    UnsupportedTarget,
    /// cell targetが現在のCSVに存在しない。
    #[error("calculation target cell `{0}` does not exist")]
    MissingTarget(CellRef),
    /// 別のbindingが既に同じcellを所有している。
    #[error("cell `{0}` already has a calculation binding")]
    DuplicateTarget(CellRef),
    /// revision counterが上限に達した。
    #[error("calculation revision counter overflowed")]
    RevisionOverflow,
    /// 式がサポートする深さを超えた。
    #[error("calculation expression exceeds the supported depth")]
    ExpressionTooDeep,
}

#[derive(Debug, Clone, PartialEq)]
/// ```text
/// 責務: [CalculationBinding: target、pure expression、dependency、trigger、revisionを1つの規則として保持する]
/// フィールド: [id: 安定ID, target: 出力先, expression: 副作用なしAST, dependencies: 直接参照cell, trigger: 再評価条件, revision: 設定世代]
/// ```
pub struct CalculationBinding {
    pub(super) id: CalculationBindingId,
    pub(super) target: CalculationTarget,
    pub(super) expression: CalculationExpression,
    pub(super) dependencies: BTreeSet<CellRef>,
    pub(super) trigger: CalculationTrigger,
    pub(super) revision: u64,
}

impl CalculationBinding {
    /// ```text
    /// 責務: [id: calculation bindingのstable IDを返す]
    /// 引数: [self: 対象binding]
    /// 戻り値: [&CalculationBindingId: binding ID]
    /// ```
    pub fn id(&self) -> &CalculationBindingId {
        &self.id
    }

    /// ```text
    /// 責務: [target: bindingの出力targetを返す]
    /// 引数: [self: 対象binding]
    /// 戻り値: [CalculationTarget: 出力target]
    /// ```
    pub const fn target(&self) -> CalculationTarget {
        self.target
    }

    /// ```text
    /// 責務: [expression: immutable calculation expressionを返す]
    /// 引数: [self: 対象binding]
    /// 戻り値: [&CalculationExpression: 副作用なし式AST]
    /// ```
    pub fn expression(&self) -> &CalculationExpression {
        &self.expression
    }

    /// ```text
    /// 責務: [dependencies: 式が直接参照するcellを返す]
    /// 引数: [self: 対象binding]
    /// 戻り値: [&BTreeSet<CellRef>: row/column順の依存cell集合]
    /// ```
    pub fn dependencies(&self) -> &BTreeSet<CellRef> {
        &self.dependencies
    }

    /// ```text
    /// 責務: [trigger: bindingの自動再評価条件を返す]
    /// 引数: [self: 対象binding]
    /// 戻り値: [CalculationTrigger: dependency変更時または明示実行時の条件]
    /// ```
    pub const fn trigger(&self) -> CalculationTrigger {
        self.trigger
    }

    /// ```text
    /// 責務: [revision: binding設定のrevisionを返す]
    /// 引数: [self: 対象binding]
    /// 戻り値: [u64: binding設定を変更するたびに増えるrevision]
    /// ```
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// ```text
/// 責務: [CalculationBindingId: 計算規則を識別する安定した文字列IDを保持する]
/// フィールド: [value: 空でないbinding ID]
/// ```
pub struct CalculationBindingId(String);

impl CalculationBindingId {
    /// ```text
    /// 責務: [new: 空でない文字列からbinding IDを作成する]
    /// 引数: [value: bindingを識別する文字列]
    /// 戻り値: [Result<Self, CalculationError>: 検証済みIDまたは入力エラー]
    /// ```
    pub fn new(value: impl Into<String>) -> Result<Self, CalculationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(CalculationError::EmptyBindingId);
        }
        Ok(Self(value))
    }

    /// ```text
    /// 責務: [as_str: 保持するbinding ID文字列を返す]
    /// 引数: [self: 参照するID]
    /// 戻り値: [&str: ID文字列]
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// ```text
/// 責務: [CalculationTarget: 計算規則の結果を割り当てるcell、範囲、列相対領域を表す]
/// 補足: [初期計算engineはCellだけを評価し、他のtarget種別は将来拡張の境界として保持する]
/// ```
pub enum CalculationTarget {
    /// 結果を1つの既存cellへ割り当てる。
    Cell(CellRef),
    /// 結果を矩形rangeへ割り当てる。
    Range(CellRange),
    /// 指定列のfirst_data_row以降へ行相対ruleを割り当てる。
    ColumnRelative {
        /// zero-basedの対象列。
        column: usize,
        /// zero-basedの最初の対象行。
        first_data_row: usize,
    },
}

impl CalculationTarget {
    /// ```text
    /// 責務: [single_cell: 単一cell targetの場合だけ参照位置を返す]
    /// 引数: [self: 計算target]
    /// 戻り値: [Option<CellRef>: 単一cell位置、または未対応target]
    /// ```
    pub const fn single_cell(self) -> Option<CellRef> {
        match self {
            Self::Cell(reference) => Some(reference),
            Self::Range(_) | Self::ColumnRelative { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationTrigger: dependency変更で自動評価するか明示Recalculateだけにするか表す]
/// ```
pub enum CalculationTrigger {
    /// 依存cellが変化したときに再評価する。
    DependencyChange,
    /// 明示Recalculate時だけ評価する。
    Manual,
}

#[derive(Debug, Clone, PartialEq)]
/// ```text
/// 責務: [CalculationValue: 計算式内のtext、integer、finite decimal値を表す]
/// 補足: [CSV cell参照は最初にTextとして読み、算術式で数値へ変換する]
/// ```
pub enum CalculationValue {
    /// CSV由来または明示された文字列値。
    Text(String),
    /// 64-bit整数値。
    Integer(i64),
    /// 有限の浮動小数点値。
    Decimal(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationOperator: pure calculation expressionで使う算術演算子を表す]
/// ```
pub enum CalculationOperator {
    /// 左右を加算する。
    Add,
    /// 右辺を左辺から減算する。
    Subtract,
    /// 左右を乗算する。
    Multiply,
    /// 左辺を右辺で除算する。
    Divide,
}

#[derive(Debug, Clone, PartialEq)]
/// ```text
/// 責務: [CalculationExpression: literal、cell read、算術、pure functionだけからなる安全な式ASTを表す]
/// 補足: [式はFile/Project I/O、network、process実行、document writeを表現できない]
/// ```
pub enum CalculationExpression {
    /// 式中に埋め込むscalar literal。
    Literal(CalculationValue),
    /// canonical CSVから値を読み取るcell参照。
    Cell(CellRef),
    /// 算術演算前に数値へ評価する符号反転。
    Negate(Box<CalculationExpression>),
    /// 左右の数値式を計算する。
    Arithmetic {
        /// 左辺の式。
        left: Box<CalculationExpression>,
        /// 適用する演算子。
        operator: CalculationOperator,
        /// 右辺の式。
        right: Box<CalculationExpression>,
    },
    /// 固定された副作用なし関数を呼び出す。
    Call {
        /// 許可する標準pure function。
        function: PureCalculationFunction,
        /// functionへ渡す式。
        arguments: Vec<CalculationExpression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [PureCalculationFunction: calculation engineが許可する副作用なしscalar functionを表す]
/// ```
pub enum PureCalculationFunction {
    /// 数値の絶対値を返す。
    Abs,
    /// 2つの数値の小さい方を返す。
    Min,
    /// 2つの数値の大きい方を返す。
    Max,
}

impl CalculationExpression {
    /// ```text
    /// 責務: [dependencies: 式が直接読むcanonical CSV cell位置を重複なく列挙する]
    /// 引数: [self: 走査するexpression]
    /// 戻り値: [BTreeSet<CellRef>: row/column順に安定した依存cell集合]
    /// ```
    pub fn dependencies(&self) -> BTreeSet<CellRef> {
        let mut pending = vec![self];
        let mut dependencies = BTreeSet::new();

        while let Some(expression) = pending.pop() {
            match expression {
                Self::Literal(_) => {}
                Self::Cell(reference) => {
                    dependencies.insert(*reference);
                }
                Self::Negate(operand) => pending.push(operand),
                Self::Arithmetic { left, right, .. } => {
                    pending.push(left);
                    pending.push(right);
                }
                Self::Call { arguments, .. } => {
                    pending.extend(arguments.iter());
                }
            }
        }

        dependencies
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationFailure: bindingの評価失敗理由をGUIや呼び出し元へ安全に報告する]
/// ```
pub enum CalculationFailure {
    /// targetまたはdependency cellが現在のCSVに存在しない。
    MissingCell(CellRef),
    /// 演算子へ数値以外の値を渡した。
    ExpectedNumber(String),
    /// 除数が0だった。
    DivisionByZero,
    /// checked arithmeticまたはfinite値検証でoverflowした。
    ArithmeticOverflow,
    /// 標準関数の引数個数が規定と異なる。
    InvalidFunctionArguments {
        /// function名。
        function: &'static str,
        /// 期待する引数数。
        expected: usize,
        /// 実際の引数数。
        actual: usize,
    },
    /// dependency先bindingがstaleまたはerror状態にある。
    DependencyUnavailable(CalculationBindingId),
    /// dependency graphに循環または循環依存がある。
    CycleDetected,
    /// 評価式が安全な最大深さを超えた。
    ExpressionTooDeep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationStatus: bindingが未評価、最新、stale、またはerrorかを表す]
/// ```
pub enum CalculationStatus {
    /// まだ評価されていない。
    Pending,
    /// valueが現在のdependency snapshotに対して評価済み。
    Evaluated,
    /// 依存値は変わったがbindingのtriggerがManualなので再評価を待つ。
    Stale,
    /// 評価できなかった理由。
    Error(CalculationFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationDependencySnapshot: 評価時にbindingが読んだraw cellと派生binding revisionを記録する]
/// ```
pub struct CalculationDependencySnapshot {
    /// 参照したzero-based cell位置。
    pub cell: CellRef,
    /// CSV内の現在のraw string。存在しないcellはNone。
    pub raw_value: Option<String>,
    /// 参照先にbindingがある場合のbinding revision。
    pub binding_revision: Option<u64>,
    /// 参照先にbindingがある場合のderived result revision。
    pub derived_revision: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
/// ```text
/// 責務: [CalculationResult: canonical CSVと分離したderived value、評価状態、revision、dependency snapshotを保持する]
/// ```
pub struct CalculationResult {
    /// 評価成功時のCSV向け文字列。error時はNone。
    pub value: Option<String>,
    /// 評価状態。
    pub status: CalculationStatus,
    /// このresultを生成したbinding revision。
    pub binding_revision: u64,
    /// resultの変更ごとに増えるengine revision。
    pub derived_revision: u64,
    /// 評価またはstale化時に確認したdependency snapshot。
    pub dependencies: Vec<CalculationDependencySnapshot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationRecalculationReport: recalculate操作で評価、stale化、失敗したbinding IDを分類する]
/// ```
pub struct CalculationRecalculationReport {
    /// 評価に成功したbinding IDs。
    pub evaluated: Vec<CalculationBindingId>,
    /// triggerにより再評価を待つbinding IDs。
    pub stale: Vec<CalculationBindingId>,
    /// errorになったbinding IDs。
    pub failed: Vec<CalculationBindingId>,
}

impl std::fmt::Display for CalculationFailure {
    /// ```text
    /// 責務: [fmt: calculation failureを利用者が読めるerror messageへ変換する]
    /// 引数: [self: failure理由, formatter: 出力先]
    /// 戻り値: [std::fmt::Result: formatterへの書き込み結果]
    /// ```
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCell(reference) => write!(formatter, "cell `{reference}` does not exist"),
            Self::ExpectedNumber(value) => write!(formatter, "value `{value}` is not numeric"),
            Self::DivisionByZero => formatter.write_str("division by zero"),
            Self::ArithmeticOverflow => formatter.write_str("calculation overflowed"),
            Self::InvalidFunctionArguments {
                function,
                expected,
                actual,
            } => write!(
                formatter,
                "function `{function}` expects {expected} arguments but received {actual}"
            ),
            Self::DependencyUnavailable(binding_id) => write!(
                formatter,
                "dependency binding `{}` has no current result",
                binding_id.as_str()
            ),
            Self::CycleDetected => formatter.write_str("calculation dependency cycle detected"),
            Self::ExpressionTooDeep => {
                formatter.write_str("calculation expression exceeds the supported depth")
            }
        }
    }
}
