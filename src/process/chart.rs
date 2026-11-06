//! ```text
//! 責務: [chart: Chartの定義（種類・data source・category/x・series）と、現在のsourceへ再解決したPoint列の計算を提供する]
//! 処理: [1: 定義はsource idと列の見出しで持つ（表示座標や列indexを持たない） 2: 解決時に見出しをsourceの列へ引き直し、見つからない・重複するものを全部まとめて報告する 3: 各cellを数値のPointにし、freshでない・数値でない・空のcellは通常のseriesに描かず、理由つきの欠損にして診断へ数える 4: 各Pointに元row参照を持たせ、drill downできるようにする]
//! 補足: [描画・保存はこのmoduleの対象外。CSVには触れず、sourceを変えてもCSVは変わらない]
//! ```
use crate::process::aggregation::{CellFreshness, SourceRowRef};

mod render;
mod source;
pub use render::{
    ChartStyle, MAX_CATEGORIES, MAX_POINTS_PER_SERIES, NumberFormat, RenderError, RenderedChart,
    render_svg,
};
pub use source::{ChartCell, ChartRow, SourceTable};

/// 基本Chartの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChartKind {
    /// 折れ線。
    Line,
    /// 横棒。
    Bar,
    /// 縦棒。
    Column,
    /// 面。
    Area,
    /// 散布図。category列が数値のxになる。
    Scatter,
    /// 円。seriesは1つで、負の値は描かない。
    Pie,
}

/// 1つのseriesの定義。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesDefinition {
    /// 表示名。Noneなら列の見出しを使う。
    pub name: Option<String>,
    /// 値の列の見出し。
    pub header: String,
}

/// Chartの定義。View設定であり、CSVには保存されない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartDefinition {
    /// Chartの名前。
    pub name: String,
    /// 種類。
    pub kind: ChartKind,
    /// data sourceのid。
    pub source_id: String,
    /// category / xの列の見出し。Noneなら1始まりのrow番号をcategoryにする。
    pub category_header: Option<String>,
    /// series。
    pub series: Vec<SeriesDefinition>,
}

/// 点を描かない理由。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MissingReason {
    /// 値のcellが空。
    Empty,
    /// 値が数値として読めない。
    NotNumeric(String),
    /// 値のcellがfreshでない（stale・error・pending）。
    NotFresh(CellFreshness),
    /// category / xのcellがfreshでない。
    CategoryNotFresh,
    /// 円グラフで負の値。
    Negative,
}

/// 点の値。
#[derive(Debug, Clone, PartialEq)]
pub enum PointValue {
    /// 描ける数値。
    Number(f64),
    /// 描かない点とその理由。
    Missing(MissingReason),
}

/// 1つの点。
#[derive(Debug, Clone, PartialEq)]
pub struct ChartPoint {
    /// sourceのrow位置。
    pub row: usize,
    /// categoryの表示文字列。
    pub category: String,
    /// 散布図のx。散布図以外、または数値でなければNone。
    pub x: Option<f64>,
    /// 値。
    pub value: PointValue,
    /// この点の元row。aggregateの点ならgroup / bucketに入った全rowで、drill downに使う。
    pub provenance: Vec<SourceRowRef>,
}

/// 解決済みのseries。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSeries {
    /// 表示名。
    pub name: String,
    /// 値の列の見出し。
    pub header: String,
    /// sourceのrow順の点。
    pub points: Vec<ChartPoint>,
}

/// 描かなかった点の件数（series毎の点を数える。複数seriesでは同じ行が系列の数だけ数えられる）。黙って0や正常な値にしない。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChartDiagnostics {
    /// staleのため描かなかった点。
    pub stale: usize,
    /// errorのため描かなかった点。
    pub error: usize,
    /// pendingのため描かなかった点。
    pub pending: usize,
    /// categoryがfreshでないため描かなかった点。
    pub category_not_fresh: usize,
    /// 数値でないため描かなかった点。
    pub non_numeric: usize,
    /// 空のため描かなかった点。
    pub empty: usize,
    /// 円グラフの負の値のため描かなかった点。
    pub negative: usize,
}

/// 解決済みのChart。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedChart {
    /// 種類。
    pub kind: ChartKind,
    /// categoryの表示文字列。sourceのrow順。
    pub categories: Vec<String>,
    /// series。
    pub series: Vec<ResolvedSeries>,
    /// 描かなかった点の診断。
    pub diagnostics: ChartDiagnostics,
}

impl ResolvedChart {
    /// freshでない値が含まれ、古い・誤った値を正常なChartとして描いていない旨を示すべきか。
    pub fn stale_indicator(&self) -> bool {
        let d = &self.diagnostics;
        d.stale + d.error + d.pending + d.category_not_fresh > 0
    }
}

/// 現在のsourceに引き直せなかった定義の内容。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChartUnresolved {
    /// 定義のsource idが見つからない。
    pub missing_source: bool,
    /// 定義のsource idが複数ある。
    pub ambiguous_source: bool,
    /// sourceに存在しない見出し。
    pub missing_columns: Vec<String>,
    /// sourceに複数ある見出し。
    pub ambiguous_columns: Vec<String>,
}

/// Chartを解決できない理由。
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ChartError {
    /// sourceまたは列を引き直せない。定義は捨てず、sourceや列が戻れば再び解決できる。
    #[error("the chart's source or columns could not be resolved")]
    Unresolved(ChartUnresolved),
    /// 定義自体が不正。
    #[error("invalid chart definition: {0}")]
    InvalidDefinition(&'static str),
}

// {
//   責務: [resolve: 定義を現在のsourceへ引き直し、Point列を作る]
//   処理: [source idと見出しをすべて探し、無い・重複するものを全部まとめてUnresolvedにする。解決できれば各rowを数値のPointにし、freshでない・数値でない・空のcellは理由つきの欠損にして診断へ数える]
//   引数: [definition: Chartの定義, tables: 現在使えるsource]
//   戻り値: [Result<ResolvedChart, ChartError>: 解決結果。定義が不正、またはsourceを引き直せなければerror]
// }
pub fn resolve(
    definition: &ChartDefinition,
    tables: &[SourceTable],
) -> Result<ResolvedChart, ChartError> {
    if definition.series.is_empty() {
        return Err(ChartError::InvalidDefinition("a chart needs a series"));
    }
    match definition.kind {
        ChartKind::Pie if definition.series.len() != 1 => {
            return Err(ChartError::InvalidDefinition(
                "a pie chart needs exactly one series",
            ));
        }
        ChartKind::Scatter if definition.category_header.is_none() => {
            return Err(ChartError::InvalidDefinition(
                "a scatter chart needs an x column",
            ));
        }
        _ => {}
    }

    let mut unresolved = ChartUnresolved::default();
    let mut matches = tables
        .iter()
        .filter(|table| table.id == definition.source_id);
    let table = match (matches.next(), matches.next()) {
        (Some(table), None) => Some(table),
        (None, _) => {
            unresolved.missing_source = true;
            None
        }
        (Some(_), Some(_)) => {
            unresolved.ambiguous_source = true;
            None
        }
    };
    let Some(table) = table else {
        return Err(ChartError::Unresolved(unresolved));
    };

    let mut column = |header: &str| -> Option<usize> {
        let mut found = table
            .headers
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.as_str() == header);
        match (found.next(), found.next()) {
            (Some((index, _)), None) => Some(index),
            (None, _) => {
                push_unique(&mut unresolved.missing_columns, header);
                None
            }
            (Some(_), Some(_)) => {
                push_unique(&mut unresolved.ambiguous_columns, header);
                None
            }
        }
    };
    let category_column = definition.category_header.as_deref().map(&mut column);
    let series_columns: Vec<Option<usize>> = definition
        .series
        .iter()
        .map(|series| column(&series.header))
        .collect();
    if unresolved != ChartUnresolved::default() {
        return Err(ChartError::Unresolved(unresolved));
    }
    let category_column = category_column.flatten();

    let mut diagnostics = ChartDiagnostics::default();
    let mut categories = Vec::with_capacity(table.rows.len());
    let mut series: Vec<ResolvedSeries> = definition
        .series
        .iter()
        .map(|series| ResolvedSeries {
            name: series.name.clone().unwrap_or_else(|| series.header.clone()),
            header: series.header.clone(),
            points: Vec::with_capacity(table.rows.len()),
        })
        .collect();

    for (row_index, row) in table.rows.iter().enumerate() {
        let category_cell = category_column.and_then(|index| row.cells.get(index));
        let category = category_cell
            .map(|cell| cell.text.clone())
            .unwrap_or_else(|| (row_index + 1).to_string());
        let category_fresh =
            category_cell.is_none_or(|cell| cell.freshness == CellFreshness::Fresh);
        // 散布図のxは、実在してfreshなcategory cellが数値のときだけ。行番号で代用しない。
        let x = category_cell
            .filter(|cell| {
                definition.kind == ChartKind::Scatter && cell.freshness == CellFreshness::Fresh
            })
            .and_then(|cell| parse_number(&cell.text));
        categories.push(category.clone());
        for (output, column_index) in series.iter_mut().zip(&series_columns) {
            let cell = column_index.and_then(|index| row.cells.get(index));
            let value = point_value(
                definition.kind,
                cell,
                (
                    category_fresh,
                    category_cell.map(|cell| cell.text.as_str()),
                    x,
                ),
                &mut diagnostics,
            );
            // 点の来歴は値のcell自身のものだけ。値の無い点にcategoryなど別cellの来歴を貸さない。
            let provenance = cell.map(|cell| cell.provenance.clone()).unwrap_or_default();
            output.points.push(ChartPoint {
                row: row_index,
                category: category.clone(),
                x,
                value,
                provenance,
            });
        }
    }
    Ok(ResolvedChart {
        kind: definition.kind,
        categories,
        series,
        diagnostics,
    })
}

// {
//   責務: [point_value: 1つのcellを描ける数値か、理由つきの欠損にする。欠損は診断へ数える]
// }
fn point_value(
    kind: ChartKind,
    cell: Option<&ChartCell>,
    (category_fresh, category_text, x): (bool, Option<&str>, Option<f64>),
    diagnostics: &mut ChartDiagnostics,
) -> PointValue {
    let missing = PointValue::Missing;
    if !category_fresh {
        diagnostics.category_not_fresh += 1;
        return missing(MissingReason::CategoryNotFresh);
    }
    // 散布図は、xを決められない点を描けない（値のcellの有無より先に判定する）。
    if kind == ChartKind::Scatter && x.is_none() {
        return match category_text {
            Some(text) => {
                diagnostics.non_numeric += 1;
                missing(MissingReason::NotNumeric(text.to_owned()))
            }
            None => {
                diagnostics.empty += 1;
                missing(MissingReason::Empty)
            }
        };
    }
    let Some(cell) = cell else {
        diagnostics.empty += 1;
        return missing(MissingReason::Empty);
    };
    match cell.freshness {
        CellFreshness::Fresh => {}
        CellFreshness::Stale => diagnostics.stale += 1,
        CellFreshness::Error => diagnostics.error += 1,
        CellFreshness::Pending => diagnostics.pending += 1,
    }
    if cell.freshness != CellFreshness::Fresh {
        return missing(MissingReason::NotFresh(cell.freshness));
    }
    if cell.text.trim().is_empty() {
        diagnostics.empty += 1;
        return missing(MissingReason::Empty);
    }
    match parse_number(&cell.text) {
        None => {
            diagnostics.non_numeric += 1;
            missing(MissingReason::NotNumeric(cell.text.clone()))
        }
        Some(number) if kind == ChartKind::Pie && number < 0.0 => {
            diagnostics.negative += 1;
            missing(MissingReason::Negative)
        }
        Some(number) => PointValue::Number(number),
    }
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
}

fn push_unique(list: &mut Vec<String>, header: &str) {
    if !list.iter().any(|existing| existing == header) {
        list.push(header.to_owned());
    }
}
