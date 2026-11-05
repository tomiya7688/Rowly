use super::{
    CalculationFailure, CalculationValue, NumericValue, PureCalculationFunction, function_name,
    numeric_value,
};

// {
//   責務: [call_text_function: 個数検証済みの引数で文字列関数を評価する]
//   引数: [function: 文字列関数, arguments: 呼び出し元が個数を検証済みのscalar引数]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 関数結果、引数不足や不正値はerror]
// }
pub(super) fn call_text_function(
    function: PureCalculationFunction,
    arguments: Vec<CalculationValue>,
) -> Result<CalculationValue, CalculationFailure> {
    let actual = arguments.len();
    let mut arguments = arguments.into_iter();
    let missing = |expected| CalculationFailure::InvalidFunctionArguments {
        function: function_name(function),
        expected,
        actual,
    };
    let first = arguments.next().ok_or_else(|| missing(1))?;
    match function {
        PureCalculationFunction::Len => text_len(first),
        PureCalculationFunction::Trim => Ok(trim(first)),
        PureCalculationFunction::Upper => Ok(change_case(first, true)),
        PureCalculationFunction::Lower => Ok(change_case(first, false)),
        PureCalculationFunction::Concat => {
            Ok(concat(first, arguments.next().ok_or_else(|| missing(2))?))
        }
        PureCalculationFunction::Left => left(first, arguments.next().ok_or_else(|| missing(2))?),
        PureCalculationFunction::Right => right(first, arguments.next().ok_or_else(|| missing(2))?),
        PureCalculationFunction::Mid => {
            let start = arguments.next().ok_or_else(|| missing(3))?;
            let count = arguments.next().ok_or_else(|| missing(3))?;
            mid(first, start, count)
        }
        _ => Err(missing(1)),
    }
}

// {
//   責務: [text_len: 文字列のUnicodeスカラ値の個数をIntegerで返す]
//   処理: [byte長ではなくchar単位で数える。結合文字や絵文字ZWJ列は構成するスカラ値ごとに数える]
//   引数: [value: 対象scalar（Integer/Decimalは表示文字列として扱う）]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 文字数、i64に収まらない場合はoverflow error]
// }
pub(super) fn text_len(value: CalculationValue) -> Result<CalculationValue, CalculationFailure> {
    i64::try_from(value.into_text().chars().count())
        .map(CalculationValue::Integer)
        .map_err(|_| CalculationFailure::ArithmeticOverflow)
}

// {
//   責務: [concat: 2つのscalarを表示文字列として連結する]
//   引数: [left: 前半, right: 後半]
//   戻り値: [CalculationValue: 連結したText]
// }
pub(super) fn concat(left: CalculationValue, right: CalculationValue) -> CalculationValue {
    let mut text = left.into_text();
    text.push_str(&right.into_text());
    CalculationValue::Text(text)
}

// {
//   責務: [left: 先頭からcount文字を返す。count超過時は全体を返す]
//   引数: [value: 対象scalar, count: 0以上のInteger]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 部分文字列、countが不正ならInvalidTextIndex]
// }
pub(super) fn left(
    value: CalculationValue,
    count: CalculationValue,
) -> Result<CalculationValue, CalculationFailure> {
    let count = text_count(count, 0)?;
    Ok(CalculationValue::Text(
        value.into_text().chars().take(count).collect(),
    ))
}

// {
//   責務: [right: 末尾からcount文字を返す。count超過時は全体を返す]
//   引数: [value: 対象scalar, count: 0以上のInteger]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 部分文字列、countが不正ならInvalidTextIndex]
// }
pub(super) fn right(
    value: CalculationValue,
    count: CalculationValue,
) -> Result<CalculationValue, CalculationFailure> {
    let count = text_count(count, 0)?;
    let text = value.into_text();
    let skip = text.chars().count().saturating_sub(count);
    Ok(CalculationValue::Text(text.chars().skip(skip).collect()))
}

// {
//   責務: [mid: 1始まりのstartからcount文字を返す]
//   処理: [startが文字数を超える場合は空文字、count超過時は末尾までを返す]
//   引数: [value: 対象scalar, start: 1以上のInteger, count: 0以上のInteger]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 部分文字列、start/countが不正ならInvalidTextIndex]
// }
pub(super) fn mid(
    value: CalculationValue,
    start: CalculationValue,
    count: CalculationValue,
) -> Result<CalculationValue, CalculationFailure> {
    let start = text_count(start, 1)?;
    let count = text_count(count, 0)?;
    Ok(CalculationValue::Text(
        value
            .into_text()
            .chars()
            .skip(start - 1)
            .take(count)
            .collect(),
    ))
}

// {
//   責務: [trim: 先頭と末尾のUnicode空白を取り除く。内部の空白は保持する]
//   引数: [value: 対象scalar]
//   戻り値: [CalculationValue: 整形後のText]
// }
pub(super) fn trim(value: CalculationValue) -> CalculationValue {
    CalculationValue::Text(value.into_text().trim().to_owned())
}

// {
//   責務: [change_case: localeに依存しないUnicode標準の大文字化・小文字化を行う]
//   引数: [value: 対象scalar, upper: trueなら大文字、falseなら小文字]
//   戻り値: [CalculationValue: 変換後のText]
// }
pub(super) fn change_case(value: CalculationValue, upper: bool) -> CalculationValue {
    let text = value.into_text();
    CalculationValue::Text(if upper {
        text.to_uppercase()
    } else {
        text.to_lowercase()
    })
}

// {
//   責務: [text_count: 文字位置・文字数の引数をminimum以上のIntegerとして検証する]
//   引数: [value: 検証する引数, minimum: 許可する最小値]
//   戻り値: [Result<usize, CalculationFailure>: 検証済みの値、非整数や範囲外はInvalidTextIndex]
// }
fn text_count(value: CalculationValue, minimum: i64) -> Result<usize, CalculationFailure> {
    let shown = value.clone().into_text();
    match numeric_value(value) {
        Ok(NumericValue::Integer(number)) if number >= minimum => {
            usize::try_from(number).map_err(|_| CalculationFailure::InvalidTextIndex(shown))
        }
        _ => Err(CalculationFailure::InvalidTextIndex(shown)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> CalculationValue {
        CalculationValue::Text(value.to_owned())
    }

    fn int(value: i64) -> CalculationValue {
        CalculationValue::Integer(value)
    }

    // {
    //   責務: [len_counts_unicode_scalars_not_bytes: 長さがbyte数ではなくUnicodeスカラ数であることを検証する]
    //   戻り値: [(): 日本語・絵文字・空文字の長さが仕様どおりなら成功する]
    // }
    #[test]
    fn len_counts_unicode_scalars_not_bytes() {
        assert_eq!(text_len(text("日本語")), Ok(int(3)));
        assert_eq!(text_len(text("a😀")), Ok(int(2)));
        assert_eq!(text_len(text("")), Ok(int(0)));
        assert_eq!(text_len(int(-12)), Ok(int(3)));
    }

    // {
    //   責務: [left_right_mid_clamp_to_the_text_and_reject_bad_indexes: 切り出しの境界と不正引数を検証する]
    //   戻り値: [(): 範囲外はemptyまたは全体、不正値はerrorなら成功する]
    // }
    #[test]
    fn left_right_mid_clamp_to_the_text_and_reject_bad_indexes() {
        assert_eq!(left(text("日本語"), int(2)), Ok(text("日本")));
        assert_eq!(left(text("ab"), int(9)), Ok(text("ab")));
        assert_eq!(left(text("ab"), int(0)), Ok(text("")));
        assert_eq!(right(text("日本語"), int(2)), Ok(text("本語")));
        assert_eq!(right(text("ab"), int(9)), Ok(text("ab")));
        assert_eq!(mid(text("abcdef"), int(2), int(3)), Ok(text("bcd")));
        assert_eq!(mid(text("abc"), int(9), int(2)), Ok(text("")));
        assert_eq!(mid(text("abc"), int(3), int(9)), Ok(text("c")));
        assert_eq!(
            mid(text("abc"), int(0), int(1)),
            Err(CalculationFailure::InvalidTextIndex("0".to_owned()))
        );
        assert_eq!(
            left(text("abc"), int(-1)),
            Err(CalculationFailure::InvalidTextIndex("-1".to_owned()))
        );
        assert_eq!(
            right(text("abc"), CalculationValue::Decimal(1.5)),
            Err(CalculationFailure::InvalidTextIndex("1.5".to_owned()))
        );
        assert_eq!(
            left(text("abc"), text("x")),
            Err(CalculationFailure::InvalidTextIndex("x".to_owned()))
        );
    }

    // {
    //   責務: [concat_trim_and_case_follow_unicode_rules: 連結・空白除去・大小変換を検証する]
    //   戻り値: [(): 内部空白保持、数値の文字列化、Unicode大小変換が仕様どおりなら成功する]
    // }
    #[test]
    fn concat_trim_and_case_follow_unicode_rules() {
        assert_eq!(concat(text("a"), int(1)), text("a1"));
        assert_eq!(trim(text("\u{3000} a  b \t")), text("a  b"));
        assert_eq!(change_case(text("straße"), true), text("STRASSE"));
        assert_eq!(change_case(text("ÀB"), false), text("àb"));
    }
}
