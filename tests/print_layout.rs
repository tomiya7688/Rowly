use rowly::process::print_layout::{
    HeaderFooter, HeaderFooterContext, Margins, Orientation, PageOrder, PageSetup, PaperSize,
    PrintLayoutError, RowHeights, Scale, TableMetrics, paginate,
};

// A4縦・余白15mm → 本文は180mm x 267mm。
fn setup() -> PageSetup {
    PageSetup::default()
}

fn table(columns: usize, column_mm: f64, rows: usize, row_mm: f64) -> TableMetrics {
    TableMetrics {
        column_widths_mm: vec![column_mm; columns],
        row_heights: RowHeights::Uniform {
            count: rows,
            height_mm: row_mm,
        },
        header_rows: 1,
    }
}

#[test]
// {
//   責務: [page_count_is_deterministic_and_rows_split_at_the_page_height: 同じ入力から常に同じページ数・境界が得られ、行が本文の高さで分かれることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn page_count_is_deterministic_and_rows_split_at_the_page_height() {
    // 本文267mm / 行10mm → 1ページ目は26行（見出し含む）。
    let metrics = table(2, 40.0, 100, 10.0);
    let first = paginate(&setup(), &metrics).unwrap();
    for _ in 0..5 {
        assert_eq!(paginate(&setup(), &metrics).unwrap(), first);
    }
    assert_eq!(first.pages[0].rows, 0..26);
    // 2ページ目以降は見出し行の分(10mm)だけ減り、25行。
    assert_eq!(first.pages[1].rows, 26..51);
    assert!(first.pages[1].repeats_header);
    assert!(!first.pages[0].repeats_header);
    assert_eq!(first.row_breaks, [26, 51, 76]);
    assert_eq!(first.page_count(), 4);
    assert_eq!(first.pages.last().unwrap().rows, 76..100);
    assert!(first.column_breaks.is_empty());
    assert_eq!(first.scale_basis_points, 10_000);
}

#[test]
// {
//   責務: [repeat_header_can_be_turned_off_and_is_skipped_when_it_cannot_fit: 見出し繰り返しを切ると全ページが同じ行数になり、見出しがページ高さを超えるなら繰り返しを取りやめることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn repeat_header_can_be_turned_off_and_is_skipped_when_it_cannot_fit() {
    let mut off = setup();
    off.repeat_header_row = false;
    let layout = paginate(&off, &table(2, 40.0, 100, 10.0)).unwrap();
    assert_eq!(layout.pages[1].rows, 26..52);
    assert!(layout.pages.iter().all(|page| !page.repeats_header));

    let mut metrics = table(2, 40.0, 10, 10.0);
    metrics.row_heights = RowHeights::Each(vec![400.0, 10.0, 10.0]);
    let layout = paginate(&setup(), &metrics).unwrap();
    assert!(layout.header_repeat_skipped);
    assert!(layout.pages.iter().all(|page| !page.repeats_header));
    assert_eq!(layout.oversized_rows, [0]);
}

#[test]
// {
//   責務: [fit_width_shrinks_a_wide_table_onto_one_page_width: 幅広の表がFitWidthで1ページの幅に収まり、狭い表は拡大されず、固定倍率では列が複数ページに分かれることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn fit_width_shrinks_a_wide_table_onto_one_page_width() {
    // 総幅 360mm、本文幅 180mm → 50%。
    let wide = table(6, 60.0, 10, 10.0);
    let mut fit = setup();
    fit.scale = Scale::FitWidth;
    let layout = paginate(&fit, &wide).unwrap();
    assert_eq!(layout.scale_basis_points, 5_000);
    assert!(layout.column_breaks.is_empty());
    assert_eq!(layout.page_count(), 1);

    let narrow = table(2, 40.0, 10, 10.0);
    assert_eq!(paginate(&fit, &narrow).unwrap().scale_basis_points, 10_000);

    // 固定100%なら列が分かれる: 180mm / 60mm = 3列ずつ。
    let layout = paginate(&setup(), &wide).unwrap();
    assert_eq!(layout.column_breaks, [3]);
    assert_eq!(layout.page_count(), 2);
    assert_eq!(layout.pages[1].columns, 3..6);
}

#[test]
// {
//   責務: [orientation_paper_margins_and_scale_change_the_printable_area: 横向き・用紙・余白・固定倍率が本文の領域を変え、ページ順で並び方が変わることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn orientation_paper_margins_and_scale_change_the_printable_area() {
    let metrics = table(6, 60.0, 30, 10.0);
    let portrait = paginate(&setup(), &metrics).unwrap();
    let mut landscape = setup();
    landscape.orientation = Orientation::Landscape;
    // 横向きA4 は本文 267mm 幅 → 4列(240mm)。
    let layout = paginate(&landscape, &metrics).unwrap();
    assert_eq!(layout.column_breaks, [4]);
    assert_ne!(layout.pages, portrait.pages);

    let mut half = setup();
    half.scale = Scale::Percent(50);
    let layout = paginate(&half, &metrics).unwrap();
    assert!(layout.column_breaks.is_empty());
    assert_eq!(layout.scale_basis_points, 5_000);

    let mut a3 = setup();
    a3.paper = PaperSize::A3;
    a3.orientation = Orientation::Landscape;
    a3.margins = Margins::uniform(10.0);
    let layout = paginate(&a3, &metrics).unwrap();
    assert!(layout.column_breaks.is_empty());

    // 列が2グループ・行が2グループなら、ページ順で並びが変わる。
    let grid = table(6, 60.0, 60, 10.0);
    let down = paginate(&setup(), &grid).unwrap();
    let mut over_setup = setup();
    over_setup.page_order = PageOrder::OverThenDown;
    let over = paginate(&over_setup, &grid).unwrap();
    assert_eq!(down.page_count(), over.page_count());
    assert_eq!(down.pages[1].columns, 0..3);
    assert_eq!(over.pages[1].columns, 3..6);
    assert_eq!(over.pages[1].rows, 0..26);
}

#[test]
// {
//   責務: [header_footer_reserve_space_and_expand_tokens: 空でないヘッダー/フッターが本文の高さを減らし、tokenが展開されることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn header_footer_reserve_space_and_expand_tokens() {
    let metrics = table(2, 40.0, 100, 10.0);
    let mut with_footer = setup();
    with_footer.footer = HeaderFooter {
        left: "{file}".into(),
        center: "{view}".into(),
        right: "{page}/{pages}".into(),
    };
    // 本文267mm - フッター帯8mm = 259mm → 25行。
    let layout = paginate(&with_footer, &metrics).unwrap();
    assert_eq!(layout.pages[0].rows, 0..25);

    let (left, center, right) = with_footer.footer.expand(HeaderFooterContext {
        page: 2,
        pages: layout.page_count(),
        file: "sales.csv",
        view: "East only",
    });
    assert_eq!(left, "sales.csv");
    assert_eq!(center, "East only");
    assert_eq!(right, format!("2/{}", layout.page_count()));
}

#[test]
// {
//   責務: [empty_tables_oversized_columns_and_invalid_input: 行・列が無い表はページ0、幅を超える列は単独ページで記録、不正な寸法・倍率・余白はerrorになることを検証する。],
//   戻り値: [(): 全assertion成功時に完了する。]
// }
fn empty_tables_oversized_columns_and_invalid_input() {
    assert_eq!(
        paginate(&setup(), &table(0, 40.0, 10, 10.0))
            .unwrap()
            .page_count(),
        0
    );
    let mut no_rows = table(2, 40.0, 0, 10.0);
    no_rows.header_rows = 0;
    assert_eq!(paginate(&setup(), &no_rows).unwrap().page_count(), 0);

    let mut metrics = table(3, 40.0, 5, 10.0);
    metrics.column_widths_mm[1] = 500.0;
    let layout = paginate(&setup(), &metrics).unwrap();
    assert_eq!(layout.oversized_columns, [1]);
    assert_eq!(layout.column_breaks, [1, 2]);

    let bad_scale = PageSetup {
        scale: Scale::Percent(5),
        ..setup()
    };
    assert_eq!(
        paginate(&bad_scale, &table(1, 40.0, 1, 10.0)),
        Err(PrintLayoutError::InvalidScale)
    );
    let bad_margin = PageSetup {
        margins: Margins::uniform(-1.0),
        ..setup()
    };
    assert_eq!(
        paginate(&bad_margin, &table(1, 40.0, 1, 10.0)),
        Err(PrintLayoutError::InvalidMargin)
    );
    let no_area = PageSetup {
        margins: Margins::uniform(200.0),
        ..setup()
    };
    assert_eq!(
        paginate(&no_area, &table(1, 40.0, 1, 10.0)),
        Err(PrintLayoutError::NoPrintableArea)
    );
    assert!(matches!(
        paginate(&setup(), &table(1, f64::NAN, 1, 10.0)),
        Err(PrintLayoutError::InvalidLength(_))
    ));
    let mut too_many_headers = table(1, 40.0, 2, 10.0);
    too_many_headers.header_rows = 3;
    assert_eq!(
        paginate(&setup(), &too_many_headers),
        Err(PrintLayoutError::HeaderRowsExceedRows)
    );
}
