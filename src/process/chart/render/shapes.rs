//! ```text
//! 責務: [chart::render::shapes: typeごとの図形（折れ線・面・棒・散布・円）と、軸・legend・欠損点の印をSVGの断片として書く]
//! 処理: [1: 点を上限内へ決定的に間引く 2: 値の範囲を「きりのよい」目盛りへ丸める 3: 値の点と、stale / error / pendingの点を別の図形で描く]
//! 補足: [座標はすべて小数2桁へ丸めて書き、同じ入力なら同じ文字列になる。色や書式の判断はrender側の設定に従う]
//! ```
use std::fmt::Write as _;

use super::{
    ChartStyle, Drawn, MAX_CATEGORIES, MAX_POINTS_PER_SERIES, Plot, SeriesColors, escape,
    format_number,
};
use crate::process::aggregation::CellFreshness;
use crate::process::chart::{
    ChartKind, ChartPoint, MissingReason, PointValue, ResolvedChart, ResolvedSeries,
};

// {
//   責務: [decimate: len個の点を、先頭と末尾を残して等間隔にmax個へ減らした位置を返す。max以下なら全部]
// }
fn decimate(len: usize, max: usize) -> Vec<usize> {
    if len <= max {
        return (0..len).collect();
    }
    (0..max).map(|step| step * (len - 1) / (max - 1)).collect()
}

// {
//   責務: [nice_scale: 値の範囲を、きりのよい刻み（1・2・5の倍数）の目盛りへ広げる]
//   戻り値: [(f64, f64, f64): 目盛りの下端・上端・刻み]
// }
fn nice_scale(min: f64, max: f64) -> (f64, f64, f64) {
    // 範囲が有限でない（極端な値でoverflowした）ときは、目盛りを作れないので既定の範囲にする。
    if !min.is_finite() || !max.is_finite() || !(max - min).is_finite() {
        return (0.0, 1.0, 0.25);
    }
    let (min, max) = if (max - min).abs() < f64::EPSILON {
        (min - 1.0, max + 1.0)
    } else {
        (min, max)
    };
    let raw = (max - min) / 4.0;
    let mut magnitude = 1.0_f64;
    while raw / magnitude >= 10.0 {
        magnitude *= 10.0;
    }
    while raw / magnitude < 1.0 {
        magnitude /= 10.0;
    }
    let normalized = raw / magnitude;
    let step = magnitude
        * if normalized <= 1.0 {
            1.0
        } else if normalized <= 2.0 {
            2.0
        } else if normalized <= 5.0 {
            5.0
        } else {
            10.0
        };
    (
        (min / step).floor() * step,
        (max / step).ceil() * step,
        step,
    )
}

/// 値の列のうち描けるもの。
fn numbers(series: &ResolvedSeries, indices: &[usize]) -> Vec<f64> {
    indices
        .iter()
        .filter_map(|&index| match series.points[index].value {
            PointValue::Number(number) => Some(number),
            PointValue::Missing(_) => None,
        })
        .collect()
}

// {
//   責務: [map_value: 値を描画領域の座標へ写す。範囲を大きく外れる値・有限でない比は、描画領域のすぐ外へ丸めて、座標にNaNや無限大を書かない]
// }
fn map_value(value: f64, low: f64, high: f64, start: f64, length: f64) -> f64 {
    let ratio = (value - low) / (high - low);
    let ratio = if ratio.is_nan() {
        0.0
    } else {
        ratio.clamp(-0.05, 1.05)
    };
    start + ratio * length
}

// {
//   責務: [draw_cartesian: 折れ線・面・縦棒・横棒・散布図を描く]
//   戻り値: [Drawn: 値として描いた点と、省いた点の数]
// }
pub(super) fn draw_cartesian(
    out: &mut String,
    chart: &ResolvedChart,
    style: &ChartStyle,
    plot: Plot,
    colors: &SeriesColors<'_>,
) -> Drawn {
    let kind = chart.kind;
    let bars = matches!(kind, ChartKind::Column | ChartKind::Bar);
    // 棒はcategoryの上限まで、それ以外は点を間引く。
    let indices: Vec<Vec<usize>> = chart
        .series
        .iter()
        .map(|series| {
            let limit = if bars {
                MAX_CATEGORIES
            } else {
                MAX_POINTS_PER_SERIES
            };
            if bars {
                (0..series.points.len().min(limit)).collect()
            } else {
                decimate(series.points.len(), limit)
            }
        })
        .collect();
    let omitted: usize = chart
        .series
        .iter()
        .zip(&indices)
        .map(|(series, kept)| series.points.len() - kept.len())
        .sum();
    // 軸の範囲は間引く前の全データから決める（間引きで極値が消えても、軸は実際の範囲を示す）。棒は描くcategoryだけ。
    let mut values: Vec<f64> = chart
        .series
        .iter()
        .zip(&indices)
        .flat_map(|(series, kept)| {
            let all: Vec<usize> = (0..series.points.len()).collect();
            numbers(series, if bars { kept } else { &all })
        })
        .collect();
    if matches!(kind, ChartKind::Area | ChartKind::Column | ChartKind::Bar) {
        values.push(0.0);
    }
    let (low, high, step) = match (
        values.iter().copied().reduce(f64::min),
        values.iter().copied().reduce(f64::max),
    ) {
        (Some(min), Some(max)) => nice_scale(min, max),
        _ => (0.0, 1.0, 0.25),
    };

    let horizontal = kind == ChartKind::Bar;
    // 値軸の目盛りとgrid。
    let ticks = (((high - low) / step).round() as usize).min(100);
    for tick in 0..=ticks {
        let value = low + step * tick as f64;
        if horizontal {
            let x = map_value(value, low, high, plot.left, plot.width);
            let _ = write!(
                out,
                "<line x1=\"{x:.2}\" y1=\"{:.2}\" x2=\"{x:.2}\" y2=\"{:.2}\" stroke=\"#e5e7eb\"/><text x=\"{x:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"11\">{}</text>",
                plot.top,
                plot.top + plot.height,
                plot.top + plot.height + 14.0,
                escape(&format_number(value, style.number_format))
            );
        } else {
            let y = map_value(value, low, high, plot.top + plot.height, -plot.height);
            let _ = write!(
                out,
                "<line x1=\"{:.2}\" y1=\"{y:.2}\" x2=\"{:.2}\" y2=\"{y:.2}\" stroke=\"#e5e7eb\"/><text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\" font-size=\"11\">{}</text>",
                plot.left,
                plot.left + plot.width,
                plot.left - 6.0,
                y + 4.0,
                escape(&format_number(value, style.number_format))
            );
        }
    }
    let _ = write!(
        out,
        "<rect x=\"{:.2}\" y=\"{:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"none\" stroke=\"#9ca3af\"/>",
        plot.left, plot.top, plot.width, plot.height
    );

    // 散布図のx範囲。
    let x_range = (kind == ChartKind::Scatter).then(|| {
        let xs: Vec<f64> = chart
            .series
            .iter()
            .flat_map(|series| series.points.iter().filter_map(|point| point.x))
            .collect();
        match (
            xs.iter().copied().reduce(f64::min),
            xs.iter().copied().reduce(f64::max),
        ) {
            (Some(min), Some(max)) => {
                let (lo, hi, _) = nice_scale(min, max);
                (lo, hi)
            }
            _ => (0.0, 1.0),
        }
    });
    let category_count = if bars {
        chart.categories.len().min(MAX_CATEGORIES)
    } else {
        chart.categories.len()
    };
    // categoryの帯の中心(px)。横棒のときは縦方向、それ以外は横方向。
    let band_center = |index: usize| -> f64 {
        let (start, length) = if horizontal {
            (plot.top, plot.height)
        } else {
            (plot.left, plot.width)
        };
        start + (index as f64 + 0.5) / category_count.max(1) as f64 * length
    };
    // 点の位置(px)。散布図は数値のx、それ以外はcategoryの帯の中心。
    let category_position = |index: usize, point: &ChartPoint| -> f64 {
        match x_range {
            Some((lo, hi)) => map_value(point.x.unwrap_or(lo), lo, hi, plot.left, plot.width),
            None => band_center(index),
        }
    };
    // categoryのlabel。多いときは間引く。
    if kind != ChartKind::Scatter {
        let stride = category_count.div_ceil(10).max(1);
        for index in (0..category_count).step_by(stride) {
            let position = band_center(index);
            let label = escape(&chart.categories[index]);
            if horizontal {
                let _ = write!(
                    out,
                    "<text x=\"{:.2}\" y=\"{:.2}\" text-anchor=\"end\" font-size=\"11\">{label}</text>",
                    plot.left - 6.0,
                    position + 4.0
                );
            } else {
                let _ = write!(
                    out,
                    "<text x=\"{position:.2}\" y=\"{:.2}\" text-anchor=\"middle\" font-size=\"11\">{label}</text>",
                    plot.top + plot.height + 14.0
                );
            }
        }
    }

    let value_px = |value: f64| {
        if horizontal {
            map_value(value, low, high, plot.left, plot.width)
        } else {
            map_value(value, low, high, plot.top + plot.height, -plot.height)
        }
    };
    let baseline = value_px(0.0_f64.clamp(low, high));
    let mut plotted = 0;
    let series_count = chart.series.len().max(1);
    for (series_index, (series, kept)) in chart.series.iter().zip(&indices).enumerate() {
        let color = colors.get(series_index);
        // 連続して描ける点の並び(run)。欠損点で折れ線を切る。
        let mut run: Vec<(f64, f64)> = Vec::new();
        let flush = |run: &mut Vec<(f64, f64)>, out: &mut String| {
            if run.is_empty() {
                return;
            }
            let points: String = run
                .iter()
                .map(|(x, y)| format!("{x:.2},{y:.2}"))
                .collect::<Vec<_>>()
                .join(" ");
            if kind == ChartKind::Area {
                let (first, last) = (run[0].0, run[run.len() - 1].0);
                let _ = write!(
                    out,
                    "<polygon points=\"{first:.2},{baseline:.2} {points} {last:.2},{baseline:.2}\" fill=\"{color}\" fill-opacity=\"0.3\"/>"
                );
            }
            if matches!(kind, ChartKind::Line | ChartKind::Area) && run.len() > 1 {
                let _ = write!(
                    out,
                    "<polyline points=\"{points}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"/>"
                );
            }
            run.clear();
        };
        // 欠損点の累計。間引きで欠損点が落ちても、その区間をまたいで折れ線をつながないために使う。
        let missing_prefix: Vec<usize> = std::iter::once(0)
            .chain(series.points.iter().scan(0, |count, point| {
                if matches!(point.value, PointValue::Missing(_)) {
                    *count += 1;
                }
                Some(*count)
            }))
            .collect();
        let mut previous: Option<usize> = None;
        for &index in kept {
            let point = &series.points[index];
            let position = category_position(index, point);
            match &point.value {
                PointValue::Number(_) if kind == ChartKind::Scatter && point.x.is_none() => {}
                PointValue::Number(value) => {
                    if let Some(prev) = previous
                        && missing_prefix[index] > missing_prefix[prev + 1]
                    {
                        flush(&mut run, out);
                    }
                    previous = Some(index);
                    plotted += 1;
                    let y = value_px(*value);
                    match kind {
                        ChartKind::Line | ChartKind::Area => {
                            run.push((position, y));
                            let _ = write!(
                                out,
                                "<circle cx=\"{position:.2}\" cy=\"{y:.2}\" r=\"3\" fill=\"{color}\"/>"
                            );
                        }
                        ChartKind::Scatter => {
                            let _ = write!(
                                out,
                                "<circle cx=\"{position:.2}\" cy=\"{y:.2}\" r=\"4\" fill=\"{color}\"/>"
                            );
                        }
                        ChartKind::Column | ChartKind::Bar => {
                            let band = (if horizontal { plot.height } else { plot.width })
                                / category_count.max(1) as f64;
                            let bar = band * 0.8 / series_count as f64;
                            let start = position - band * 0.4 + bar * series_index as f64;
                            let (low_px, high_px) = (baseline.min(y), baseline.max(y));
                            if horizontal {
                                let _ = write!(
                                    out,
                                    "<rect x=\"{low_px:.2}\" y=\"{start:.2}\" width=\"{:.2}\" height=\"{bar:.2}\" fill=\"{color}\"/>",
                                    high_px - low_px
                                );
                            } else {
                                let _ = write!(
                                    out,
                                    "<rect x=\"{start:.2}\" y=\"{low_px:.2}\" width=\"{bar:.2}\" height=\"{:.2}\" fill=\"{color}\"/>",
                                    high_px - low_px
                                );
                            }
                        }
                        ChartKind::Pie => {}
                    }
                }
                PointValue::Missing(reason) => {
                    flush(&mut run, out);
                    previous = None;
                    if kind == ChartKind::Scatter && point.x.is_none() {
                        continue;
                    }
                    let (x, y) = if horizontal {
                        (baseline, position)
                    } else {
                        (position, baseline)
                    };
                    draw_missing_marker(out, reason, x, y);
                }
            }
        }
        flush(&mut run, out);
    }
    Drawn { plotted, omitted }
}

// {
//   責務: [draw_missing_marker: 描かない点の位置に、理由が分かる印を描く。stale=橙の菱形、error=赤い×、pending=灰色の白抜き丸。空・数値でない値は印を出さない]
// }
fn draw_missing_marker(out: &mut String, reason: &MissingReason, x: f64, y: f64) {
    match reason {
        MissingReason::NotFresh(CellFreshness::Stale) => {
            let _ = write!(
                out,
                "<polygon data-reason=\"stale\" points=\"{x:.2},{:.2} {:.2},{y:.2} {x:.2},{:.2} {:.2},{y:.2}\" fill=\"none\" stroke=\"#d97706\" stroke-width=\"2\"/>",
                y - 5.0,
                x + 5.0,
                y + 5.0,
                x - 5.0
            );
        }
        MissingReason::NotFresh(CellFreshness::Error) => {
            let _ = write!(
                out,
                "<path data-reason=\"error\" d=\"M{:.2} {:.2}L{:.2} {:.2}M{:.2} {:.2}L{:.2} {:.2}\" stroke=\"#dc2626\" stroke-width=\"2\"/>",
                x - 4.0,
                y - 4.0,
                x + 4.0,
                y + 4.0,
                x - 4.0,
                y + 4.0,
                x + 4.0,
                y - 4.0
            );
        }
        MissingReason::NotFresh(CellFreshness::Pending) | MissingReason::CategoryNotFresh => {
            let _ = write!(
                out,
                "<circle data-reason=\"pending\" cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"4\" fill=\"none\" stroke=\"#6b7280\" stroke-width=\"2\"/>"
            );
        }
        MissingReason::NotFresh(CellFreshness::Fresh)
        | MissingReason::Empty
        | MissingReason::NotNumeric(_)
        | MissingReason::Negative => {}
    }
}

// {
//   責務: [draw_pie: 円グラフを描く。categoryごとの割合を扇形にし、割合をlegend側で示せるよう書式で整形する]
// }
pub(super) fn draw_pie(
    out: &mut String,
    chart: &ResolvedChart,
    _style: &ChartStyle,
    plot: Plot,
    colors: &SeriesColors<'_>,
) -> Drawn {
    let series = &chart.series[0];
    let limit = series.points.len().min(MAX_CATEGORIES);
    let slices: Vec<(usize, f64)> = (0..limit)
        .filter_map(|index| match series.points[index].value {
            PointValue::Number(value) if value > 0.0 => Some((index, value)),
            _ => None,
        })
        .collect();
    let total: f64 = slices.iter().map(|(_, value)| value).sum();
    let (cx, cy) = (plot.left + plot.width / 2.0, plot.top + plot.height / 2.0);
    let radius = (plot.width.min(plot.height) / 2.0 - 4.0).max(10.0);
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for &(index, value) in &slices {
        let color = colors.get(index);
        let sweep = value / total * std::f64::consts::TAU;
        if slices.len() == 1 {
            let _ = write!(
                out,
                "<circle cx=\"{cx:.2}\" cy=\"{cy:.2}\" r=\"{radius:.2}\" fill=\"{color}\"/>"
            );
        } else {
            let end = angle + sweep;
            let large = i32::from(sweep > std::f64::consts::PI);
            let _ = write!(
                out,
                "<path d=\"M{cx:.2} {cy:.2}L{:.2} {:.2}A{radius:.2} {radius:.2} 0 {large} 1 {:.2} {:.2}Z\" fill=\"{color}\" stroke=\"#ffffff\"/>",
                cx + radius * angle.cos(),
                cy + radius * angle.sin(),
                cx + radius * end.cos(),
                cy + radius * end.sin()
            );
        }
        angle += sweep;
    }
    // 描かなかった点(欠損)は円の下に印で示す。
    let mut marker_x = plot.left + 8.0;
    for point in &series.points[..limit] {
        if let PointValue::Missing(reason) = &point.value {
            draw_missing_marker(out, reason, marker_x, plot.top + plot.height - 6.0);
            marker_x += 14.0;
        }
    }
    Drawn {
        plotted: slices.len(),
        omitted: series.points.len() - limit,
    }
}

// {
//   責務: [draw_legend: 右側にlegendを描く。円グラフはcategory、それ以外はseriesの名前と色を並べる]
// }
pub(super) fn draw_legend(
    svg: &mut String,
    chart: &ResolvedChart,
    plot: Plot,
    width: f64,
    colors: &SeriesColors<'_>,
) {
    let names: Vec<&str> = if chart.kind == ChartKind::Pie {
        chart
            .categories
            .iter()
            .take(MAX_CATEGORIES.min(12))
            .map(String::as_str)
            .collect()
    } else {
        chart
            .series
            .iter()
            .map(|series| series.name.as_str())
            .collect()
    };
    let x = (width - 144.0).max(plot.left + plot.width + 8.0);
    for (index, name) in names.iter().enumerate() {
        let y = plot.top + 6.0 + index as f64 * 18.0;
        let _ = write!(
            svg,
            "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"12\" height=\"12\" fill=\"{}\"/><text x=\"{:.2}\" y=\"{:.2}\" font-size=\"12\">{}</text>",
            colors.get(index),
            x + 18.0,
            y + 10.0,
            escape(name)
        );
    }
}
