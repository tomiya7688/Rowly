use super::{
    CalculationExpression, CalculationFailure, CalculationValue, PureCalculationFunction,
    evaluate_expression,
};
use crate::process::CellRef;

// {
//   責務: [evaluate_lazy: If・And・Or・Notを短絡評価する。他の関数ではNoneを返し通常の先行評価に任せる]
//   処理: [Ifは選ばれた分岐だけ、And/Orは結果が決まれば右辺を評価しない。条件・オペランドはBooleanでなければならず、数値やTextを暗黙に変換しない]
//   引数: [function: 呼び出す関数, arguments: 個数検証済みの引数式, depth: 現在の式深さ, read_cell: 制限されたcell reader]
//   戻り値: [Option<Result<CalculationValue, CalculationFailure>>: 遅延評価の対象ならその結果]
// }
pub(super) fn evaluate_lazy(
    function: PureCalculationFunction,
    arguments: &[CalculationExpression],
    depth: usize,
    read_cell: &mut impl FnMut(CellRef) -> Result<CalculationValue, CalculationFailure>,
) -> Option<Result<CalculationValue, CalculationFailure>> {
    match (function, arguments) {
        (PureCalculationFunction::Not, [operand]) => Some(
            evaluate_boolean(operand, depth, read_cell)
                .map(|value| CalculationValue::Boolean(!value)),
        ),
        (PureCalculationFunction::And, [left, right]) => {
            Some(short_circuit(left, right, false, depth, read_cell))
        }
        (PureCalculationFunction::Or, [left, right]) => {
            Some(short_circuit(left, right, true, depth, read_cell))
        }
        (PureCalculationFunction::If, [condition, then_branch, else_branch]) => {
            Some(match evaluate_boolean(condition, depth, read_cell) {
                Ok(true) => evaluate_expression(then_branch, depth + 1, read_cell),
                Ok(false) => evaluate_expression(else_branch, depth + 1, read_cell),
                Err(failure) => Err(failure),
            })
        }
        _ => None,
    }
}

// {
//   責務: [short_circuit: AndまたはOrを左から評価し、結果が決まれば右辺を評価しない]
//   引数: [left: 左辺, right: 右辺, stop_on: 左辺がこの値なら右辺を評価せずその値を返す（Andはfalse、Orはtrue）, depth: 式深さ, read_cell: cell reader]
//   戻り値: [Result<CalculationValue, CalculationFailure>: Boolean、またはExpectedBoolean等のerror]
// }
fn short_circuit(
    left: &CalculationExpression,
    right: &CalculationExpression,
    stop_on: bool,
    depth: usize,
    read_cell: &mut impl FnMut(CellRef) -> Result<CalculationValue, CalculationFailure>,
) -> Result<CalculationValue, CalculationFailure> {
    if evaluate_boolean(left, depth, read_cell)? == stop_on {
        return Ok(CalculationValue::Boolean(stop_on));
    }
    evaluate_boolean(right, depth, read_cell).map(CalculationValue::Boolean)
}

// {
//   責務: [evaluate_boolean: 式を評価しBooleanであることを要求する]
//   引数: [expression: 評価する式, depth: 親の式深さ, read_cell: cell reader]
//   戻り値: [Result<bool, CalculationFailure>: bool、それ以外はExpectedBoolean]
// }
fn evaluate_boolean(
    expression: &CalculationExpression,
    depth: usize,
    read_cell: &mut impl FnMut(CellRef) -> Result<CalculationValue, CalculationFailure>,
) -> Result<bool, CalculationFailure> {
    match evaluate_expression(expression, depth + 1, read_cell)? {
        CalculationValue::Boolean(value) => Ok(value),
        other => Err(CalculationFailure::ExpectedBoolean(other.into_text())),
    }
}
