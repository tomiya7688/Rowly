//! ```text
//! 責務: [print_layout: Page Setup modelと、表をページへ分ける計算を提供する。CSVには一切触れない純粋な計算である]
//! 処理: [1: 用紙・向き・余白・倍率からページに置ける領域を求める 2: 列を左から詰めて列方向のページ境界を決める 3: 行を上から詰めて行方向のページ境界を決め、2ページ目以降に見出し行を繰り返す]
//! 補足: [長さはすべて整数のマイクロメートルで計算し、倍率はベーシスポイント（1% = 100）で持つ。同じ入力なら常に同じページ数・同じ境界になる。ページ分割のためにCSVへ空行を足さず、列幅もCSVへ書かない]
//! ```
use std::ops::Range;

mod setup;
pub use setup::{
    HeaderFooter, HeaderFooterContext, Margins, Orientation, PageOrder, PageSetup, PaperSize, Scale,
};

/// 1mmあたりのマイクロメートル。
const MICROMETERS_PER_MM: f64 = 1000.0;
/// 倍率の最小・最大（パーセント）。
const MIN_SCALE_PERCENT: u16 = 10;
const MAX_SCALE_PERCENT: u16 = 400;
/// ヘッダー/フッターが空でないときに本文から取り分ける高さ（mm）。
const HEADER_FOOTER_BAND_MM: f64 = 8.0;
/// 1つの長さ（用紙・余白・列幅・行高）の上限（mm）。整数演算がオーバーフローしないよう、これを超える値はerrorにする。
const MAX_LENGTH_MM: f64 = 100_000.0;
/// 行数の上限。上限の長さと倍率を掛けた合計でも`i64`に収まる。
const MAX_ROWS: usize = 100_000_000;

/// 行の高さ。
#[derive(Debug, Clone, PartialEq)]
pub enum RowHeights {
    /// 全行が同じ高さ。
    Uniform {
        /// 行数（見出し行を含む）。
        count: usize,
        /// 1行の高さ（mm）。
        height_mm: f64,
    },
    /// 行ごとの高さ（mm。見出し行を含む）。
    Each(Vec<f64>),
}

/// 表の大きさ（倍率100%での寸法）。列幅・行高はView側の表示設定から呼び出し側が渡す。
#[derive(Debug, Clone, PartialEq)]
pub struct TableMetrics {
    /// 列幅（mm）。
    pub column_widths_mm: Vec<f64>,
    /// 行の高さ。
    pub row_heights: RowHeights,
    /// 先頭から数えた見出し行の数。繰り返しの対象になる。
    pub header_rows: usize,
}

/// 1ページ分の範囲。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// ページ番号（1始まり）。
    pub number: usize,
    /// 載せる行の範囲（見出し行を含む元の行番号）。
    pub rows: Range<usize>,
    /// 載せる列の範囲。
    pub columns: Range<usize>,
    /// 見出し行を先頭に繰り返して置くか。`rows`の外にある見出し行を足す場合だけtrue。
    pub repeats_header: bool,
}

/// ページ分割の結果。
#[derive(Debug, Clone, PartialEq)]
pub struct PageLayout {
    /// 実際に使った倍率（ベーシスポイント。100% = 10000）。
    pub scale_basis_points: u32,
    /// ページ順に並んだページ。
    pub pages: Vec<Page>,
    /// 行方向の改ページ位置（各ページの先頭行。最初のページは含まない）。
    pub row_breaks: Vec<usize>,
    /// 列方向の改ページ位置（各ページの先頭列。最初のページは含まない）。
    pub column_breaks: Vec<usize>,
    /// 1ページの幅に収まらず、単独で1ページを占めた列。
    pub oversized_columns: Vec<usize>,
    /// 1ページの高さに収まらず、単独で1ページを占めた行。
    pub oversized_rows: Vec<usize>,
    /// 見出し行がページ高さを超えるため、繰り返しを取りやめたか。
    pub header_repeat_skipped: bool,
}

impl PageLayout {
    /// 総ページ数。
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }
}

/// Page Setupまたは表の寸法の不備。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PrintLayoutError {
    /// 寸法が正の有限値でない。
    #[error("{0} must be a positive finite length")]
    InvalidLength(&'static str),
    /// 余白が負、または有限でない。
    #[error("margins must be finite and not negative")]
    InvalidMargin,
    /// 余白・ヘッダー・フッターで本文の領域が残らない。
    #[error("margins and header/footer leave no printable area")]
    NoPrintableArea,
    /// 倍率が範囲外。
    #[error("scale must be between 10% and 400%")]
    InvalidScale,
    /// 見出し行の数が行数を超える。
    #[error("header row count exceeds the row count")]
    HeaderRowsExceedRows,
}

// {
//   責務: [to_um: mmをマイクロメートルの整数にする（四捨五入）]
// }
fn to_um(mm: f64) -> i64 {
    (mm * MICROMETERS_PER_MM).round() as i64
}

// {
//   責務: [scaled: 長さへ倍率を掛ける（切り上げ。縮小・拡大とも、ページに収まる側へ誤差を寄せない）]
// }
fn scaled(length_um: i64, basis_points: u32) -> i64 {
    // 呼び出し元はpositiveを通した正の長さだけを渡す（負の値では切り上げにならない）。
    debug_assert!(length_um >= 0);
    let product = i128::from(length_um) * i128::from(basis_points);
    ((product + 9_999) / 10_000) as i64
}

fn positive(value: f64, name: &'static str) -> Result<i64, PrintLayoutError> {
    if value.is_finite() && value > 0.0 && value <= MAX_LENGTH_MM {
        Ok(to_um(value))
    } else {
        Err(PrintLayoutError::InvalidLength(name))
    }
}

// {
//   責務: [paginate: Page Setupと表の寸法から、決定的なページ分割を計算する]
//   処理: [向きに応じた用紙から余白とヘッダー/フッター帯を引いた領域を求め、倍率（FitWidthは表の総幅から決める）を掛けた列幅で列を、行高で行を詰める。2ページ目以降は見出し行を先頭に足し、その分だけ本文の高さを減らす]
//   引数: [setup: Page Setup, table: 倍率100%での表の寸法]
//   戻り値: [Result<PageLayout, PrintLayoutError>: ページ分割。行または列が無ければページは0。寸法の不備はerror]
// }
pub fn paginate(setup: &PageSetup, table: &TableMetrics) -> Result<PageLayout, PrintLayoutError> {
    let (paper_width, paper_height) = setup.paper.dimensions_mm();
    let (paper_width, paper_height) = match setup.orientation {
        Orientation::Portrait => (paper_width, paper_height),
        Orientation::Landscape => (paper_height, paper_width),
    };
    let paper_width = positive(paper_width, "paper width")?;
    let paper_height = positive(paper_height, "paper height")?;
    let margins = [
        setup.margins.top_mm,
        setup.margins.bottom_mm,
        setup.margins.left_mm,
        setup.margins.right_mm,
    ];
    if margins
        .iter()
        .any(|m| !m.is_finite() || *m < 0.0 || *m > MAX_LENGTH_MM)
    {
        return Err(PrintLayoutError::InvalidMargin);
    }
    let band = to_um(HEADER_FOOTER_BAND_MM);
    let header_band = if setup.header.is_empty() { 0 } else { band };
    let footer_band = if setup.footer.is_empty() { 0 } else { band };
    let printable_width =
        paper_width - to_um(setup.margins.left_mm) - to_um(setup.margins.right_mm);
    let printable_height = paper_height
        - to_um(setup.margins.top_mm)
        - to_um(setup.margins.bottom_mm)
        - header_band
        - footer_band;
    if printable_width <= 0 || printable_height <= 0 {
        return Err(PrintLayoutError::NoPrintableArea);
    }

    let widths: Vec<i64> = table
        .column_widths_mm
        .iter()
        .map(|width| positive(*width, "column width"))
        .collect::<Result<_, _>>()?;
    let heights: Vec<i64> = match &table.row_heights {
        RowHeights::Uniform { count, height_mm } => {
            if *count > MAX_ROWS {
                return Err(PrintLayoutError::InvalidLength("row count"));
            }
            vec![positive(*height_mm, "row height")?; *count]
        }
        RowHeights::Each(each) => each
            .iter()
            .map(|height| positive(*height, "row height"))
            .collect::<Result<_, _>>()?,
    };
    if table.header_rows > heights.len() {
        return Err(PrintLayoutError::HeaderRowsExceedRows);
    }

    let basis_points = match setup.scale {
        Scale::Percent(percent) => {
            if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&percent) {
                return Err(PrintLayoutError::InvalidScale);
            }
            u32::from(percent) * 100
        }
        Scale::FitWidth => {
            let total: i64 = widths.iter().sum();
            if total <= printable_width || total == 0 {
                10_000
            } else {
                // 収まる最大の倍率（切り捨て）。列ごとの切り上げが累積して溢れる場合は、
                // 列数だけ（1列あたり最大1um）余裕を見込んだ倍率に下げる。最小倍率で下限を持つ。
                let fit = |width: i64| (i128::from(width) * 10_000 / i128::from(total)) as u32;
                let exact = fit(printable_width);
                let fits = |basis_points: u32| {
                    widths
                        .iter()
                        .map(|width| scaled(*width, basis_points))
                        .sum::<i64>()
                        <= printable_width
                };
                let columns = i64::try_from(widths.len()).unwrap_or(i64::MAX);
                let chosen = if fits(exact) {
                    exact
                } else {
                    fit((printable_width - columns).max(0))
                };
                chosen.max(MIN_SCALE_PERCENT as u32 * 100)
            }
        }
    };

    let mut layout = PageLayout {
        scale_basis_points: basis_points,
        pages: Vec::new(),
        row_breaks: Vec::new(),
        column_breaks: Vec::new(),
        oversized_columns: Vec::new(),
        oversized_rows: Vec::new(),
        header_repeat_skipped: false,
    };
    if widths.is_empty() || heights.is_empty() {
        return Ok(layout);
    }

    let column_groups = pack_columns(&widths, basis_points, printable_width, &mut layout);
    let row_groups = pack_rows(
        &heights,
        table.header_rows,
        setup.repeat_header_row,
        basis_points,
        printable_height,
        &mut layout,
    );
    layout.column_breaks = column_groups.iter().skip(1).map(|g| g.start).collect();
    layout.row_breaks = row_groups.iter().skip(1).map(|g| g.0.start).collect();

    let mut push = |rows: &(Range<usize>, bool), columns: &Range<usize>| {
        let number = layout.pages.len() + 1;
        layout.pages.push(Page {
            number,
            rows: rows.0.clone(),
            columns: columns.clone(),
            repeats_header: rows.1,
        });
    };
    match setup.page_order {
        PageOrder::DownThenOver => {
            for columns in &column_groups {
                for rows in &row_groups {
                    push(rows, columns);
                }
            }
        }
        PageOrder::OverThenDown => {
            for rows in &row_groups {
                for columns in &column_groups {
                    push(rows, columns);
                }
            }
        }
    }
    Ok(layout)
}

// {
//   責務: [pack_columns: 列を左から詰めて列グループにする。1列でも幅を超える列は単独で1グループにして記録する]
// }
fn pack_columns(
    widths: &[i64],
    basis_points: u32,
    printable_width: i64,
    layout: &mut PageLayout,
) -> Vec<Range<usize>> {
    let mut groups = Vec::new();
    let mut start = 0;
    let mut used = 0_i64;
    for (index, width) in widths.iter().enumerate() {
        let width = scaled(*width, basis_points);
        if width > printable_width {
            layout.oversized_columns.push(index);
        }
        if index > start && used + width > printable_width {
            groups.push(start..index);
            start = index;
            used = 0;
        }
        used += width;
    }
    groups.push(start..widths.len());
    groups
}

// {
//   責務: [pack_rows: 行を上から詰めて行グループにする。2ページ目以降は見出し行を先頭に足し、その高さを差し引く]
//   戻り値: [Vec<(Range<usize>, bool)>: 各ページの本文の行範囲と、見出し行を繰り返すか]
// }
fn pack_rows(
    heights: &[i64],
    header_rows: usize,
    repeat_header: bool,
    basis_points: u32,
    printable_height: i64,
    layout: &mut PageLayout,
) -> Vec<(Range<usize>, bool)> {
    let scaled_heights: Vec<i64> = heights
        .iter()
        .map(|height| scaled(*height, basis_points))
        .collect();
    let header_height: i64 = scaled_heights[..header_rows].iter().sum();
    let repeat = repeat_header && header_rows > 0 && header_rows < heights.len();
    let repeat = if repeat && header_height >= printable_height {
        layout.header_repeat_skipped = true;
        false
    } else {
        repeat
    };

    let mut groups = Vec::new();
    let mut start = 0;
    let mut used = 0_i64;
    // 2ページ目以降に置く行は、繰り返す見出し行の分だけ使える高さが減る。
    let capacity_from = |start: usize| {
        if repeat && start >= header_rows {
            printable_height - header_height
        } else {
            printable_height
        }
    };
    for (index, height) in scaled_heights.iter().enumerate() {
        let mut capacity = capacity_from(start);
        if index > start && used + height > capacity {
            groups.push((start..index, repeat && start >= header_rows));
            start = index;
            used = 0;
            capacity = capacity_from(start);
        }
        if *height > capacity {
            layout.oversized_rows.push(index);
        }
        used += height;
    }
    groups.push((start..heights.len(), repeat && start >= header_rows));
    groups
}
