//! ```text
//! 責務: [chart::source: Chartのデータ源（raw range・current Viewのrow・Group By結果・Pivot結果）を共通の表形式（SourceTable）にそろえる]
//! 処理: [1: 各sourceをid・見出し・row（cellのfreshnessと元rowへの参照つき）の表にする 2: Group By / Pivotの結果は、集計のerrorをError cellにし、cellごとに元row集合を持たせる]
//! 補足: [表示screenの座標をデータidentityにしない。元データは複製せず、CSVには触れない]
//! ```
use crate::process::aggregation::{CellFreshness, GroupByResult, InputRow, SourceRowRef};
use crate::process::calculation::CalculationValue;
use crate::process::pivot::PivotResult;

/// Chartが読む1つのcell。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartCell {
    /// 文字列。数値としての解釈はChart側で行う。
    pub text: String,
    /// freshness。fresh以外は通常のseriesとして描かない。
    pub freshness: CellFreshness,
    /// このcellが表す元row。aggregateのcellならgroup / bucketに入った全rowで、drill downに使う。
    pub provenance: Vec<SourceRowRef>,
}

impl ChartCell {
    fn new(text: impl Into<String>, provenance: Vec<SourceRowRef>) -> Self {
        Self {
            text: text.into(),
            freshness: CellFreshness::Fresh,
            provenance,
        }
    }

    fn error(provenance: Vec<SourceRowRef>) -> Self {
        Self {
            text: String::new(),
            freshness: CellFreshness::Error,
            provenance,
        }
    }
}

/// Chartが読む1つのrow。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChartRow {
    /// 見出し順のcell。
    pub cells: Vec<ChartCell>,
}

/// Chartのsourceにできる表。idはsourceのstableな名前で、列は見出しで指す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTable {
    /// source id。
    pub id: String,
    /// 見出し。
    pub headers: Vec<String>,
    /// rows。
    pub rows: Vec<ChartRow>,
}

impl SourceTable {
    // {
    //   責務: [from_input_rows: raw range・current Viewのrowを表にする]
    //   引数: [id: source id, headers: 見出し, rows: 呼び出し側が選んだ入力row（全row・View filter後・選択範囲など）]
    //   戻り値: [SourceTable: 各cellの元row参照は入力rowのsource_row]
    // }
    pub fn from_input_rows(id: &str, headers: Vec<String>, rows: &[InputRow]) -> Self {
        Self {
            id: id.to_owned(),
            headers,
            rows: rows
                .iter()
                .map(|row| ChartRow {
                    cells: row
                        .cells
                        .iter()
                        .map(|cell| ChartCell {
                            text: cell.text.clone(),
                            freshness: cell.freshness,
                            provenance: vec![row.source_row.clone()],
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    // {
    //   責務: [from_group_by: Group By結果を表にする。1 groupが1 row]
    //   処理: [key列はFresh、集計値はCalculationの値の文字列。集計がerrorならError cell。provenanceはgroupの元row全部]
    //   引数: [id: source id, key_headers: key列の見出し, aggregate_headers: 集計列の見出し, result: Group By結果]
    //   戻り値: [SourceTable: 見出しはkey列→集計列の順]
    // }
    pub fn from_group_by(
        id: &str,
        key_headers: &[String],
        aggregate_headers: &[String],
        result: &GroupByResult,
    ) -> Self {
        let headers = key_headers
            .iter()
            .chain(aggregate_headers)
            .cloned()
            .collect();
        let rows = result
            .groups
            .iter()
            .map(|group| {
                let rows = &group.source_rows;
                ChartRow {
                    cells: group
                        .keys
                        .iter()
                        .map(|key| ChartCell::new(key, rows.clone()))
                        .chain(group.values.iter().map(|value| match value {
                            Ok(value) => ChartCell::new(value_text(value), rows.clone()),
                            Err(_) => ChartCell::error(rows.clone()),
                        }))
                        .collect(),
                }
            })
            .collect();
        Self {
            id: id.to_owned(),
            headers,
            rows,
        }
    }

    // {
    //   責務: [from_pivot: Pivot結果の1つのValueを、行key列＋column key毎の列の幅広い表にする]
    //   処理: [column keyを" / "でつないだ文字列を列の見出しにする。該当rowが無いbucketは元row参照が空のFresh空cell。bucketの集計がerrorならError cell。row key列の元rowは、そのrowの全bucketの和]
    //   引数: [id: source id, row_key_headers: row key列の見出し, result: Pivot結果, value_index: 使うValueの位置]
    //   戻り値: [SourceTable: 見出しはrow key列→column key毎の列の順]
    // }
    pub fn from_pivot(
        id: &str,
        row_key_headers: &[String],
        result: &PivotResult,
        value_index: usize,
    ) -> Self {
        let headers = row_key_headers
            .iter()
            .cloned()
            .chain(result.column_keys.iter().map(|key| key.join(" / ")))
            .collect();
        let rows = result
            .row_keys
            .iter()
            .zip(&result.cells)
            .map(|(row_key, cells)| {
                let row_sources: Vec<SourceRowRef> = cells
                    .iter()
                    .flatten()
                    .flat_map(|cell| cell.source_rows.iter().cloned())
                    .collect();
                ChartRow {
                    cells: row_key
                        .iter()
                        .map(|key| ChartCell::new(key, row_sources.clone()))
                        .chain(cells.iter().map(|cell| match cell {
                            None => ChartCell::new("", Vec::new()),
                            Some(cell) => match cell.values.get(value_index) {
                                Some(Ok(value)) => {
                                    ChartCell::new(value_text(value), cell.source_rows.clone())
                                }
                                _ => ChartCell::error(cell.source_rows.clone()),
                            },
                        }))
                        .collect(),
                }
            })
            .collect();
        Self {
            id: id.to_owned(),
            headers,
            rows,
        }
    }
}

// {
//   責務: [value_text: Calculationの値をChartが読む文字列にする]
// }
fn value_text(value: &CalculationValue) -> String {
    match value {
        CalculationValue::Text(text) => text.clone(),
        CalculationValue::Integer(number) => number.to_string(),
        CalculationValue::Decimal(number) => number.to_string(),
        CalculationValue::Boolean(flag) => if *flag { "TRUE" } else { "FALSE" }.to_owned(),
    }
}
