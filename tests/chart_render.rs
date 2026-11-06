use std::time::{Duration, Instant};

use rowly::process::{
    CsvDocument,
    aggregation::{CellFreshness, InputCell, InputRow, SourceRowRef, rows_from_document},
    chart::{
        ChartDefinition, ChartKind, ChartStyle, NumberFormat, RenderError, ResolvedChart,
        SeriesDefinition, SourceTable, render_svg, resolve,
    },
};
use tempfile::tempdir;

fn table(rows: &[(&str, InputCell)]) -> SourceTable {
    let rows: Vec<InputRow> = rows
        .iter()
        .enumerate()
        .map(|(index, (category, value))| InputRow {
            source_row: SourceRowRef {
                source: "t".into(),
                row: index,
            },
            cells: vec![InputCell::fresh(*category), value.clone()],
        })
        .collect();
    SourceTable::from_input_rows("t", ["k", "v"].map(String::from).to_vec(), &rows)
}

fn chart(kind: ChartKind, table: &SourceTable) -> ResolvedChart {
    let definition = ChartDefinition {
        name: "c".into(),
        kind,
        source_id: "t".into(),
        category_header: Some("k".into()),
        series: vec![SeriesDefinition {
            name: Some("Sales".into()),
            header: "v".into(),
        }],
    };
    resolve(&definition, std::slice::from_ref(table)).unwrap()
}

fn fresh(text: &str) -> InputCell {
    InputCell::fresh(text)
}

fn sample() -> SourceTable {
    table(&[
        ("1", fresh("10")),
        ("2", fresh("25")),
        ("3", fresh("15")),
        ("4", fresh("40")),
    ])
}

#[test]
// {
//   責務: [all_six_kinds_render_their_shapes: 6種類のChartがそれぞれ固有の図形を持つSVGになり、描いた点の数が合うことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn all_six_kinds_render_their_shapes() {
    let table = sample();
    let expected = [
        (ChartKind::Line, "<polyline"),
        (ChartKind::Area, "<polygon"),
        (ChartKind::Column, "<rect x="),
        (ChartKind::Bar, "<rect x="),
        (ChartKind::Scatter, "r=\"4\""),
        (ChartKind::Pie, "<path d="),
    ];
    for (kind, marker) in expected {
        let rendered = render_svg(&chart(kind, &table), &ChartStyle::default()).unwrap();
        assert!(rendered.svg.starts_with("<svg "), "{kind:?}");
        assert!(rendered.svg.ends_with("</svg>"), "{kind:?}");
        assert!(rendered.svg.contains(marker), "{kind:?} lacks {marker}");
        assert_eq!(rendered.plotted_points, 4, "{kind:?}");
        assert_eq!(rendered.omitted_points, 0, "{kind:?}");
    }
}

#[test]
// {
//   責務: [rendering_is_deterministic_and_leaves_the_csv_unchanged: 同じ入力から常に同じSVGが得られ、描画の前後でCSVが変わらないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn rendering_is_deterministic_and_leaves_the_csv_unchanged() {
    let directory = tempdir().unwrap();
    let document = CsvDocument::create(
        directory.path().join("r.csv"),
        vec![
            vec!["k".to_owned(), "v".to_owned()],
            vec!["1".to_owned(), "3".to_owned()],
            vec!["2".to_owned(), "9".to_owned()],
        ],
    )
    .unwrap();
    let snapshot = |document: &CsvDocument| -> Vec<Vec<String>> {
        (0..document.row_count())
            .map(|row| {
                (0..document.column_count())
                    .map(|column| document.cell(row, column).unwrap_or_default().to_string())
                    .collect()
            })
            .collect()
    };
    let before = snapshot(&document);
    let rows = rows_from_document(&document, "t", 1..document.row_count());
    let table = SourceTable::from_input_rows("t", ["k", "v"].map(String::from).to_vec(), &rows);
    let resolved = chart(ChartKind::Line, &table);
    let first = render_svg(&resolved, &ChartStyle::default()).unwrap();
    for _ in 0..5 {
        assert_eq!(
            render_svg(&resolved, &ChartStyle::default()).unwrap(),
            first
        );
    }
    assert_eq!(snapshot(&document), before);
}

#[test]
// {
//   責務: [title_axis_titles_legend_colors_and_number_format_are_applied: title・軸title・legend・series色・数値書式が出力に現れ、legendを消せ、文字がescapeされることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn title_axis_titles_legend_colors_and_number_format_are_applied() {
    let table = table(&[("a", fresh("1000")), ("b", fresh("250000"))]);
    let resolved = chart(ChartKind::Column, &table);
    let style = ChartStyle {
        title: Some("Sales <2026> & \"Q1\"".into()),
        x_axis_title: Some("month".into()),
        y_axis_title: Some("amount".into()),
        series_colors: vec!["#123456".into()],
        number_format: NumberFormat {
            decimals: 0,
            thousands_separator: true,
        },
        ..ChartStyle::default()
    };
    let rendered = render_svg(&resolved, &style).unwrap();
    let svg = &rendered.svg;
    assert!(svg.contains("Sales &lt;2026&gt; &amp; &quot;Q1&quot;"));
    assert!(svg.contains(">month</text>") && svg.contains(">amount</text>"));
    assert!(svg.contains("fill=\"#123456\""));
    assert!(svg.contains(">Sales</text>"), "legend names the series");
    assert!(
        svg.contains(">250,000<") || svg.contains(">300,000<"),
        "thousands separator"
    );
    assert!(svg.contains("<desc>Sales: 2点"), "accessible summary");

    let without_legend = ChartStyle {
        show_legend: false,
        ..style
    };
    assert!(
        !render_svg(&resolved, &without_legend)
            .unwrap()
            .svg
            .contains(">Sales</text>")
    );
}

#[test]
// {
//   責務: [stale_error_and_pending_points_are_marked_apart_from_values: stale / error / pendingの点が値とは別の印で示され、警告文が出て、freshなChartには出ないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn stale_error_and_pending_points_are_marked_apart_from_values() {
    let cell = |text: &str, freshness| InputCell {
        text: text.to_owned(),
        freshness,
    };
    let table = table(&[
        ("1", fresh("10")),
        ("2", cell("99", CellFreshness::Stale)),
        ("3", cell("99", CellFreshness::Error)),
        ("4", cell("99", CellFreshness::Pending)),
        ("5", fresh("")),
    ]);
    let rendered = render_svg(&chart(ChartKind::Line, &table), &ChartStyle::default()).unwrap();
    let svg = &rendered.svg;
    assert!(svg.contains("data-reason=\"stale\""));
    assert!(svg.contains("data-reason=\"error\""));
    assert!(svg.contains("data-reason=\"pending\""));
    assert!(svg.contains("data-role=\"freshness-warning\""));
    assert!(svg.contains("stale 1 / error 1 / pending 1"));
    // 値として描いたのは1点だけ。99は描かれない。
    assert_eq!(rendered.plotted_points, 1);
    assert!(!svg.contains(">99<"));

    let clean = render_svg(&chart(ChartKind::Line, &sample()), &ChartStyle::default()).unwrap();
    assert!(!clean.svg.contains("freshness-warning"));
    assert!(!clean.svg.contains("data-reason"));
}

#[test]
// {
//   責務: [huge_series_are_capped_so_the_ui_is_not_frozen: 巨大なseriesが描く点の上限で間引かれ・切られ、省いた件数が報告され、短時間で終わることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn huge_series_are_capped_so_the_ui_is_not_frozen() {
    let rows: Vec<(String, InputCell)> = (0..50_000)
        .map(|index| (index.to_string(), fresh(&((index % 97) as f64).to_string())))
        .collect();
    let borrowed: Vec<(&str, InputCell)> = rows
        .iter()
        .map(|(category, value)| (category.as_str(), value.clone()))
        .collect();
    let huge = table(&borrowed);

    let started = Instant::now();
    let line = render_svg(&chart(ChartKind::Line, &huge), &ChartStyle::default()).unwrap();
    assert_eq!(
        line.plotted_points,
        rowly::process::chart::MAX_POINTS_PER_SERIES
    );
    assert_eq!(
        line.omitted_points,
        50_000 - rowly::process::chart::MAX_POINTS_PER_SERIES
    );
    let bars = render_svg(&chart(ChartKind::Column, &huge), &ChartStyle::default()).unwrap();
    assert_eq!(bars.plotted_points, rowly::process::chart::MAX_CATEGORIES);
    assert_eq!(
        bars.omitted_points,
        50_000 - rowly::process::chart::MAX_CATEGORIES
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "rendering 50,000 points must be bounded"
    );
    assert!(
        line.svg.len() < 600_000,
        "svg size is bounded: {}",
        line.svg.len()
    );

    // 間引いても先頭と末尾は残り、決定的。
    let again = render_svg(&chart(ChartKind::Line, &huge), &ChartStyle::default()).unwrap();
    assert_eq!(again, line);
}

#[test]
// {
//   責務: [invalid_settings_are_rejected_and_empty_charts_render: 不正な大きさ・色をerrorにし、描ける点が無いChartでもpanicせずSVGを返すことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn invalid_settings_are_rejected_and_empty_charts_render() {
    let resolved = chart(ChartKind::Line, &sample());
    for (width, height) in [(100, 400), (640, 100), (20_000, 400)] {
        let style = ChartStyle {
            width,
            height,
            ..ChartStyle::default()
        };
        assert_eq!(render_svg(&resolved, &style), Err(RenderError::InvalidSize));
    }
    for color in ["red", "#12345", "#12345g", "123456"] {
        let style = ChartStyle {
            series_colors: vec![color.into()],
            ..ChartStyle::default()
        };
        assert_eq!(
            render_svg(&resolved, &style),
            Err(RenderError::InvalidColor(color.into()))
        );
    }

    let nothing = table(&[("a", fresh("")), ("b", fresh("x"))]);
    for kind in [
        ChartKind::Line,
        ChartKind::Area,
        ChartKind::Column,
        ChartKind::Bar,
        ChartKind::Scatter,
        ChartKind::Pie,
    ] {
        let rendered = render_svg(&chart(kind, &nothing), &ChartStyle::default()).unwrap();
        assert_eq!(rendered.plotted_points, 0, "{kind:?}");
        assert!(
            rendered.svg.contains("<desc>Sales: 描ける点なし"),
            "{kind:?}"
        );
    }
}

// 5,000点を2,000点へ間引くときに残る位置(render側のdecimateと同じ式)。
fn kept_by_decimation(len: usize, max: usize) -> Vec<usize> {
    (0..max).map(|step| step * (len - 1) / (max - 1)).collect()
}

#[test]
// {
//   責務: [decimation_keeps_gaps_and_the_true_axis_range: 間引きで欠損点や極値の点が落ちても、折れ線が欠損をまたいでつながらず、軸が実際の最大値を示すことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn decimation_keeps_gaps_and_the_true_axis_range() {
    let kept = kept_by_decimation(5_000, 2_000);
    let dropped = (2_500..4_999).find(|index| !kept.contains(index)).unwrap();
    // 間引きで落ちる位置に、staleの点と極端に大きい値を置く。
    let stale_at = dropped;
    let spike_at = (dropped + 500..4_999)
        .find(|index| !kept.contains(index))
        .unwrap();
    let rows: Vec<(String, InputCell)> = (0..5_000)
        .map(|index| {
            let cell = if index == stale_at {
                InputCell {
                    text: "5".into(),
                    freshness: CellFreshness::Stale,
                }
            } else if index == spike_at {
                fresh("1000")
            } else {
                fresh("1")
            };
            (index.to_string(), cell)
        })
        .collect();
    let borrowed: Vec<(&str, InputCell)> = rows
        .iter()
        .map(|(category, value)| (category.as_str(), value.clone()))
        .collect();
    let rendered = render_svg(
        &chart(ChartKind::Line, &table(&borrowed)),
        &ChartStyle::default(),
    )
    .unwrap();
    // staleの点は落ちても、前後の区間は別の折れ線に分かれる(つながらない)。
    assert_eq!(rendered.svg.matches("<polyline").count(), 2);
    // 極値の点が間引かれても、軸は実際の最大値まで伸びる。
    assert!(rendered.svg.contains(">1000<"));
}

#[test]
// {
//   責務: [extreme_values_do_not_produce_non_finite_output: 極端に大きい有限の値でも、SVGにNaNや無限大を書かずpanicしないことを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn extreme_values_do_not_produce_non_finite_output() {
    let table = table(&[
        ("1", fresh("-1.7e308")),
        ("2", fresh("1.7e308")),
        ("3", fresh("0")),
    ]);
    for kind in [
        ChartKind::Line,
        ChartKind::Area,
        ChartKind::Column,
        ChartKind::Bar,
        ChartKind::Scatter,
    ] {
        let svg = render_svg(&chart(kind, &table), &ChartStyle::default())
            .unwrap()
            .svg;
        assert!(!svg.contains("NaN") && !svg.contains("inf"), "{kind:?}");
    }
}
