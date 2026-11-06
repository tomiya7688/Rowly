use rowly::process::{
    CsvDocument,
    view_style::{
        BorderEdge, BorderEdgeSetting, BorderLine, BorderStyle, CellStyle, Color, HorizontalAlign,
        StyleRule, StyleTarget, TextOverflow, ViewStyle, ViewStyleError,
    },
};
use tempfile::tempdir;

fn headers() -> Vec<String> {
    ["dept", "month", "sales"].map(String::from).to_vec()
}

fn color(text: &str) -> Option<Color> {
    Some(Color::parse(text).unwrap())
}

fn rule(target: StyleTarget, style: CellStyle) -> StyleRule {
    StyleRule { target, style }
}

fn column(name: &str) -> StyleTarget {
    StyleTarget::Column(name.into())
}

fn line(style: BorderStyle) -> BorderEdgeSetting {
    BorderEdgeSetting::Line(BorderLine { style, color: None })
}

#[test]
// {
//   責務: [style_merge_is_deterministic_and_property_wise: styleの統合が入力順で項目ごとに決まり、未指定の項目は他を上書きせず、同じ入力から常に同じ結果になることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn style_merge_is_deterministic_and_property_wise() {
    let mut view = ViewStyle::default();
    view.add_rule(rule(
        column("sales"),
        CellStyle {
            horizontal: Some(HorizontalAlign::Right),
            bold: Some(true),
            background: color("#FFEEAA"),
            ..CellStyle::default()
        },
    ))
    .unwrap();
    view.add_rule(rule(
        StyleTarget::Cell {
            column: "sales".into(),
            row: 2,
        },
        CellStyle {
            bold: Some(false),
            text_color: color("#cc0000"),
            ..CellStyle::default()
        },
    ))
    .unwrap();
    let resolved = view.resolve(&headers());

    // 行2のcell: 後のruleのboldが勝ち、未指定の水平配置と背景は先のruleのまま。
    let style = resolved.style_for(2, 2);
    assert_eq!(style.bold, Some(false));
    assert_eq!(style.horizontal, Some(HorizontalAlign::Right));
    assert_eq!(style.background, color("#ffeeaa"), "色は小文字へ正規化");
    assert_eq!(style.text_color, color("#cc0000"));
    // 行1のcell: 後のruleは当たらない。
    let other = resolved.style_for(1, 2);
    assert_eq!(other.bold, Some(true));
    assert_eq!(other.text_color, None);
    // 当たらないcellは全項目が未指定。
    assert_eq!(resolved.style_for(2, 0), CellStyle::default());

    for _ in 0..5 {
        assert_eq!(view.resolve(&headers()).style_for(2, 2), style);
    }
    // 順序を入れ替えると、後のruleが勝つので結果が変わる(順序が意味を持つ)。
    let reversed = ViewStyle {
        rules: view.rules.iter().rev().cloned().collect(),
        ..ViewStyle::default()
    };
    assert_eq!(
        reversed.resolve(&headers()).style_for(2, 2).bold,
        Some(true)
    );
}

#[test]
// {
//   責務: [all_six_borders_go_through_the_same_operation: 斜めを含む6方向の罫線が同じ操作で指定・統合・消去でき、斜めだけ別の扱いにならないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn all_six_borders_go_through_the_same_operation() {
    assert_eq!(BorderEdge::ALL.len(), 6);
    assert!(BorderEdge::ALL.contains(&BorderEdge::DiagonalDown));
    assert!(BorderEdge::ALL.contains(&BorderEdge::DiagonalUp));

    for edge in BorderEdge::ALL {
        let mut base = CellStyle::default();
        base.set_border(edge, line(BorderStyle::Thin));
        let mut over = CellStyle::default();
        over.set_border(edge, line(BorderStyle::Double));
        // どの辺も、後の指定が先の指定を上書きする。
        assert_eq!(
            base.merged_with(&over).borders.get(&edge),
            Some(&line(BorderStyle::Double)),
            "{edge:?}"
        );
        // 明示的な消去が、先の罫線を消す。
        let mut clear = CellStyle::default();
        clear.set_border(edge, BorderEdgeSetting::Clear);
        assert_eq!(
            base.merged_with(&clear).borders.get(&edge),
            Some(&BorderEdgeSetting::Clear),
            "{edge:?}"
        );
        // 他の辺には影響しない。
        assert_eq!(base.merged_with(&over).borders.len(), 1, "{edge:?}");
    }
    let mut all = CellStyle::default();
    for edge in BorderEdge::ALL {
        all.set_border(edge, line(BorderStyle::Dashed));
    }
    assert_eq!(all.borders.len(), 6);
}

#[test]
// {
//   責務: [targets_keep_header_identity_and_missing_ones_go_dormant: targetが列の見出しで解決され、列の並びが変わっても同じ効き方をし、見出しが無い/重複するruleだけが休眠して他は有効のままで、戻れば再び有効になることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn targets_keep_header_identity_and_missing_ones_go_dormant() {
    let mut view = ViewStyle::default();
    let bold = CellStyle {
        bold: Some(true),
        ..CellStyle::default()
    };
    view.add_rule(rule(column("sales"), bold.clone())).unwrap();
    view.add_rule(rule(column("gone"), bold.clone())).unwrap();
    view.add_rule(rule(
        StyleTarget::Range {
            columns: vec!["dept".into(), "gone".into(), "month".into()],
            first_row: 0,
            last_row: 5,
        },
        bold.clone(),
    ))
    .unwrap();

    let resolved = view.resolve(&headers());
    assert_eq!(resolved.active.len(), 1);
    assert_eq!(resolved.dormant.len(), 2);
    assert_eq!(resolved.dormant[0].index, 1);
    assert_eq!(resolved.dormant[0].missing, ["gone"]);
    assert_eq!(resolved.dormant[1].missing, ["gone"]);
    assert_eq!(resolved.style_for(0, 2).bold, Some(true));
    assert_eq!(
        resolved.style_for(0, 0).bold,
        None,
        "休眠したrangeは効かない"
    );

    // 列の並びが変わっても、見出しで引き直すので同じcellに効く。
    let reordered: Vec<String> = ["sales", "dept", "month"].map(String::from).to_vec();
    assert_eq!(view.resolve(&reordered).style_for(0, 0).bold, Some(true));

    // 範囲に同じ見出しを2回書いても、解決後の列位置は重複しない。
    let mut twice = ViewStyle::default();
    twice
        .add_rule(rule(
            StyleTarget::Range {
                columns: vec!["month".into(), "month".into(), "dept".into()],
                first_row: 0,
                last_row: 1,
            },
            bold.clone(),
        ))
        .unwrap();
    assert_eq!(twice.resolve(&headers()).active[0].columns, [1, 0]);

    // 見出しが戻れば休眠したruleも有効になる。ruleは捨てられていない。
    let mut with_gone = headers();
    with_gone.push("gone".into());
    let back = view.resolve(&with_gone);
    assert!(back.dormant.is_empty());
    assert_eq!(back.active.len(), 3);
    assert_eq!(back.style_for(0, 3).bold, Some(true));
    assert_eq!(view.rules.len(), 3);

    // 見出しが重複すると、どの列か決められないので休眠する。
    let mut duplicated = headers();
    duplicated.push("sales".into());
    let ambiguous = view.resolve(&duplicated);
    assert_eq!(ambiguous.dormant[0].ambiguous, ["sales"]);
}

#[test]
// {
//   責務: [row_heights_range_targets_and_validation: 行高がCSVの行位置で保たれ、rangeが行の範囲に限って効き、不正な指定がerrorになることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn row_heights_range_targets_and_validation() {
    let mut view = ViewStyle::default();
    view.set_row_height(3, 12.5).unwrap();
    view.add_rule(rule(
        StyleTarget::Range {
            columns: vec!["dept".into(), "month".into()],
            first_row: 2,
            last_row: 4,
        },
        CellStyle {
            overflow: Some(TextOverflow::Wrap),
            ..CellStyle::default()
        },
    ))
    .unwrap();
    let resolved = view.resolve(&headers());
    assert_eq!(resolved.row_height_mm(3), Some(12.5));
    assert_eq!(resolved.row_height_mm(2), None);
    for (row, column, expected) in [
        (1, 0, None),
        (2, 0, Some(TextOverflow::Wrap)),
        (4, 1, Some(TextOverflow::Wrap)),
        (5, 1, None),
        (3, 2, None),
    ] {
        assert_eq!(
            resolved.style_for(row, column).overflow,
            expected,
            "({row},{column})"
        );
    }

    for height in [0.0, -1.0, f64::NAN, f64::INFINITY, 1_000.5] {
        assert_eq!(
            view.set_row_height(1, height),
            Err(ViewStyleError::InvalidRowHeight)
        );
    }
    for target in [
        StyleTarget::Column(String::new()),
        StyleTarget::Range {
            columns: vec![],
            first_row: 0,
            last_row: 1,
        },
        StyleTarget::Range {
            columns: vec!["dept".into()],
            first_row: 5,
            last_row: 1,
        },
    ] {
        assert_eq!(
            view.add_rule(rule(target, CellStyle::default())),
            Err(ViewStyleError::InvalidTarget)
        );
    }
    for bad in ["red", "#12345", "#12345g", "123456", "#1234567", ""] {
        assert!(Color::parse(bad).is_none(), "{bad}");
    }
    assert_eq!(Color::parse("#AbCdEf").unwrap().as_str(), "#abcdef");
}

#[test]
// {
//   責務: [styling_never_touches_the_csv: styleの作成・解決・適用がCSVのraw値を変えないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn styling_never_touches_the_csv() {
    let directory = tempdir().unwrap();
    let rows = vec![
        vec!["dept".to_owned(), "month".to_owned(), "sales".to_owned()],
        vec!["east".to_owned(), "1".to_owned(), "10".to_owned()],
    ];
    let document = CsvDocument::create(directory.path().join("s.csv"), rows.clone()).unwrap();
    let mut view = ViewStyle::default();
    view.add_rule(rule(
        column("sales"),
        CellStyle {
            bold: Some(true),
            background: color("#ff0000"),
            ..CellStyle::default()
        },
    ))
    .unwrap();
    let headers: Vec<String> = (0..document.column_count())
        .map(|column| document.cell(0, column).unwrap_or_default().to_string())
        .collect();
    let resolved = view.resolve(&headers);
    assert_eq!(resolved.style_for(1, 2).bold, Some(true));
    for (row, expected) in rows.iter().enumerate() {
        for (column, cell) in expected.iter().enumerate() {
            assert_eq!(document.cell(row, column), Some(cell.as_str()));
        }
    }
    assert!(!document.is_dirty());
}
