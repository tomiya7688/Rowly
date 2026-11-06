//! ```text
//! 責務: [display_format: raw文字列を、View用の表示文字列にする表示書式（DisplayFormat）の核を提供する。CSVの値は変えず、表示形式を理由に数値などへ自動変換しない]
//! 処理: [1: 書式（Text・Integer・Decimal・Percent・Date・DateTime・Boolean・Custom）ごとに、rawを決まった規則で読む 2: 読めなければ書き換えずにrawをそのまま返し、理由つきのformat errorを添える 3: 数値は10進の文字列のまま桁を扱い、浮動小数点の誤差を持ち込まない]
//! 補足: [rawの読み方はlocale非依存の正規形（ASCIIの符号・数字・小数点、ISO日付）だけ。「1,234」や「2026/1/2」のようなlocale依存の表記を、曖昧なまま正規形として読まない。空のrawは値が無いので、どの書式でも空の表示になる]
//! ```

/// 日付の表示様式。rawの読み方（ISO 8601）には影響しない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateStyle {
    /// 2026-01-02
    Iso,
    /// 2026/01/02
    Slash,
    /// 2026年1月2日
    Japanese,
    /// 02/01/2026（日/月/年）
    DayMonthYear,
    /// 01/02/2026（月/日/年）
    MonthDayYear,
}

/// 表示書式。View設定であり、CSVには保存されない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayFormat {
    /// 文字列のまま。`00123`などをそのまま見せる。
    Text,
    /// 整数。小数部が0以外のrawは読めない（黙って丸めない）。
    Integer {
        /// 3桁区切りを使うか。
        thousands_separator: bool,
    },
    /// 小数。指定の桁へ四捨五入（絶対値で5以上は切り上げ）する。
    Decimal {
        /// 小数の桁数。
        places: u8,
        /// 3桁区切りを使うか。
        thousands_separator: bool,
    },
    /// 割合。rawは0.125のような値で、100倍して`%`を付ける。
    Percent {
        /// 小数の桁数。
        places: u8,
    },
    /// 日付。rawは`YYYY-MM-DD`。
    Date(DateStyle),
    /// 日時。rawは`YYYY-MM-DDTHH:MM[:SS]`または空白区切り。時差の指定は読まない。
    DateTime {
        /// 日付部の様式。
        style: DateStyle,
        /// 秒まで表示するか。
        show_seconds: bool,
    },
    /// 真偽。rawは`true`/`false`（大文字小文字は問わない）。
    Boolean {
        /// trueの表示。
        true_label: String,
        /// falseの表示。
        false_label: String,
    },
    /// 利用者定義の書式。idで登録済みの`CustomFormatRegistry`の整形関数を呼ぶ。
    Custom(String),
}

/// 書式を適用できなかった理由。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormatError {
    /// 数値として読めない。
    #[error("not a number in the canonical form (sign, digits, optional decimal point)")]
    NotANumber,
    /// 整数として読めない（小数部がある）。
    #[error("not an integer")]
    NotAnInteger,
    /// 日付として読めない。
    #[error("not a valid date (expected YYYY-MM-DD)")]
    InvalidDate,
    /// 日時として読めない。
    #[error("not a valid date-time (expected YYYY-MM-DDTHH:MM[:SS], no time zone)")]
    InvalidDateTime,
    /// 真偽として読めない。
    #[error("not a boolean (expected true or false)")]
    NotABoolean,
    /// Custom書式のidが登録されていない。
    #[error("custom format `{0}` is not registered")]
    UnknownCustom(String),
    /// Custom書式の整形関数が失敗した。
    #[error("custom format failed: {0}")]
    CustomFailed(String),
}

/// 1 cellの表示結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Display {
    /// 書式を適用した表示。
    Formatted(String),
    /// 書式を適用できなかった。`shown`はrawそのまま（書き換えない）で、errorを示す印と一緒に表示する。
    Error {
        /// 表示するraw。
        shown: String,
        /// 理由。
        error: FormatError,
    },
}

impl Display {
    /// 表示する文字列。errorのときはrawそのまま。
    pub fn text(&self) -> &str {
        match self {
            Self::Formatted(text) => text,
            Self::Error { shown, .. } => shown,
        }
    }

    /// format errorか。
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }
}

/// Custom書式の登録先。拡張の境界であり、coreはCustomの中身を知らない。
pub trait CustomFormatRegistry {
    /// idの書式でrawを整形する。idが未登録ならNone、整形に失敗すればErr。
    fn format(&self, id: &str, raw: &str) -> Option<Result<String, String>>;
}

/// Custom書式を1つも持たない登録先。
pub struct NoCustomFormats;

impl CustomFormatRegistry for NoCustomFormats {
    fn format(&self, _id: &str, _raw: &str) -> Option<Result<String, String>> {
        None
    }
}

// {
//   責務: [format_value: rawを書式で表示文字列にする。Custom書式は登録されていないものとして扱う]
//   引数: [raw: CSVのraw文字列, format: 表示書式]
//   戻り値: [Display: 整形結果、または書き換えないraw+理由]
// }
pub fn format_value(raw: &str, format: &DisplayFormat) -> Display {
    format_value_with(raw, format, &NoCustomFormats)
}

// {
//   責務: [format_value_with: rawを書式で表示文字列にする。Custom書式は登録先に任せる]
//   処理: [空のrawは空の表示。それ以外は書式ごとにrawを正規形として読み、読めなければrawをそのまま返してerrorを添える]
//   引数: [raw: CSVのraw文字列, format: 表示書式, custom: Custom書式の登録先]
//   戻り値: [Display: 整形結果、または書き換えないraw+理由]
// }
pub fn format_value_with(
    raw: &str,
    format: &DisplayFormat,
    custom: &dyn CustomFormatRegistry,
) -> Display {
    if raw.is_empty() {
        return Display::Formatted(String::new());
    }
    let result = match format {
        DisplayFormat::Text => Ok(raw.to_owned()),
        DisplayFormat::Integer {
            thousands_separator,
        } => format_integer(raw, *thousands_separator),
        DisplayFormat::Decimal {
            places,
            thousands_separator,
        } => format_decimal(raw, usize::from(*places), *thousands_separator),
        DisplayFormat::Percent { places } => format_percent(raw, usize::from(*places)),
        DisplayFormat::Date(style) => parse_date(raw.trim())
            .map(|date| render_date(date, *style))
            .ok_or(FormatError::InvalidDate),
        DisplayFormat::DateTime {
            style,
            show_seconds,
        } => format_date_time(raw, *style, *show_seconds),
        DisplayFormat::Boolean {
            true_label,
            false_label,
        } => match raw.trim().to_ascii_lowercase().as_str() {
            "true" => Ok(true_label.clone()),
            "false" => Ok(false_label.clone()),
            _ => Err(FormatError::NotABoolean),
        },
        DisplayFormat::Custom(id) => match custom.format(id, raw) {
            None => Err(FormatError::UnknownCustom(id.clone())),
            Some(Ok(text)) => Ok(text),
            Some(Err(message)) => Err(FormatError::CustomFailed(message)),
        },
    };
    match result {
        Ok(text) => Display::Formatted(text),
        Err(error) => Display::Error {
            shown: raw.to_owned(),
            error,
        },
    }
}

/// 10進の符号つき数。桁は文字列のまま持ち、浮動小数点を使わない。
struct Decimal {
    negative: bool,
    /// 整数部（先頭の0は除く。0なら"0"）。
    integer: String,
    /// 小数部。
    fraction: String,
}

// {
//   責務: [parse_decimal: rawを正規形（任意の符号、1桁以上の整数部、任意の小数点と1桁以上の小数部）として読む。3桁区切りや指数表記は読まない]
// }
fn parse_decimal(raw: &str) -> Option<Decimal> {
    let text = raw.trim();
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let (integer, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    let all_digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    let has_point = digits.contains('.');
    if integer.is_empty() || !all_digits(integer) || !all_digits(fraction) {
        return None;
    }
    if has_point && fraction.is_empty() {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    Some(Decimal {
        negative,
        integer: if integer.is_empty() { "0" } else { integer }.to_owned(),
        fraction: fraction.to_owned(),
    })
}

// {
//   責務: [round_digits: 整数部と小数部の桁を、小数placesへ四捨五入（絶対値で5以上は切り上げ）する]
//   戻り値: [(String, String): 丸めた整数部と、ちょうどplaces桁の小数部]
// }
fn round_digits(integer: &str, fraction: &str, places: usize) -> (String, String) {
    let mut digits: Vec<u8> = integer.bytes().map(|b| b - b'0').collect();
    digits.extend(
        fraction
            .bytes()
            .map(|b| b - b'0')
            .chain(std::iter::repeat(0))
            .take(places),
    );
    let round_up = fraction.as_bytes().get(places).is_some_and(|b| *b >= b'5');
    if round_up {
        let mut index = digits.len();
        loop {
            if index == 0 {
                digits.insert(0, 1);
                break;
            }
            index -= 1;
            if digits[index] == 9 {
                digits[index] = 0;
            } else {
                digits[index] += 1;
                break;
            }
        }
    }
    let text: String = digits.iter().map(|d| char::from(b'0' + d)).collect();
    let split = text.len() - places;
    (text[..split].to_owned(), text[split..].to_owned())
}

// {
//   責務: [group_thousands: 整数部の桁を3桁ごとに区切る]
// }
fn group_thousands(integer: &str) -> String {
    let mut grouped = String::with_capacity(integer.len() + integer.len() / 3);
    for (index, ch) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    grouped
}

// {
//   責務: [render_number: 符号・整数部・小数部を表示文字列にする。丸めて0になった値に負の符号を付けない]
// }
fn render_number(negative: bool, integer: &str, fraction: &str, thousands: bool) -> String {
    let is_zero = integer.bytes().all(|b| b == b'0') && fraction.bytes().all(|b| b == b'0');
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let mut text = String::new();
    if negative && !is_zero {
        text.push('-');
    }
    if thousands {
        text.push_str(&group_thousands(integer));
    } else {
        text.push_str(integer);
    }
    if !fraction.is_empty() {
        text.push('.');
        text.push_str(fraction);
    }
    text
}

fn format_integer(raw: &str, thousands: bool) -> Result<String, FormatError> {
    let decimal = parse_decimal(raw).ok_or(FormatError::NotANumber)?;
    if decimal.fraction.bytes().any(|b| b != b'0') {
        return Err(FormatError::NotAnInteger);
    }
    Ok(render_number(
        decimal.negative,
        &decimal.integer,
        "",
        thousands,
    ))
}

fn format_decimal(raw: &str, places: usize, thousands: bool) -> Result<String, FormatError> {
    let decimal = parse_decimal(raw).ok_or(FormatError::NotANumber)?;
    let (integer, fraction) = round_digits(&decimal.integer, &decimal.fraction, places);
    Ok(render_number(
        decimal.negative,
        &integer,
        &fraction,
        thousands,
    ))
}

fn format_percent(raw: &str, places: usize) -> Result<String, FormatError> {
    let decimal = parse_decimal(raw).ok_or(FormatError::NotANumber)?;
    // 100倍は小数点を2桁右へ動かすこと。
    let mut fraction = decimal.fraction.clone();
    while fraction.len() < 2 {
        fraction.push('0');
    }
    let integer = format!("{}{}", decimal.integer, &fraction[..2]);
    let (integer, fraction) = round_digits(&integer, &fraction[2..], places);
    Ok(format!(
        "{}%",
        render_number(decimal.negative, &integer, &fraction, false)
    ))
}

/// 年・月・日。
type Date = (u32, u32, u32);

fn parse_date(text: &str) -> Option<Date> {
    let mut parts = text.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some()
        || year.len() != 4
        || month.len() != 2
        || day.len() != 2
        || ![year, month, day]
            .iter()
            .all(|part| part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let (year, month, day): Date = (year.parse().ok()?, month.parse().ok()?, day.parse().ok()?);
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (1..=days).contains(&day).then_some((year, month, day))
}

fn render_date((year, month, day): Date, style: DateStyle) -> String {
    match style {
        DateStyle::Iso => format!("{year:04}-{month:02}-{day:02}"),
        DateStyle::Slash => format!("{year:04}/{month:02}/{day:02}"),
        DateStyle::Japanese => format!("{year}年{month}月{day}日"),
        DateStyle::DayMonthYear => format!("{day:02}/{month:02}/{year:04}"),
        DateStyle::MonthDayYear => format!("{month:02}/{day:02}/{year:04}"),
    }
}

fn format_date_time(
    raw: &str,
    style: DateStyle,
    show_seconds: bool,
) -> Result<String, FormatError> {
    let text = raw.trim();
    let (date, time) = text
        .split_once('T')
        .or_else(|| text.split_once(' '))
        .ok_or(FormatError::InvalidDateTime)?;
    let date = parse_date(date).ok_or(FormatError::InvalidDateTime)?;
    let mut fields = time.split(':');
    let (hour, minute) = (fields.next(), fields.next());
    let second = fields.next();
    if fields.next().is_some() {
        return Err(FormatError::InvalidDateTime);
    }
    let field = |part: Option<&str>, max: u32| -> Option<u32> {
        let part = part?;
        (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
            .filter(|value| *value <= max)
    };
    let hour = field(hour, 23).ok_or(FormatError::InvalidDateTime)?;
    let minute = field(minute, 59).ok_or(FormatError::InvalidDateTime)?;
    let second = match second {
        None => 0,
        Some(_) => field(second, 59).ok_or(FormatError::InvalidDateTime)?,
    };
    let clock = if show_seconds {
        format!("{hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{hour:02}:{minute:02}")
    };
    Ok(format!("{} {clock}", render_date(date, style)))
}

// {
//   責務: [parse_iso_date: `YYYY-MM-DD`を検証して(年, 月, 日)にする。存在しない日・桁違いはNone。条件付き書式などが同じ日付規則を共有する]
// }
pub(crate) fn parse_iso_date(text: &str) -> Option<(u32, u32, u32)> {
    parse_date(text.trim())
}

// {
//   責務: [compare_canonical_decimals: 2つの正規形の10進文字列を、桁のまま正確に比較する。どちらかが正規形でなければNone（0として扱わない）]
//   処理: [符号が違えば符号で決め、同じなら整数部の桁数、整数部、小数部（短い方を0で補う）の順に比べる。負の数は大小を反転し、-0と0は等しい]
// }
pub(crate) fn compare_canonical_decimals(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let (left, right) = (parse_decimal(left)?, parse_decimal(right)?);
    let is_zero = |d: &Decimal| d.integer == "0" && d.fraction.bytes().all(|b| b == b'0');
    let negative = |d: &Decimal| d.negative && !is_zero(d);
    let magnitude = |a: &Decimal, b: &Decimal| {
        a.integer
            .len()
            .cmp(&b.integer.len())
            .then_with(|| a.integer.cmp(&b.integer))
            .then_with(|| {
                let width = a.fraction.len().max(b.fraction.len());
                let pad = |d: &Decimal| format!("{:0<width$}", d.fraction);
                pad(a).cmp(&pad(b))
            })
    };
    Some(match (negative(&left), negative(&right)) {
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (false, false) => magnitude(&left, &right),
        (true, true) => magnitude(&right, &left),
    })
}
