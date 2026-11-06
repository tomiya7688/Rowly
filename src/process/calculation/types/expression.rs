//! 計算式ASTと式の値・演算子・DSL文字列化を定義する。

use super::*;

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
    /// 比較・論理演算の結果。数値やTextへ暗黙に変換しない。
    Boolean(bool),
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
    /// ` $ `で相対・絶対を指定したcell参照（少なくとも1軸が絶対）。
    AnchoredCell(CalculationCellReference),
    /// 参照先が構造編集で削除されたcell参照。評価はDeletedReference errorになる。
    DeletedReference,
    /// 条件に合う行だけを集計する（SUMIF・COUNTIF）。
    ConditionalAggregate {
        /// SumIfまたはCountIf。
        function: ConditionalAggregateFunction,
        /// 条件を判定するrange。
        criteria_range: CalculationRangeReference,
        /// 比較関数（Equals・NotEquals・Less・LessOrEqual・Greater・GreaterOrEqual）。
        comparison: PureCalculationFunction,
        /// 判定するcellと比べる値の式。
        value: Box<CalculationExpression>,
        /// SumIfの合計対象range。CountIfではNone。
        sum_range: Option<CalculationRangeReference>,
    },
    /// range内の値を集計する。rangeは集計関数の引数としてだけ現れる。
    Aggregate {
        /// 集計関数。
        function: AggregateFunction,
        /// 集計対象のrange。
        range: CalculationRangeReference,
    },
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
/// 責務: [AggregateFunction: range内の値を集計する副作用なし関数を表す]
/// ```
pub enum AggregateFunction {
    /// 数値の合計。空は0。
    Sum,
    /// 数値として解釈できるcellの個数。
    Count,
    /// 空でないcellの個数（raw文字列が空でないもの）。
    CountA,
    /// 数値の平均。数値が無ければerror。
    Average,
    /// 数値の最小値。数値が無ければerror。
    Min,
    /// 数値の最大値。数値が無ければerror。
    Max,
}

impl AggregateFunction {
    /// {
    ///   責務: [dsl_name: DSL上の関数名を返す]
    ///   戻り値: [&'static str: 関数名]
    /// }
    pub const fn dsl_name(self) -> &'static str {
        match self {
            Self::Sum => "Sum",
            Self::Count => "Count",
            Self::CountA => "CountA",
            Self::Average => "Average",
            Self::Min => "Min",
            Self::Max => "Max",
        }
    }

    /// {
    ///   責務: [from_dsl_name: DSL関数名を大文字小文字無視で集計関数へ対応付ける]
    ///   引数: [name: DSL上の関数名]
    ///   戻り値: [Option<Self>: 集計関数。未対応名はNone]
    /// }
    pub fn from_dsl_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "sum" => Some(Self::Sum),
            "count" => Some(Self::Count),
            "counta" => Some(Self::CountA),
            "average" => Some(Self::Average),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            _ => None,
        }
    }
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
    /// 数値を0から遠い方向へ四捨五入しIntegerを返す。
    Round,
    /// 数値を負の無限大方向へ切り捨てIntegerを返す。
    Floor,
    /// 数値を正の無限大方向へ切り上げIntegerを返す。
    Ceil,
    /// 除数と同じ符号の剰余を返す。
    Mod,
    /// 文字列のUnicodeスカラ値の個数を返す。
    Len,
    /// 2つの値を文字列として連結する。
    Concat,
    /// 先頭から指定文字数を返す。
    Left,
    /// 末尾から指定文字数を返す。
    Right,
    /// 1始まりの位置から指定文字数を返す。
    Mid,
    /// 前後のUnicode空白を除く。
    Trim,
    /// 大文字へ変換する。
    Upper,
    /// 小文字へ変換する。
    Lower,
    /// 条件・比較・論理関数 If。
    If,
    /// 条件・比較・論理関数 And。
    And,
    /// 条件・比較・論理関数 Or。
    Or,
    /// 条件・比較・論理関数 Not。
    Not,
    /// 条件・比較・論理関数 Equals。
    Equals,
    /// 条件・比較・論理関数 NotEquals。
    NotEquals,
    /// 条件・比較・論理関数 Less。
    Less,
    /// 条件・比較・論理関数 LessOrEqual。
    LessOrEqual,
    /// 条件・比較・論理関数 Greater。
    Greater,
    /// 条件・比較・論理関数 GreaterOrEqual。
    GreaterOrEqual,
    /// 年・月・日からISO 8601の日付Textを作る。
    Date,
    /// 日付Textの年を返す。
    Year,
    /// 日付Textの月を返す。
    Month,
    /// 日付Textの日を返す。
    Day,
}

impl CalculationExpression {
    /// ```text
    /// 責務: [to_dsl_source: CalculationExpressionをRowly DSLの安全な式表記へ変換する]
    /// 引数: [self: 変換するpure calculation AST]
    /// 戻り値: [String: parserが再度受理できるDSL expression]
    /// ```
    pub fn to_dsl_source(&self) -> String {
        match self {
            Self::Literal(CalculationValue::Text(value)) => {
                format!("String({})", quote_calculation_text(value))
            }
            Self::Literal(CalculationValue::Integer(value)) => {
                format!("Integer({})", quote_calculation_text(&value.to_string()))
            }
            Self::Literal(CalculationValue::Boolean(value)) => {
                format!("Boolean({})", quote_calculation_text(&value.to_string()))
            }
            Self::Literal(CalculationValue::Decimal(value)) => {
                format!("Decimal({})", quote_calculation_text(&value.to_string()))
            }
            Self::Cell(reference) => {
                format!(
                    "CellValue({})",
                    quote_calculation_text(&reference.to_string())
                )
            }
            Self::DeletedReference => "CellValue(\"#REF!\")".to_owned(),
            Self::ConditionalAggregate {
                function,
                criteria_range,
                comparison,
                value,
                sum_range,
            } => {
                let mut arguments = format!(
                    "CellRange({}), {}, {}",
                    quote_calculation_text(&criteria_range.to_string()),
                    quote_calculation_text(comparison_symbol(*comparison).unwrap_or("=")),
                    value.to_dsl_source()
                );
                if let Some(sum_range) = sum_range {
                    arguments.push_str(&format!(
                        ", CellRange({})",
                        quote_calculation_text(&sum_range.to_string())
                    ));
                }
                format!("{}({arguments})", function.dsl_name())
            }
            Self::Aggregate { function, range } => format!(
                "{}(CellRange({}))",
                function.dsl_name(),
                quote_calculation_text(&range.to_string())
            ),
            Self::AnchoredCell(reference) => {
                format!(
                    "CellValue({})",
                    quote_calculation_text(&reference.to_string())
                )
            }
            Self::Negate(operand) => format!("-({})", operand.to_dsl_source()),
            Self::Arithmetic {
                left,
                operator,
                right,
            } => {
                let operator = match operator {
                    CalculationOperator::Add => "+",
                    CalculationOperator::Subtract => "-",
                    CalculationOperator::Multiply => "*",
                    CalculationOperator::Divide => "/",
                };
                format!(
                    "({}) {operator} ({})",
                    left.to_dsl_source(),
                    right.to_dsl_source()
                )
            }
            Self::Call {
                function,
                arguments,
            } => {
                let function = match function {
                    PureCalculationFunction::Abs => "Abs",
                    PureCalculationFunction::Min => "Min",
                    PureCalculationFunction::Max => "Max",
                    PureCalculationFunction::Round => "Round",
                    PureCalculationFunction::Floor => "Floor",
                    PureCalculationFunction::Ceil => "Ceil",
                    PureCalculationFunction::Mod => "Mod",
                    PureCalculationFunction::Len => "Len",
                    PureCalculationFunction::Concat => "Concat",
                    PureCalculationFunction::Left => "Left",
                    PureCalculationFunction::Right => "Right",
                    PureCalculationFunction::Mid => "Mid",
                    PureCalculationFunction::Trim => "Trim",
                    PureCalculationFunction::Upper => "Upper",
                    PureCalculationFunction::Lower => "Lower",
                    PureCalculationFunction::If => "If",
                    PureCalculationFunction::And => "And",
                    PureCalculationFunction::Or => "Or",
                    PureCalculationFunction::Not => "Not",
                    PureCalculationFunction::Equals => "Equals",
                    PureCalculationFunction::NotEquals => "NotEquals",
                    PureCalculationFunction::Less => "Less",
                    PureCalculationFunction::LessOrEqual => "LessOrEqual",
                    PureCalculationFunction::Greater => "Greater",
                    PureCalculationFunction::GreaterOrEqual => "GreaterOrEqual",
                    PureCalculationFunction::Date => "Date",
                    PureCalculationFunction::Year => "Year",
                    PureCalculationFunction::Month => "Month",
                    PureCalculationFunction::Day => "Day",
                };
                let arguments = arguments
                    .iter()
                    .map(Self::to_dsl_source)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{function}({arguments})")
            }
        }
    }

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
                Self::Literal(_) | Self::DeletedReference => {}
                Self::Aggregate { range, .. } => dependencies.extend(range.cells()),
                Self::ConditionalAggregate {
                    criteria_range,
                    value,
                    sum_range,
                    ..
                } => {
                    dependencies.extend(criteria_range.cells());
                    if let Some(sum_range) = sum_range {
                        dependencies.extend(sum_range.cells());
                    }
                    pending.push(value);
                }
                Self::Cell(reference) => {
                    dependencies.insert(*reference);
                }
                Self::AnchoredCell(reference) => {
                    dependencies.insert(reference.cell());
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

    /// {
    ///   責務: [shift_cell_references: 範囲式を複製する出力cellに合わせて依存cell参照を相対移動する。]
    ///   引数: [row_offset: 移動する行数。 column_offset: 移動する列数。]
    ///   戻り値: [Option<Self>: 全参照を有効な座標へ移動できた式。]
    /// }
    pub fn shift_cell_references(&self, row_offset: isize, column_offset: isize) -> Option<Self> {
        match self {
            Self::Literal(value) => Some(Self::Literal(value.clone())),
            Self::Cell(reference) => Some(Self::Cell(CellRef::new(
                reference.row().checked_add_signed(row_offset)?,
                reference.column().checked_add_signed(column_offset)?,
            ))),
            Self::AnchoredCell(reference) => Some(Self::AnchoredCell(
                reference.shifted(row_offset, column_offset)?,
            )),
            Self::DeletedReference => Some(Self::DeletedReference),
            Self::ConditionalAggregate {
                function,
                criteria_range,
                comparison,
                value,
                sum_range,
            } => Some(Self::ConditionalAggregate {
                function: *function,
                criteria_range: criteria_range.shifted(row_offset, column_offset)?,
                comparison: *comparison,
                value: Box::new(value.shift_cell_references(row_offset, column_offset)?),
                sum_range: match sum_range {
                    Some(range) => Some(range.shifted(row_offset, column_offset)?),
                    None => None,
                },
            }),
            Self::Aggregate { function, range } => Some(Self::Aggregate {
                function: *function,
                range: range.shifted(row_offset, column_offset)?,
            }),
            Self::Negate(operand) => Some(Self::Negate(Box::new(
                operand.shift_cell_references(row_offset, column_offset)?,
            ))),
            Self::Arithmetic {
                left,
                operator,
                right,
            } => Some(Self::Arithmetic {
                left: Box::new(left.shift_cell_references(row_offset, column_offset)?),
                operator: *operator,
                right: Box::new(right.shift_cell_references(row_offset, column_offset)?),
            }),
            Self::Call {
                function,
                arguments,
            } => Some(Self::Call {
                function: *function,
                arguments: arguments
                    .iter()
                    .map(|argument| argument.shift_cell_references(row_offset, column_offset))
                    .collect::<Option<Vec<_>>>()?,
            }),
        }
    }
}

// {
//   責務: [quote_calculation_text: DSL literal内の引用符と制御文字をescapeする]
//   引数: [value: 引用符で囲む文字列]
//   戻り値: [String: Rowly DSLのquoted string]
// }
fn quote_calculation_text(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}
