use std::cmp::Ordering;

use super::{
    CalculationFailure, CalculationOperator, CalculationValue, NumericValue, arithmetic,
    numeric_value,
};
use crate::process::CellRef;
use crate::process::calculation::types::{AggregateFunction, CalculationRangeReference};

// {
//   責務: [evaluate_aggregate: range内の値を集計関数で評価する]
//   処理: [cellを行優先で読み、空白だけのcellを数値集計から除き、数値でないTextは暗黙に0へ変換せずerrorにする。合計と平均は既存の算術規則でInteger精度契約を保つ]
//   引数: [function: 集計関数, range: 集計対象range, read_cell: 制限されたcell reader（範囲外はMissingCell）]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 集計値、またはMissingCell・ExpectedNumber・EmptyAggregate等]
// }
pub(super) fn evaluate_aggregate(
    function: AggregateFunction,
    range: CalculationRangeReference,
    read_cell: &mut impl FnMut(CellRef) -> Result<CalculationValue, CalculationFailure>,
) -> Result<CalculationValue, CalculationFailure> {
    let mut texts = Vec::new();
    for cell in range.cells() {
        texts.push(read_cell(cell)?.into_text());
    }
    aggregate_texts(function, texts)
}

// {
//   責務: [aggregate_texts: 文字列のcell値を集計関数で評価する。range集計とGroup Byが同じ規則を共有する]
//   処理: [空白だけの値を数値集計から除き、数値でないTextは暗黙に0へ変換せずerrorにする。合計と平均は既存の算術規則でInteger精度契約を保つ]
//   引数: [function: 集計関数, texts: 集計対象のcell文字列]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 集計値、またはExpectedNumber・EmptyAggregate等]
// }
pub fn aggregate_texts(
    function: AggregateFunction,
    texts: impl IntoIterator<Item = String>,
) -> Result<CalculationValue, CalculationFailure> {
    let mut non_empty = 0_i64;
    let mut numeric_count = 0_i64;
    let mut numbers = Vec::new();
    for text in texts {
        if !text.is_empty() {
            non_empty += 1;
        }
        if text.trim().is_empty() {
            continue;
        }
        match function {
            AggregateFunction::CountA => {}
            AggregateFunction::Count => {
                if numeric_value(CalculationValue::Text(text)).is_ok() {
                    numeric_count += 1;
                }
            }
            _ => numbers.push(into_value(numeric_value(CalculationValue::Text(text))?)),
        }
    }

    match function {
        AggregateFunction::CountA => Ok(CalculationValue::Integer(non_empty)),
        AggregateFunction::Count => Ok(CalculationValue::Integer(numeric_count)),
        AggregateFunction::Sum => sum(numbers),
        AggregateFunction::Average => {
            if numbers.is_empty() {
                return Err(CalculationFailure::EmptyAggregate);
            }
            let count =
                i64::try_from(numbers.len()).map_err(|_| CalculationFailure::ArithmeticOverflow)?;
            arithmetic(
                sum(numbers)?,
                CalculationOperator::Divide,
                CalculationValue::Integer(count),
            )
        }
        AggregateFunction::Min | AggregateFunction::Max => extreme(function, numbers),
    }
}

// {
//   責務: [sum: 数値列を既存のchecked算術で合計する。空は0]
//   引数: [numbers: 空でない数値]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 合計、overflow・precision lossはerror]
// }
fn sum(numbers: Vec<CalculationValue>) -> Result<CalculationValue, CalculationFailure> {
    numbers
        .into_iter()
        .try_fold(CalculationValue::Integer(0), |total, value| {
            arithmetic(total, CalculationOperator::Add, value)
        })
}

// {
//   責務: [extreme: 数値列の最小または最大を元の型のまま返す]
//   引数: [function: MinまたはMax, numbers: 空でない数値]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 極値、数値が無ければEmptyAggregate]
// }
fn extreme(
    function: AggregateFunction,
    numbers: Vec<CalculationValue>,
) -> Result<CalculationValue, CalculationFailure> {
    let mut numbers = numbers.into_iter();
    let mut best = numbers.next().ok_or(CalculationFailure::EmptyAggregate)?;
    for value in numbers {
        let ordering = numeric_value(value.clone())?.compare(numeric_value(best.clone())?);
        let replace = if function == AggregateFunction::Min {
            ordering == Ordering::Less
        } else {
            ordering == Ordering::Greater
        };
        if replace {
            best = value;
        }
    }
    Ok(best)
}

// {
//   責務: [into_value: 内部numeric値をCalculationValueへ戻す]
//   引数: [value: 数値]
//   戻り値: [CalculationValue: 同じ型のscalar]
// }
pub(super) fn into_value(value: NumericValue) -> CalculationValue {
    match value {
        NumericValue::Integer(value) => CalculationValue::Integer(value),
        NumericValue::Decimal(value) => CalculationValue::Decimal(value),
    }
}
