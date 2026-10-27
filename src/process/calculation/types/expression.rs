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
            Self::Literal(CalculationValue::Decimal(value)) => {
                format!("Decimal({})", quote_calculation_text(&value.to_string()))
            }
            Self::Cell(reference) => {
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
