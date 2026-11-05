use super::{
    CalculationFailure, CalculationValue, NumericValue, PureCalculationFunction, function_name,
    numeric_value,
};

/// 日付として受け付ける年の最小値（4桁の西暦）。
const MIN_YEAR: i64 = 1;
/// 日付として受け付ける年の最大値（4桁の西暦）。
const MAX_YEAR: i64 = 9999;

// {
//   責務: [call_date_function: 個数検証済みの引数で日付関数を評価する]
//   引数: [function: 日付関数, arguments: 呼び出し元が個数を検証済みのscalar引数]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 関数結果、不正な日付はInvalidDate]
// }
pub(super) fn call_date_function(
    function: PureCalculationFunction,
    arguments: Vec<CalculationValue>,
) -> Result<CalculationValue, CalculationFailure> {
    let actual = arguments.len();
    let missing = |expected| CalculationFailure::InvalidFunctionArguments {
        function: function_name(function),
        expected,
        actual,
    };
    let mut arguments = arguments.into_iter();
    let first = arguments.next().ok_or_else(|| missing(1))?;
    match function {
        PureCalculationFunction::Date => {
            let month = arguments.next().ok_or_else(|| missing(3))?;
            let day = arguments.next().ok_or_else(|| missing(3))?;
            make_date(&first, &month, &day)
        }
        PureCalculationFunction::Year => date_part(first, |(year, _, _)| year),
        PureCalculationFunction::Month => date_part(first, |(_, month, _)| month),
        PureCalculationFunction::Day => date_part(first, |(_, _, day)| day),
        _ => Err(missing(1)),
    }
}

// {
//   責務: [make_date: 年・月・日のIntegerから実在する日付をISO 8601のTextで作る]
//   引数: [year: 1〜9999, month: 1〜12, day: 月の日数以内]
//   戻り値: [Result<CalculationValue, CalculationFailure>: ``YYYY-MM-DD`` のText、存在しない日付はInvalidDate]
// }
fn make_date(
    year: &CalculationValue,
    month: &CalculationValue,
    day: &CalculationValue,
) -> Result<CalculationValue, CalculationFailure> {
    let shown = format!(
        "{}-{}-{}",
        year.clone().into_text(),
        month.clone().into_text(),
        day.clone().into_text()
    );
    let invalid = || CalculationFailure::InvalidDate(shown.clone());
    let (year, month, day) = (
        integer_part(year).ok_or_else(invalid)?,
        integer_part(month).ok_or_else(invalid)?,
        integer_part(day).ok_or_else(invalid)?,
    );
    if !(MIN_YEAR..=MAX_YEAR).contains(&year)
        || !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
    {
        return Err(invalid());
    }
    Ok(CalculationValue::Text(format!(
        "{year:04}-{month:02}-{day:02}"
    )))
}

// {
//   責務: [date_part: ISO 8601の日付Textを検証し、指定要素のIntegerを返す]
//   引数: [value: ``YYYY-MM-DD`` のText, select: 年月日から返す要素を選ぶ関数]
//   戻り値: [Result<CalculationValue, CalculationFailure>: 要素のInteger、形式不正や存在しない日付はInvalidDate]
// }
fn date_part(
    value: CalculationValue,
    select: fn((i64, i64, i64)) -> i64,
) -> Result<CalculationValue, CalculationFailure> {
    let text = value.into_text();
    parse_iso_date(&text)
        .map(|parts| CalculationValue::Integer(select(parts)))
        .ok_or(CalculationFailure::InvalidDate(text))
}

// {
//   責務: [parse_iso_date: 厳密な``YYYY-MM-DD``だけを年月日へ分解する]
//   処理: [locale依存の区切り・並び・2桁年・空白付きを受け付けず、実在する日付だけを返す]
//   引数: [text: 検査する文字列]
//   戻り値: [Option<(i64, i64, i64)>: 年・月・日、不正ならNone]
// }
fn parse_iso_date(text: &str) -> Option<(i64, i64, i64)> {
    let bytes = text.as_bytes();
    let shape_is_valid = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 4 || index == 7 || byte.is_ascii_digit());
    if !shape_is_valid {
        return None;
    }
    let year: i64 = text[0..4].parse().ok()?;
    let month: i64 = text[5..7].parse().ok()?;
    let day: i64 = text[8..10].parse().ok()?;
    ((MIN_YEAR..=MAX_YEAR).contains(&year)
        && (1..=12).contains(&month)
        && day >= 1
        && day <= days_in_month(year, month))
    .then_some((year, month, day))
}

// {
//   責務: [integer_part: 日付要素の引数をIntegerとして取り出す。Decimalや非数値は拒否する]
//   引数: [value: 年・月・日の引数]
//   戻り値: [Option<i64>: Integer値、それ以外はNone]
// }
fn integer_part(value: &CalculationValue) -> Option<i64> {
    match numeric_value(value.clone()) {
        Ok(NumericValue::Integer(number)) => Some(number),
        _ => None,
    }
}

// {
//   責務: [days_in_month: グレゴリオ暦の月の日数を返す]
//   引数: [year: 西暦年, month: 1〜12]
//   戻り値: [i64: その月の日数]
// }
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(value: i64) -> CalculationValue {
        CalculationValue::Integer(value)
    }

    fn text(value: &str) -> CalculationValue {
        CalculationValue::Text(value.to_owned())
    }

    // {
    //   責務: [date_builds_iso_text_and_rejects_nonexistent_days: 実在日付だけがISO形式になることを検証する]
    //   戻り値: [(): うるう年・月末・範囲外の扱いが仕様どおりなら成功する]
    // }
    #[test]
    fn date_builds_iso_text_and_rejects_nonexistent_days() {
        assert_eq!(
            make_date(&int(2024), &int(2), &int(29)),
            Ok(text("2024-02-29"))
        );
        assert_eq!(make_date(&int(7), &int(1), &int(5)), Ok(text("0007-01-05")));
        for (y, m, d) in [
            (2023, 2, 29),
            (1900, 2, 29),
            (2024, 4, 31),
            (2024, 13, 1),
            (2024, 0, 1),
            (0, 1, 1),
            (10000, 1, 1),
            (2024, 1, 0),
        ] {
            assert!(
                matches!(
                    make_date(&int(y), &int(m), &int(d)),
                    Err(CalculationFailure::InvalidDate(_))
                ),
                "{y}-{m}-{d}"
            );
        }
        assert_eq!(
            make_date(&int(2000), &int(2), &int(29)),
            Ok(text("2000-02-29"))
        );
        assert!(make_date(&CalculationValue::Decimal(2024.5), &int(1), &int(1)).is_err());
        assert!(make_date(&text("abc"), &int(1), &int(1)).is_err());
    }

    // {
    //   責務: [parts_accept_only_strict_iso_text: 厳密なISO形式以外を暗黙解釈しないことを検証する]
    //   戻り値: [(): locale依存の形式や不正日付がerrorで、正しい形式は分解できれば成功する]
    // }
    #[test]
    fn parts_accept_only_strict_iso_text() {
        assert_eq!(date_part(text("2024-03-09"), |(y, _, _)| y), Ok(int(2024)));
        assert_eq!(date_part(text("2024-03-09"), |(_, m, _)| m), Ok(int(3)));
        assert_eq!(date_part(text("2024-03-09"), |(_, _, d)| d), Ok(int(9)));
        for bad in [
            "2024/03/09",
            "03/09/2024",
            "2024-3-9",
            " 2024-03-09",
            "2024-02-30",
            "２０２４-03-09",
            "",
            "20240309",
        ] {
            assert_eq!(
                date_part(text(bad), |(y, _, _)| y),
                Err(CalculationFailure::InvalidDate(bad.to_owned())),
                "{bad}"
            );
        }
    }
}
