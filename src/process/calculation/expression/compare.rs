use std::cmp::Ordering;

use super::{
    CalculationFailure, CalculationValue, NumericValue, PureCalculationFunction, function_name,
    numeric_value,
};

// {
//   責務: [call_compare_function: 2つのscalarを比較してBooleanを返す]
//   処理: [両辺が数値として解釈できれば数値比較、そうでなければ表示文字列をUnicodeスカラ値の順で比較する。Booleanは同じ型同士の等価比較だけを許可する]
//   引数: [function: Equals・NotEquals・Less・LessOrEqual・Greater・GreaterOrEqual, arguments: 個数検証済みの2引数]
//   戻り値: [Result<CalculationValue, CalculationFailure>: Boolean、比較できない組はIncomparableValues]
// }
pub(super) fn call_compare_function(
    function: PureCalculationFunction,
    arguments: Vec<CalculationValue>,
) -> Result<CalculationValue, CalculationFailure> {
    let actual = arguments.len();
    let mut arguments = arguments.into_iter();
    let (Some(left), Some(right)) = (arguments.next(), arguments.next()) else {
        return Err(CalculationFailure::InvalidFunctionArguments {
            function: function_name(function),
            expected: 2,
            actual,
        });
    };
    let is_equality = matches!(
        function,
        PureCalculationFunction::Equals | PureCalculationFunction::NotEquals
    );
    let ordering = order(left, right, is_equality)?;
    let result = match function {
        PureCalculationFunction::Equals => ordering == Ordering::Equal,
        PureCalculationFunction::NotEquals => ordering != Ordering::Equal,
        PureCalculationFunction::Less => ordering == Ordering::Less,
        PureCalculationFunction::LessOrEqual => ordering != Ordering::Greater,
        PureCalculationFunction::Greater => ordering == Ordering::Greater,
        PureCalculationFunction::GreaterOrEqual => ordering != Ordering::Less,
        _ => {
            return Err(CalculationFailure::InvalidFunctionArguments {
                function: function_name(function),
                expected: 2,
                actual,
            });
        }
    };
    Ok(CalculationValue::Boolean(result))
}

// {
//   責務: [order: 比較規則に従って2つの値の順序を決める]
//   引数: [left: 左辺, right: 右辺, is_equality: 等価比較ならtrue（Boolean同士の比較を許可する）]
//   戻り値: [Result<Ordering, CalculationFailure>: 順序、比較できない組はIncomparableValues]
// }
fn order(
    left: CalculationValue,
    right: CalculationValue,
    is_equality: bool,
) -> Result<Ordering, CalculationFailure> {
    match (&left, &right) {
        (CalculationValue::Boolean(left), CalculationValue::Boolean(right)) if is_equality => {
            Ok(left.cmp(right))
        }
        (CalculationValue::Boolean(_), _) | (_, CalculationValue::Boolean(_)) => {
            Err(CalculationFailure::IncomparableValues)
        }
        _ => match (numeric_value(left.clone()), numeric_value(right.clone())) {
            (Ok(left), Ok(right)) => Ok(compare_numbers(left, right)),
            _ => Ok(left.into_text().cmp(&right.into_text())),
        },
    }
}

// {
//   責務: [compare_numbers: IntegerとDecimalを精度を保って比較する]
//   引数: [left: 左辺, right: 右辺]
//   戻り値: [Ordering: 数値としての順序]
// }
fn compare_numbers(left: NumericValue, right: NumericValue) -> Ordering {
    left.compare(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> CalculationValue {
        CalculationValue::Text(value.to_owned())
    }

    fn call(
        function: PureCalculationFunction,
        left: CalculationValue,
        right: CalculationValue,
    ) -> Result<CalculationValue, CalculationFailure> {
        call_compare_function(function, vec![left, right])
    }

    // {
    //   責務: [numeric_looking_text_compares_as_number_otherwise_as_text: 数値に見えるTextは数値比較し、それ以外は文字列順で比較することを検証する]
    //   戻り値: [(): 数値/Textの比較規則が仕様どおりなら成功する]
    // }
    #[test]
    fn numeric_looking_text_compares_as_number_otherwise_as_text() {
        use PureCalculationFunction::{Equals, Greater, Less};
        let truth = Ok(CalculationValue::Boolean(true));
        // "10" と "9" は数値として比較される（文字列順なら"10" < "9"）。
        assert_eq!(call(Greater, text("10"), text("9")), truth);
        assert_eq!(
            call(Equals, text("1.0"), CalculationValue::Integer(1)),
            truth
        );
        // 数値でないTextは文字列順。
        assert_eq!(call(Less, text("apple"), text("banana")), truth);
        // 数値とTextの混在は文字列順（"10" < "abc"）。
        assert_eq!(
            call(Less, CalculationValue::Integer(10), text("abc")),
            truth
        );
        // 空文字は数値ではなく、空文字どうしは等しい。
        assert_eq!(call(Equals, text(""), text("")), truth);
    }

    // {
    //   責務: [boolean_compares_only_by_equality_with_boolean: Booleanは同型の等価比較だけを許可することを検証する]
    //   戻り値: [(): 不正な組がIncomparableValuesなら成功する]
    // }
    #[test]
    fn boolean_compares_only_by_equality_with_boolean() {
        use PureCalculationFunction::{Equals, Less};
        let (yes, no) = (
            CalculationValue::Boolean(true),
            CalculationValue::Boolean(false),
        );
        assert_eq!(
            call(Equals, yes.clone(), yes.clone()),
            Ok(CalculationValue::Boolean(true))
        );
        assert_eq!(
            call(Less, no, yes.clone()),
            Err(CalculationFailure::IncomparableValues)
        );
        assert_eq!(
            call(Equals, yes, text("TRUE")),
            Err(CalculationFailure::IncomparableValues)
        );
    }
}
