use rowly::process::{
    CsvDocument,
    aggregation::CellFreshness,
    conditional_format::{
        CellInput, CompareOp, ConditionalError, ConditionalFormat, ConditionalRule, DerivedInput,
        Diagnostic, Literal, Predicate, ValueSource,
    },
    view_style::{CellStyle, Color, StyleTarget},
};
use tempfile::tempdir;

fn headers() -> Vec<String> {
    ["name", "price", "due", "total"].map(String::from).to_vec()
}

fn background(hex: &str) -> CellStyle {
    CellStyle {
        background: Some(Color::parse(hex).unwrap()),
        ..CellStyle::default()
    }
}

fn rule(
    column: &str,
    source: ValueSource,
    predicate: Predicate,
    style: CellStyle,
) -> ConditionalRule {
    ConditionalRule {
        target: StyleTarget::Column(column.into()),
        source,
        predicate,
        style,
        stop_if_true: false,
    }
}

fn compare(op: CompareOp, literal: Literal) -> Predicate {
    Predicate::Compare { op, literal }
}

fn number(text: &str) -> Literal {
    Literal::Number(text.into())
}

fn raw(text: &str) -> CellInput<'_> {
    CellInput {
        raw: text,
        derived: None,
    }
}

fn format_of(rules: Vec<ConditionalRule>) -> ConditionalFormat {
    let mut format = ConditionalFormat::default();
    for rule in rules {
        format.add_rule(rule).unwrap();
    }
    format
}

#[test]
// {
//   責務: [raw_text_and_typed_number_compare_differently_and_exactly: raw(文字列の辞書順)と明示した数値の比較が別物で、10と9が数値では10の方が大きく、桁の多い小数も正確に比べられ、数値に読めないrawは0や一致とせず診断になることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn raw_text_and_typed_number_compare_differently_and_exactly() {
    let red = background("#ff0000");
    let by_text = format_of(vec![rule(
        "price",
        ValueSource::Raw,
        compare(CompareOp::Gt, Literal::Text("9".into())),
        red.clone(),
    )])
    .resolve(&headers());
    let by_number = format_of(vec![rule(
        "price",
        ValueSource::Number,
        compare(CompareOp::Gt, number("9")),
        red.clone(),
    )])
    .resolve(&headers());
    // 文字列では"10" < "9"、数値では10 > 9。
    assert!(by_text.evaluate(1, 1, &raw("10")).matched.is_empty());
    assert_eq!(by_number.evaluate(1, 1, &raw("10")).matched, [0]);
    assert_eq!(by_number.evaluate(1, 1, &raw("10")).style, red);

    // 浮動小数点では区別できない桁も正確に比べる。
    let exact = format_of(vec![rule(
        "price",
        ValueSource::Number,
        compare(CompareOp::Gt, number("0.1000000000000000000001")),
        red.clone(),
    )])
    .resolve(&headers());
    assert!(
        exact
            .evaluate(0, 1, &raw("0.1000000000000000000001"))
            .matched
            .is_empty()
    );
    assert_eq!(
        exact
            .evaluate(0, 1, &raw("0.1000000000000000000002"))
            .matched,
        [0]
    );
    assert!(
        exact.evaluate(0, 1, &raw("-5")).matched.is_empty(),
        "負の数は小さい"
    );

    // 数値に読めないrawは0として扱わず、診断になる。合ったことにもならない。
    let lt_one = format_of(vec![rule(
        "price",
        ValueSource::Number,
        compare(CompareOp::Lt, number("1")),
        red,
    )])
    .resolve(&headers());
    for text in ["abc", "1,234", "1e3", ".5"] {
        let result = lt_one.evaluate(0, 1, &raw(text));
        assert!(result.matched.is_empty(), "{text}");
        assert_eq!(
            result.diagnostics,
            [(0, Diagnostic::NotANumber(text.into()))],
            "{text}"
        );
    }
    // 空のcellは数値の比較に合わない。診断も出さない。
    let blank = lt_one.evaluate(0, 1, &raw(""));
    assert!(blank.matched.is_empty() && blank.diagnostics.is_empty());
}

#[test]
// {
//   責務: [dates_between_and_text_predicates: 日付の比較と範囲、文字列の含む/始まる/空、論理の組み合わせが正しく評価されることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn dates_between_and_text_predicates() {
    let style = background("#00ff00");
    let overdue = format_of(vec![rule(
        "due",
        ValueSource::Date,
        compare(CompareOp::Lt, Literal::Date("2026-11-07".into())),
        style.clone(),
    )])
    .resolve(&headers());
    assert_eq!(overdue.evaluate(0, 2, &raw("2026-11-06")).matched, [0]);
    assert!(
        overdue
            .evaluate(0, 2, &raw("2026-11-07"))
            .matched
            .is_empty()
    );
    assert_eq!(
        overdue.evaluate(0, 2, &raw("2026-02-30")).diagnostics,
        [(0, Diagnostic::NotADate("2026-02-30".into()))]
    );
    assert!(overdue.evaluate(0, 2, &raw("")).diagnostics.is_empty());

    let in_range = format_of(vec![rule(
        "price",
        ValueSource::Number,
        Predicate::Between {
            low: number("10"),
            high: number("20.5"),
        },
        style.clone(),
    )])
    .resolve(&headers());
    for (text, hit) in [
        ("9.99", false),
        ("10", true),
        ("20.5", true),
        ("20.51", false),
    ] {
        assert_eq!(
            !in_range.evaluate(0, 1, &raw(text)).matched.is_empty(),
            hit,
            "{text}"
        );
    }

    let names = format_of(vec![rule(
        "name",
        ValueSource::Raw,
        Predicate::And(vec![
            Predicate::StartsWith("Ab".into()),
            Predicate::Not(Box::new(Predicate::Contains("x".into()))),
            Predicate::Or(vec![
                Predicate::IsEmpty,
                compare(CompareOp::Ne, Literal::Text("Abc".into())),
            ]),
        ]),
        style,
    )])
    .resolve(&headers());
    for (text, hit) in [
        ("Abd", true),
        ("Abc", false),
        ("Abx", false),
        ("xAb", false),
    ] {
        assert_eq!(
            !names.evaluate(0, 0, &raw(text)).matched.is_empty(),
            hit,
            "{text}"
        );
    }
}

#[test]
// {
//   責務: [stale_error_pending_and_missing_derived_values_are_never_evaluated: 計算結果がstale・error・pending・無しのときは値を評価せず、合ったことにせず診断に残り、freshな計算結果だけが評価されることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn stale_error_pending_and_missing_derived_values_are_never_evaluated() {
    let format = format_of(vec![rule(
        "total",
        ValueSource::DerivedNumber,
        compare(CompareOp::Ge, number("100")),
        background("#ff0000"),
    )])
    .resolve(&headers());
    let derived = |text, freshness| CellInput {
        raw: "",
        derived: Some(DerivedInput { text, freshness }),
    };
    // staleの999は条件(>=100)に合うはずの値だが、通常の値として評価しない。
    for (freshness, diagnostic) in [
        (CellFreshness::Stale, Diagnostic::DerivedStale),
        (CellFreshness::Error, Diagnostic::DerivedError),
        (CellFreshness::Pending, Diagnostic::DerivedPending),
    ] {
        let result = format.evaluate(0, 3, &derived("999", freshness));
        assert!(result.matched.is_empty(), "{freshness:?}");
        assert_eq!(result.style, CellStyle::default(), "{freshness:?}");
        assert_eq!(result.diagnostics, [(0, diagnostic)], "{freshness:?}");
    }
    let missing = format.evaluate(0, 3, &raw("999"));
    assert_eq!(missing.diagnostics, [(0, Diagnostic::DerivedMissing)]);
    assert!(
        missing.matched.is_empty(),
        "rawが条件に合っても計算結果は別物"
    );

    // freshな計算結果だけが評価される。
    assert_eq!(
        format
            .evaluate(0, 3, &derived("100", CellFreshness::Fresh))
            .matched,
        [0]
    );
    assert!(
        format
            .evaluate(0, 3, &derived("99.99", CellFreshness::Fresh))
            .matched
            .is_empty()
    );
    // freshでも数値に読めない結果は0にせず診断になる。
    assert_eq!(
        format
            .evaluate(0, 3, &derived("#N/A", CellFreshness::Fresh))
            .diagnostics,
        [(0, Diagnostic::NotANumber("#N/A".into()))]
    );
}

#[test]
// {
//   責務: [multiple_rules_are_ordered_deterministically_with_stop_if_true: 複数ruleがrule順に、項目ごとに後の明示値が勝つよう決定的に重なり、stop_if_trueで後のruleが評価されず、順序を変えると結果が変わることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn multiple_rules_are_ordered_deterministically_with_stop_if_true() {
    let bold_red = CellStyle {
        bold: Some(true),
        background: Some(Color::parse("#ff0000").unwrap()),
        ..CellStyle::default()
    };
    let blue = background("#0000ff");
    let over_ten = rule(
        "price",
        ValueSource::Number,
        compare(CompareOp::Gt, number("10")),
        bold_red,
    );
    let over_hundred = rule(
        "price",
        ValueSource::Number,
        compare(CompareOp::Gt, number("100")),
        blue.clone(),
    );

    let ordered = format_of(vec![over_ten.clone(), over_hundred.clone()]).resolve(&headers());
    let result = ordered.evaluate(0, 1, &raw("500"));
    assert_eq!(result.matched, [0, 1]);
    // 背景は後のruleが勝ち、未指定の太字は先のruleのまま。
    assert_eq!(result.style.background, blue.background);
    assert_eq!(result.style.bold, Some(true));
    for _ in 0..5 {
        assert_eq!(ordered.evaluate(0, 1, &raw("500")), result);
    }
    // 順序を入れ替えると、後のruleが勝つので背景が変わる。
    let reversed = format_of(vec![over_hundred.clone(), over_ten.clone()]).resolve(&headers());
    assert_eq!(
        reversed.evaluate(0, 1, &raw("500")).style.background,
        Some(Color::parse("#ff0000").unwrap())
    );

    // stop_if_trueは、合ったら後のruleを評価しない(後のruleは合わず、診断も出ない)。
    let mut first = over_hundred.clone();
    first.stop_if_true = true;
    let stopped = format_of(vec![first, over_ten]).resolve(&headers());
    let result = stopped.evaluate(0, 1, &raw("500"));
    assert_eq!(result.matched, [0]);
    assert_eq!(result.style.bold, None);
    // 先のruleが合わなければ、後のruleは評価される。
    assert_eq!(stopped.evaluate(0, 1, &raw("50")).matched, [1]);
}

#[test]
// {
//   責務: [targets_resolve_by_header_and_missing_ones_go_dormant: ruleのtargetが見出しで解決され、列の並びが変わっても同じ効き方をし、見出しが無いruleだけが休眠して他は有効のままで、戻れば再び有効になることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn targets_resolve_by_header_and_missing_ones_go_dormant() {
    let format = format_of(vec![
        rule(
            "price",
            ValueSource::Number,
            compare(CompareOp::Gt, number("1")),
            background("#111111"),
        ),
        rule(
            "gone",
            ValueSource::Raw,
            Predicate::IsNotEmpty,
            background("#222222"),
        ),
    ]);
    let resolved = format.resolve(&headers());
    assert_eq!(resolved.active.len(), 1);
    assert_eq!(resolved.dormant.len(), 1);
    assert_eq!(resolved.dormant[0].index, 1);
    assert_eq!(resolved.dormant[0].missing, ["gone"]);
    assert_eq!(resolved.evaluate(0, 1, &raw("5")).matched, [0]);

    let reordered: Vec<String> = ["total", "due", "price", "name"].map(String::from).to_vec();
    assert_eq!(
        format.resolve(&reordered).evaluate(0, 2, &raw("5")).matched,
        [0]
    );

    let mut with_gone = headers();
    with_gone.push("gone".into());
    let back = format.resolve(&with_gone);
    assert!(back.dormant.is_empty());
    assert_eq!(back.evaluate(0, 4, &raw("x")).matched, [1]);
    assert_eq!(format.rules.len(), 2, "休眠したruleは捨てられない");
}

#[test]
// {
//   責務: [invalid_rules_are_rejected_when_added: 空のtarget・型の合わない右辺・正規形でない数値・存在しない日付・文字列条件の誤用・空の組み合わせ/検索語を、ruleの追加時にerrorにすることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn invalid_rules_are_rejected_when_added() {
    let add = |source, predicate, target: Option<StyleTarget>| {
        let mut format = ConditionalFormat::default();
        let mut candidate = rule("price", source, predicate, CellStyle::default());
        if let Some(target) = target {
            candidate.target = target;
        }
        format.add_rule(candidate)
    };
    let bad_target = add(
        ValueSource::Raw,
        Predicate::IsEmpty,
        Some(StyleTarget::Column(String::new())),
    );
    assert_eq!(bad_target, Err(ConditionalError::InvalidTarget));
    for (source, literal) in [
        (ValueSource::Number, Literal::Text("1".into())),
        (ValueSource::Number, number("1,000")),
        (ValueSource::Number, number("abc")),
        (ValueSource::Date, Literal::Date("2026-02-30".into())),
        (ValueSource::Date, number("1")),
        (ValueSource::Raw, number("1")),
    ] {
        assert_eq!(
            add(source, compare(CompareOp::Eq, literal.clone()), None),
            Err(ConditionalError::LiteralMismatch),
            "{source:?} {literal:?}"
        );
    }
    for (source, predicate) in [
        (ValueSource::Number, Predicate::Contains("1".into())),
        (ValueSource::Date, Predicate::StartsWith("2".into())),
        (ValueSource::Raw, Predicate::Contains(String::new())),
        (ValueSource::Raw, Predicate::And(vec![])),
        (ValueSource::Raw, Predicate::Or(vec![])),
    ] {
        assert_eq!(
            add(source, predicate.clone(), None),
            Err(ConditionalError::InvalidPredicate),
            "{source:?} {predicate:?}"
        );
    }
    // 入れ子の中の誤りも拒否する。
    assert_eq!(
        add(
            ValueSource::Number,
            Predicate::Not(Box::new(Predicate::Contains("x".into()))),
            None
        ),
        Err(ConditionalError::InvalidPredicate)
    );
}

#[test]
// {
//   責務: [evaluation_has_no_side_effects_and_leaves_the_csv_unchanged: 評価がCSVのraw値を変えず副作用が無いことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn evaluation_has_no_side_effects_and_leaves_the_csv_unchanged() {
    let directory = tempdir().unwrap();
    let rows = vec![
        vec!["name".to_owned(), "price".to_owned()],
        vec!["a".to_owned(), "10".to_owned()],
        vec!["b".to_owned(), "oops".to_owned()],
    ];
    let document = CsvDocument::create(directory.path().join("c.csv"), rows.clone()).unwrap();
    let headers: Vec<String> = (0..document.column_count())
        .map(|column| document.cell(0, column).unwrap_or_default().to_string())
        .collect();
    let resolved = format_of(vec![rule(
        "price",
        ValueSource::Number,
        compare(CompareOp::Gt, number("5")),
        background("#ff0000"),
    )])
    .resolve(&headers);
    for row in 1..document.row_count() {
        let text = document.cell(row, 1).unwrap_or_default();
        let _ = resolved.evaluate(row, 1, &raw(text));
    }
    assert_eq!(resolved.evaluate(1, 1, &raw("10")).matched, [0]);
    assert_eq!(resolved.evaluate(2, 1, &raw("oops")).diagnostics.len(), 1);
    for (row, expected) in rows.iter().enumerate() {
        for (column, cell) in expected.iter().enumerate() {
            assert_eq!(document.cell(row, column), Some(cell.as_str()));
        }
    }
    assert!(!document.is_dirty());
}
