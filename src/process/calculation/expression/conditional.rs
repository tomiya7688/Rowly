use super::aggregate::into_value;
use super::compare::call_compare_function;
use super::{
    CalculationExpression, CalculationFailure, CalculationOperator, CalculationValue,
    PureCalculationFunction, arithmetic, evaluate_expression, numeric_value,
};
use crate::process::CellRef;
use crate::process::calculation::types::{CalculationRangeReference, ConditionalAggregateFunction};

/// SUMIF・COUNTIFの評価に必要な、構造体へまとめた引数。
pub(super) struct ConditionalSpec<'a> {
    /// SumIfまたはCountIf。
    pub(super) function: ConditionalAggregateFunction,
    /// 条件を判定するrange。
    pub(super) criteria_range: CalculationRangeReference,
    /// 比較関数（Equals等）。
    pub(super) comparison: PureCalculationFunction,
    /// 判定するcellと比べる値の式。
    pub(super) value: &'a CalculationExpression,
    /// SumIfの合計対象range。省略時は判定rangeそのものを合計する。
    pub(super) sum_range: Option<CalculationRangeReference>,
}

// {
//   責務: [evaluate_conditional: 判定rangeの各cellを値と比較し、合うcellの数または対応する合計対象の和を返す]
//   処理: [比較値は1回だけ評価する。比較は#292と同じ規則で、空cellは空文字として比較する。合計対象は空白cellを除き、数値でないTextは暗黙に0へ変換せずerrorにする]
//   引数: [spec: 関数・range・比較・比較値, depth: 式深さ, read_cell: 制限されたcell reader]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 件数または合計。大きさが違うrangeはRangeShapeMismatch]
// }
pub(super) fn evaluate_conditional(
    spec: ConditionalSpec<'_>,
    depth: usize,
    read_cell: &mut impl FnMut(CellRef) -> Result<CalculationValue, CalculationFailure>,
) -> Result<CalculationValue, CalculationFailure> {
    let criterion = evaluate_expression(spec.value, depth + 1, read_cell)?;
    let criteria_cells = spec.criteria_range.cells();
    let sum_cells = match spec.sum_range {
        Some(range) if range.dimensions() != spec.criteria_range.dimensions() => {
            return Err(CalculationFailure::RangeShapeMismatch);
        }
        Some(range) => range.cells(),
        None => criteria_cells.clone(),
    };

    let mut count = 0_i64;
    let mut total = CalculationValue::Integer(0);
    for (index, cell) in criteria_cells.iter().enumerate() {
        let text = read_cell(*cell)?.into_text();
        let matched = call_compare_function(
            spec.comparison,
            vec![CalculationValue::Text(text), criterion.clone()],
        )?;
        if matched != CalculationValue::Boolean(true) {
            continue;
        }
        count += 1;
        if spec.function == ConditionalAggregateFunction::CountIf {
            continue;
        }
        let Some(sum_cell) = sum_cells.get(index) else {
            return Err(CalculationFailure::RangeShapeMismatch);
        };
        let sum_text = read_cell(*sum_cell)?.into_text();
        if sum_text.trim().is_empty() {
            continue;
        }
        let number = into_value(numeric_value(CalculationValue::Text(sum_text))?);
        total = arithmetic(total, CalculationOperator::Add, number)?;
    }

    Ok(match spec.function {
        ConditionalAggregateFunction::CountIf => CalculationValue::Integer(count),
        ConditionalAggregateFunction::SumIf => total,
    })
}
