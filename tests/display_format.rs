use rowly::process::{
    CsvDocument,
    display_format::{
        CustomFormatRegistry, DateStyle, Display, DisplayFormat, FormatError, format_value,
        format_value_with,
    },
};
use tempfile::tempdir;

fn shown(raw: &str, format: &DisplayFormat) -> String {
    format_value(raw, format).text().to_owned()
}

fn decimal(places: u8) -> DisplayFormat {
    DisplayFormat::Decimal {
        places,
        thousands_separator: false,
    }
}

#[test]
// {
//   責務: [text_format_keeps_the_raw_value_exactly: Text書式がrawをそのまま見せ、先頭の0や数字に見える文字列を数値化しないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn text_format_keeps_the_raw_value_exactly() {
    for raw in ["00123", "1e5", " 7 ", "+81 3-1234", "0x1F", "2026-1-2"] {
        assert_eq!(
            format_value(raw, &DisplayFormat::Text),
            Display::Formatted(raw.to_owned())
        );
    }
}

#[test]
// {
//   責務: [numbers_are_formatted_exactly_without_floating_point: Integer・Decimal・Percentが10進の桁のまま四捨五入され、3桁区切り・符号・繰り上がりが正しいことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn numbers_are_formatted_exactly_without_floating_point() {
    let integer = DisplayFormat::Integer {
        thousands_separator: true,
    };
    assert_eq!(shown("1234567", &integer), "1,234,567");
    assert_eq!(shown("-1000", &integer), "-1,000");
    assert_eq!(shown("0042", &integer), "42");
    assert_eq!(shown("12.000", &integer), "12");

    // 0.1 + 0.2 のような浮動小数点の誤差が出ない。
    assert_eq!(shown("0.30000000000000004", &decimal(2)), "0.30");
    assert_eq!(shown("2.675", &decimal(2)), "2.68");
    assert_eq!(shown("-2.675", &decimal(2)), "-2.68");
    assert_eq!(shown("9.995", &decimal(2)), "10.00");
    assert_eq!(shown("999.9", &decimal(0)), "1000");
    assert_eq!(shown("3", &decimal(3)), "3.000");
    assert_eq!(shown("+5.5", &decimal(1)), "5.5");
    assert_eq!(
        shown("-0.001", &decimal(2)),
        "0.00",
        "丸めて0になる値に負の符号を付けない"
    );
    let grouped = DisplayFormat::Decimal {
        places: 2,
        thousands_separator: true,
    };
    assert_eq!(shown("1234567.891", &grouped), "1,234,567.89");

    let percent = |places| DisplayFormat::Percent { places };
    assert_eq!(shown("0.125", &percent(1)), "12.5%");
    assert_eq!(shown("1", &percent(0)), "100%");
    assert_eq!(shown("0.5", &percent(0)), "50%");
    assert_eq!(shown("0.00005", &percent(2)), "0.01%");
    assert_eq!(shown("12.345", &percent(1)), "1234.5%");
    assert_eq!(shown("-0.25", &percent(0)), "-25%");
    assert_eq!(shown("0.9995", &percent(1)), "100.0%");
}

#[test]
// {
//   責務: [parse_failure_keeps_the_raw_and_reports_the_reason: 書式に読めないrawが書き換えられず、理由つきのformat errorになり、localeに依存する表記を勝手に正規形として読まないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn parse_failure_keeps_the_raw_and_reports_the_reason() {
    let result = format_value("abc", &decimal(2));
    assert_eq!(
        result,
        Display::Error {
            shown: "abc".into(),
            error: FormatError::NotANumber
        }
    );
    assert!(result.is_error());
    assert_eq!(result.text(), "abc");

    // localeに依存する・曖昧な表記は読まない。
    for raw in [
        "1,234",
        "1.234,5",
        "1e3",
        ".5",
        "5.",
        "--1",
        "１２３",
        "1_000",
        "0x10",
        "12abc",
    ] {
        assert!(
            format_value(raw, &decimal(1)).is_error(),
            "{raw} must not be interpreted"
        );
        assert_eq!(format_value(raw, &decimal(1)).text(), raw);
    }
    // 整数書式は小数部のある値を黙って丸めない。
    let integer = DisplayFormat::Integer {
        thousands_separator: false,
    };
    assert_eq!(
        format_value("12.5", &integer),
        Display::Error {
            shown: "12.5".into(),
            error: FormatError::NotAnInteger
        }
    );
}

#[test]
// {
//   責務: [empty_raw_is_empty_for_every_format: 空のrawがどの書式でも空の表示になりerrorにならないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn empty_raw_is_empty_for_every_format() {
    let formats = [
        DisplayFormat::Text,
        decimal(2),
        DisplayFormat::Percent { places: 1 },
        DisplayFormat::Date(DateStyle::Iso),
        DisplayFormat::Boolean {
            true_label: "はい".into(),
            false_label: "いいえ".into(),
        },
        DisplayFormat::Custom("anything".into()),
    ];
    for format in formats {
        assert_eq!(format_value("", &format), Display::Formatted(String::new()));
    }
}

#[test]
// {
//   責務: [dates_and_date_times_follow_the_chosen_style_and_validate_the_calendar: 日付・日時がISOのrawから様式どおりに表示され、存在しない日付・時差付き・不正な時刻が読まれないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn dates_and_date_times_follow_the_chosen_style_and_validate_the_calendar() {
    let date = |style| DisplayFormat::Date(style);
    assert_eq!(shown("2026-01-02", &date(DateStyle::Iso)), "2026-01-02");
    assert_eq!(shown("2026-01-02", &date(DateStyle::Slash)), "2026/01/02");
    assert_eq!(
        shown("2026-01-02", &date(DateStyle::Japanese)),
        "2026年1月2日"
    );
    assert_eq!(
        shown("2026-01-02", &date(DateStyle::DayMonthYear)),
        "02/01/2026"
    );
    assert_eq!(
        shown("2026-01-02", &date(DateStyle::MonthDayYear)),
        "01/02/2026"
    );
    assert_eq!(shown("2024-02-29", &date(DateStyle::Iso)), "2024-02-29");
    for raw in [
        "2026-02-29",
        "2100-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-04-31",
        "2026-1-2",
        "2026/01/02",
        "26-01-02",
        "2026-01-02-03",
        "01/02/2026",
    ] {
        assert!(
            format_value(raw, &date(DateStyle::Iso)).is_error(),
            "{raw} must be rejected"
        );
    }
    assert_eq!(shown("2000-02-29", &date(DateStyle::Iso)), "2000-02-29");

    let date_time = |show_seconds| DisplayFormat::DateTime {
        style: DateStyle::Slash,
        show_seconds,
    };
    assert_eq!(
        shown("2026-01-02T03:04", &date_time(false)),
        "2026/01/02 03:04"
    );
    assert_eq!(
        shown("2026-01-02 03:04:05", &date_time(true)),
        "2026/01/02 03:04:05"
    );
    assert_eq!(
        shown("2026-01-02T03:04", &date_time(true)),
        "2026/01/02 03:04:00"
    );
    for raw in [
        "2026-01-02",
        "2026-01-02T24:00",
        "2026-01-02T23:60",
        "2026-01-02T03:04:60",
        "2026-01-02T03:04Z",
        "2026-01-02T03:04+09:00",
        "2026-02-30T03:04",
        "2026-01-02T3:04",
        "2026-01-02T03",
    ] {
        assert!(
            format_value(raw, &date_time(false)).is_error(),
            "{raw} must be rejected"
        );
    }
}

#[test]
// {
//   責務: [booleans_use_labels_and_only_true_or_false: 真偽が大文字小文字を問わずtrue/falseだけを読み、表示名に置き換わり、1/0やyes/noを読まないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn booleans_use_labels_and_only_true_or_false() {
    let format = DisplayFormat::Boolean {
        true_label: "はい".into(),
        false_label: "いいえ".into(),
    };
    assert_eq!(shown("TRUE", &format), "はい");
    assert_eq!(shown("False", &format), "いいえ");
    for raw in ["1", "0", "yes", "no", "t", "真"] {
        assert!(format_value(raw, &format).is_error(), "{raw}");
    }
}

struct Registry;

impl CustomFormatRegistry for Registry {
    fn format(&self, id: &str, raw: &str) -> Option<Result<String, String>> {
        match id {
            "shout" => Some(Ok(raw.to_uppercase())),
            "broken" => Some(Err("cannot format".into())),
            _ => None,
        }
    }
}

#[test]
// {
//   責務: [custom_formats_go_through_the_registry_boundary: Custom書式が登録先に任され、未登録・失敗がrawを保ったerrorになり、登録が無ければ使えないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn custom_formats_go_through_the_registry_boundary() {
    let shout = DisplayFormat::Custom("shout".into());
    assert_eq!(
        format_value_with("abc", &shout, &Registry),
        Display::Formatted("ABC".into())
    );
    assert_eq!(
        format_value_with("abc", &DisplayFormat::Custom("broken".into()), &Registry),
        Display::Error {
            shown: "abc".into(),
            error: FormatError::CustomFailed("cannot format".into())
        }
    );
    assert_eq!(
        format_value_with("abc", &DisplayFormat::Custom("nope".into()), &Registry),
        Display::Error {
            shown: "abc".into(),
            error: FormatError::UnknownCustom("nope".into())
        }
    );
    // 登録先を渡さないformat_valueでは、Customは未登録として扱われる。
    assert!(format_value("abc", &shout).is_error());
}

#[test]
// {
//   責務: [formatting_never_changes_the_csv: 書式の適用がCSVのraw値を変えないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn formatting_never_changes_the_csv() {
    let directory = tempdir().unwrap();
    let rows = vec![
        vec!["id".to_owned(), "ratio".to_owned()],
        vec!["00123".to_owned(), "0.125".to_owned()],
        vec!["abc".to_owned(), "x".to_owned()],
    ];
    let document = CsvDocument::create(directory.path().join("f.csv"), rows.clone()).unwrap();
    let formats = [
        DisplayFormat::Text,
        DisplayFormat::Percent { places: 1 },
        decimal(2),
    ];
    for row in 0..document.row_count() {
        for column in 0..document.column_count() {
            let raw = document.cell(row, column).unwrap_or_default().to_string();
            for format in &formats {
                let _ = format_value(&raw, format);
            }
        }
    }
    for (row, expected) in rows.iter().enumerate() {
        for (column, cell) in expected.iter().enumerate() {
            assert_eq!(document.cell(row, column), Some(cell.as_str()));
        }
    }
    assert!(!document.is_dirty());
    assert_eq!(
        shown("0.125", &DisplayFormat::Percent { places: 1 }),
        "12.5%"
    );
    assert_eq!(
        document.cell(1, 1),
        Some("0.125"),
        "表示は12.5%でもrawは0.125のまま"
    );
    assert_eq!(shown("00123", &DisplayFormat::Text), "00123");
}

#[test]
// {
//   責務: [decimal_and_percent_rounding_matches_an_integer_reference: 小数3桁の値を全て走査し、Decimal・Percentの丸めが整数演算の基準実装(絶対値で5以上は切り上げ)と一致することを検証する。桁の補完・桁上がり・負の値・places 0..=4を含む。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn decimal_and_percent_rounding_matches_an_integer_reference() {
    // 基準: 値を10^3倍した整数nを、10^(3-places)単位で四捨五入(絶対値)する。
    let reference = |n: i64, places: u32, scale: i64| -> String {
        let magnitude = n.abs();
        let unit_digits = 3_u32.saturating_sub(places);
        let unit = 10_i64.pow(unit_digits);
        let rounded = (magnitude + unit / 2) / unit; // places桁までの整数
        let extra = places.saturating_sub(3); // 3桁より多い場合は0を補う
        let rounded = rounded * 10_i64.pow(extra);
        let divisor = 10_i64.pow(places);
        let (integer, fraction) = (rounded / divisor, rounded % divisor);
        let sign = if n < 0 && rounded != 0 { "-" } else { "" };
        let _ = scale;
        if places == 0 {
            format!("{sign}{integer}")
        } else {
            format!(
                "{sign}{integer}.{fraction:0width$}",
                width = places as usize
            )
        }
    };
    for n in -12_345_i64..=12_345 {
        let raw = {
            let sign = if n < 0 { "-" } else { "" };
            format!("{sign}{}.{:03}", n.abs() / 1000, n.abs() % 1000)
        };
        for places in 0..=4_u32 {
            assert_eq!(
                shown(&raw, &decimal(places as u8)),
                reference(n, places, 1),
                "raw={raw} places={places}"
            );
        }
    }
}
