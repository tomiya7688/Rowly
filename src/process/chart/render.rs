//! ```text
//! 責務: [chart::render: 解決済みChart（ResolvedChart）を、決定的なSVG文字列にする。6つの基本typeと、title・軸title・legend・series色・数値書式・欠損点の表示を持つ]
//! 処理: [1: 描画範囲（余白・legend・軸）を整数の座標で決める 2: 点を間引いて上限内にする 3: typeごとの図形（折れ線・面・棒・散布・円）を描く 4: stale / error / pendingの点を、値の点とは別の印で示す]
//! 補足: [CSVには触れず、入力の ResolvedChart を読むだけ。同じ入力なら常に同じ文字列になる。巨大なseriesはUIを固めないよう、描く点の数に上限を設けて間引く。画像だけで意味を伝えないよう、SVGにtitle・descとseriesの要約を持たせる]
//! ```
use std::fmt::Write as _;

use super::{ChartKind, ResolvedChart};

mod shapes;

/// 1 seriesで描く点の上限。超える場合は等間隔に間引く（先頭と末尾は残す）。
pub const MAX_POINTS_PER_SERIES: usize = 2_000;
/// 棒・円で描くcategoryの上限。超える分は描かず、件数を報告する。
pub const MAX_CATEGORIES: usize = 200;
/// 既定のseries色（色覚の違いに配慮した8色）。
const DEFAULT_COLORS: [&str; 8] = [
    "#0072b2", "#e69f00", "#009e73", "#cc79a7", "#56b4e9", "#d55e00", "#f0e442", "#000000",
];

/// 数値の書式。軸のtick labelと円グラフの割合に使う。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NumberFormat {
    /// 小数の桁数。
    pub decimals: u8,
    /// 3桁区切りを使うか。
    pub thousands_separator: bool,
}

/// 描画の見た目の設定。Chartの定義とは別のView設定で、CSVには保存されない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartStyle {
    /// 図の題名。
    pub title: Option<String>,
    /// 横軸の題名。
    pub x_axis_title: Option<String>,
    /// 縦軸の題名。
    pub y_axis_title: Option<String>,
    /// legendを表示するか。
    pub show_legend: bool,
    /// seriesの色（`#rrggbb`）。足りない分は既定の色を順に使う。
    pub series_colors: Vec<String>,
    /// 数値の書式。
    pub number_format: NumberFormat,
    /// 幅（px）。
    pub width: u32,
    /// 高さ（px）。
    pub height: u32,
}

impl Default for ChartStyle {
    fn default() -> Self {
        Self {
            title: None,
            x_axis_title: None,
            y_axis_title: None,
            show_legend: true,
            series_colors: Vec::new(),
            number_format: NumberFormat::default(),
            width: 640,
            height: 400,
        }
    }
}

/// 描画結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedChart {
    /// SVG文書。
    pub svg: String,
    /// 値として描いた点の数。
    pub plotted_points: usize,
    /// 上限のために間引いた・描かなかった点の数。
    pub omitted_points: usize,
}

/// 描画できない設定。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RenderError {
    /// 幅または高さが範囲外。
    #[error("chart size must be between 200 and 10000 pixels")]
    InvalidSize,
    /// 色が`#rrggbb`でない。
    #[error("series color `{0}` must look like #rrggbb")]
    InvalidColor(String),
}

/// 描画領域（px）。
#[derive(Debug, Clone, Copy)]
pub(super) struct Plot {
    pub(super) left: f64,
    pub(super) top: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

// {
//   責務: [render_svg: ResolvedChartを決定的なSVGにする]
//   処理: [設定を検証し、title・軸・legendの領域を除いた描画領域へ、typeごとの図形を描く。点は上限内へ間引き、stale・error・pendingの点は別の印で示す。freshでない点を含むときは、その旨と件数を図の上部に書く]
//   引数: [chart: 解決済みChart, style: 見た目の設定]
//   戻り値: [Result<RenderedChart, RenderError>: SVGと描いた点・省いた点の数。設定が不正ならerror]
// }
pub fn render_svg(chart: &ResolvedChart, style: &ChartStyle) -> Result<RenderedChart, RenderError> {
    if !(200..=10_000).contains(&style.width) || !(200..=10_000).contains(&style.height) {
        return Err(RenderError::InvalidSize);
    }
    if let Some(bad) = style
        .series_colors
        .iter()
        .find(|color| !is_hex_color(color))
    {
        return Err(RenderError::InvalidColor(bad.clone()));
    }
    let width = f64::from(style.width);
    let height = f64::from(style.height);
    let legend_width = if style.show_legend { 150.0 } else { 0.0 };
    let top = if style.title.is_some() { 48.0 } else { 28.0 };
    let bottom = if style.x_axis_title.is_some() {
        56.0
    } else {
        40.0
    };
    let left = if style.y_axis_title.is_some() {
        72.0
    } else {
        56.0
    };
    let plot = Plot {
        left,
        top,
        width: (width - left - legend_width - 16.0).max(40.0),
        height: (height - top - bottom).max(40.0),
    };
    let colors = SeriesColors(style);

    let mut body = String::new();
    let drawn = if chart.kind == ChartKind::Pie {
        shapes::draw_pie(&mut body, chart, style, plot, &colors)
    } else {
        shapes::draw_cartesian(&mut body, chart, style, plot, &colors)
    };

    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" role=\"img\" aria-label=\"{label}\">",
        w = style.width,
        h = style.height,
        label = escape(style.title.as_deref().unwrap_or("chart")),
    );
    let _ = write!(
        svg,
        "<title>{}</title><desc>{}</desc>",
        escape(style.title.as_deref().unwrap_or("chart")),
        escape(&summary(chart, style))
    );
    let _ = write!(
        svg,
        "<rect width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>",
        style.width, style.height
    );
    if let Some(title) = &style.title {
        let _ = write!(
            svg,
            "<text x=\"{:.2}\" y=\"24\" text-anchor=\"middle\" font-size=\"16\" font-weight=\"bold\">{}</text>",
            width / 2.0,
            escape(title)
        );
    }
    if let Some(note) = freshness_note(chart) {
        let _ = write!(
            svg,
            "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\" font-size=\"12\" fill=\"#b45309\" data-role=\"freshness-warning\">{}</text>",
            width - 8.0,
            if style.title.is_some() { 42.0 } else { 16.0 },
            escape(&note)
        );
    }
    svg.push_str(&body);
    if let Some(title) = &style.x_axis_title {
        let _ = write!(
            svg,
            "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"12\">{}</text>",
            plot.left + plot.width / 2.0,
            height - 8.0,
            escape(title)
        );
    }
    if let Some(title) = &style.y_axis_title {
        let _ = write!(
            svg,
            "<text transform=\"translate(14 {:.2}) rotate(-90)\" text-anchor=\"middle\" font-size=\"12\">{}</text>",
            plot.top + plot.height / 2.0,
            escape(title)
        );
    }
    if style.show_legend {
        shapes::draw_legend(&mut svg, chart, plot, width, &colors);
    }
    svg.push_str("</svg>");
    Ok(RenderedChart {
        svg,
        plotted_points: drawn.plotted,
        omitted_points: drawn.omitted,
    })
}

/// 図形の描画結果の件数。
pub(super) struct Drawn {
    pub(super) plotted: usize,
    pub(super) omitted: usize,
}

/// series色の選択。
pub(super) struct SeriesColors<'a>(&'a ChartStyle);

impl SeriesColors<'_> {
    // {
    //   責務: [get: index番目のseries / categoryの色。指定が足りなければ既定の色を順に使う]
    // }
    pub(super) fn get(&self, index: usize) -> &str {
        self.0
            .series_colors
            .get(index)
            .map(String::as_str)
            .unwrap_or(DEFAULT_COLORS[index % DEFAULT_COLORS.len()])
    }
}

fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|ch| ch.is_ascii_hexdigit())
}

// {
//   責務: [escape: XMLのtext / 属性値に入れる文字列をescapeする]
// }
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

// {
//   責務: [format_number: 数値書式に従って文字列にする。同じ値は常に同じ文字列（小数は丸めてから整形）]
// }
pub(super) fn format_number(value: f64, format: NumberFormat) -> String {
    let text = format!("{:.*}", usize::from(format.decimals), value);
    if !format.thousands_separator {
        return text;
    }
    let (sign, rest) = text
        .strip_prefix('-')
        .map_or(("", text.as_str()), |r| ("-", r));
    let (integer, fraction) = rest.split_once('.').map_or((rest, ""), |(i, f)| (i, f));
    let mut grouped = String::new();
    for (index, ch) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    if fraction.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{fraction}")
    }
}

// {
//   責務: [freshness_note: freshでない点があるとき、図の上部に出す警告文を作る]
// }
fn freshness_note(chart: &ResolvedChart) -> Option<String> {
    chart.stale_indicator().then(|| {
        let d = chart.diagnostics;
        format!(
            "古い・誤りの値を含む: stale {} / error {} / pending {} / category {}",
            d.stale, d.error, d.pending, d.category_not_fresh
        )
    })
}

// {
//   責務: [summary: 図を見なくても内容が分かる、seriesごとの要約文を作る（アクセシビリティ）]
// }
fn summary(chart: &ResolvedChart, style: &ChartStyle) -> String {
    let mut parts = Vec::new();
    for series in &chart.series {
        let numbers: Vec<f64> = series
            .points
            .iter()
            .filter_map(|point| match point.value {
                super::PointValue::Number(number) => Some(number),
                super::PointValue::Missing(_) => None,
            })
            .collect();
        let text = match (
            numbers.iter().copied().reduce(f64::min),
            numbers.iter().copied().reduce(f64::max),
        ) {
            (Some(low), Some(high)) => format!(
                "{}: {}点, 最小 {}, 最大 {}",
                series.name,
                numbers.len(),
                format_number(low, style.number_format),
                format_number(high, style.number_format)
            ),
            _ => format!("{}: 描ける点なし", series.name),
        };
        parts.push(text);
    }
    parts.join(" / ")
}
