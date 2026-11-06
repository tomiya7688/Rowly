//! 条件付き集計の関数種別と、比較記号の対応表。

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [ConditionalAggregateFunction: 条件に合うcellだけを集計する副作用なし関数を表す]
/// ```
pub enum ConditionalAggregateFunction {
    /// 条件に合う行のsum_rangeを合計する。
    SumIf,
    /// 条件に合うcellの個数を数える。
    CountIf,
    /// 完全一致する1件のresult_rangeのcellを返す。0件・複数件はerror。
    Lookup,
}

impl ConditionalAggregateFunction {
    /// {
    ///   責務: [dsl_name: DSL上の関数名を返す]
    ///   戻り値: [&'static str: 関数名]
    /// }
    pub const fn dsl_name(self) -> &'static str {
        match self {
            Self::SumIf => "SumIf",
            Self::CountIf => "CountIf",
            Self::Lookup => "Lookup",
        }
    }

    /// {
    ///   責務: [from_dsl_name: DSL関数名を大文字小文字無視で対応付ける]
    ///   引数: [name: DSL上の関数名]
    ///   戻り値: [Option<Self>: 対応する関数。未対応名はNone]
    /// }
    pub fn from_dsl_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "sumif" => Some(Self::SumIf),
            "countif" => Some(Self::CountIf),
            "lookup" => Some(Self::Lookup),
            _ => None,
        }
    }
}

/// {
///   責務: [comparison_from_symbol: 比較記号を比較関数へ対応付ける]
///   引数: [symbol: `=`・`<>`・`<`・`<=`・`>`・`>=`]
///   戻り値: [Option<PureCalculationFunction>: 比較関数。未対応記号はNone]
/// }
pub fn comparison_from_symbol(symbol: &str) -> Option<PureCalculationFunction> {
    match symbol {
        "=" => Some(PureCalculationFunction::Equals),
        "<>" => Some(PureCalculationFunction::NotEquals),
        "<" => Some(PureCalculationFunction::Less),
        "<=" => Some(PureCalculationFunction::LessOrEqual),
        ">" => Some(PureCalculationFunction::Greater),
        ">=" => Some(PureCalculationFunction::GreaterOrEqual),
        _ => None,
    }
}

/// {
///   責務: [comparison_symbol: 比較関数を比較記号へ戻す]
///   引数: [function: 比較関数]
///   戻り値: [Option<&'static str>: 記号。比較関数でなければNone]
/// }
pub fn comparison_symbol(function: PureCalculationFunction) -> Option<&'static str> {
    match function {
        PureCalculationFunction::Equals => Some("="),
        PureCalculationFunction::NotEquals => Some("<>"),
        PureCalculationFunction::Less => Some("<"),
        PureCalculationFunction::LessOrEqual => Some("<="),
        PureCalculationFunction::Greater => Some(">"),
        PureCalculationFunction::GreaterOrEqual => Some(">="),
        _ => None,
    }
}
