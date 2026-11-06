//! ```text
//! 責務: [aggregation: 元CSVを変えずにGroup By集計のDerived Tableとrow provenanceを作る]
//! 処理: [1: 入力rowをgroup keyのraw値でまとめる 2: 集計はCalculation標準関数と同じ規則で評価する 3: group毎に元rowの参照とfreshness診断を保つ]
//! 補足: [入力rowの範囲（all canonical rows / View filter後 / selection）は呼び出し側が選ぶ。このmoduleは入力を読むだけで書き換えない]
//! ```
use std::collections::{BTreeSet, HashMap};

use crate::process::CsvDocument;
use crate::process::calculation::{
    AggregateFunction, CalculationFailure, CalculationValue, aggregate_texts,
};

/// 集計結果から元データへ戻るためのrow参照。元データは複製しない。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceRowRef {
    /// physical sourceを識別する名前。複数sourceのprovenanceを区別する。
    pub source: String,
    /// source内の0始まりのrow index。
    pub row: usize,
}

/// cell値のfreshness。derived Calculation valueを集計に使う場合に区別する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellFreshness {
    /// 最新の値。通常どおり集計する。
    Fresh,
    /// 古い値。集計から除き、診断に数える。
    Stale,
    /// 評価errorの値。集計から除き、診断に数える。
    Error,
    /// 評価待ちの値。集計から除き、診断に数える。
    Pending,
}

/// 集計入力の1cell。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputCell {
    /// raw文字列。
    pub text: String,
    /// freshness。
    pub freshness: CellFreshness,
}

impl InputCell {
    /// {
    ///   責務: [fresh: 最新のraw文字列cellを作る]
    ///   戻り値: [InputCell: Freshなcell]
    /// }
    pub fn fresh(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            freshness: CellFreshness::Fresh,
        }
    }
}

/// 集計入力の1row。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputRow {
    /// 元rowへの参照。
    pub source_row: SourceRowRef,
    /// 列順のcell。
    pub cells: Vec<InputCell>,
}

/// 1つの集計列の定義。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregateSpec {
    /// COUNT(*): groupのrow数。
    CountRows,
    /// 指定列へCalculationの集計関数を適用する。
    Column {
        /// 適用する関数。
        function: AggregateFunction,
        /// 対象列のindex。
        column: usize,
    },
    /// 指定列の空でないraw値の種類数。"1"と"1.0"は別の値として数える。
    DistinctCount {
        /// 対象列のindex。
        column: usize,
    },
}

/// Group Byの要求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupByRequest {
    /// group keyにする列index。空なら全rowを1つのgroupにする。
    pub key_columns: Vec<usize>,
    /// 集計列。
    pub aggregates: Vec<AggregateSpec>,
}

/// groupのfreshness診断。除外した件数を黙って0扱いしない。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FreshnessDiagnostics {
    /// 集計から除いたstale cellの数。
    pub stale: usize,
    /// 集計から除いたerror cellの数。
    pub error: usize,
    /// 集計から除いたpending cellの数。
    pub pending: usize,
}

/// 1 groupの結果。
#[derive(Debug, Clone, PartialEq)]
pub struct GroupRow {
    /// group keyのraw値。
    pub keys: Vec<String>,
    /// 集計列の値。errorはその集計だけに閉じる。
    pub values: Vec<Result<CalculationValue, CalculationFailure>>,
    /// このgroupに入った元row。drill downに使う。
    pub source_rows: Vec<SourceRowRef>,
    /// 集計から除いたnon-fresh cellの診断。
    pub diagnostics: FreshnessDiagnostics,
}

/// Group Byの結果。
#[derive(Debug, Clone, PartialEq)]
pub struct GroupByResult {
    /// 最初に現れた順のgroup。
    pub groups: Vec<GroupRow>,
    /// group keyがnon-freshだったため、どのgroupにも入れなかったrow。
    pub excluded_rows: Vec<SourceRowRef>,
}

/// 集計入力や要求の不備。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AggregationError {
    /// 要求が列範囲外を指している。
    #[error("column {column} is out of range for row {row}")]
    ColumnOutOfRange {
        /// 範囲外の列index。
        column: usize,
        /// 問題のあるinput rowの位置。
        row: usize,
    },
}

// {
//   責務: [rows_from_document: CsvDocumentの指定rowを読み取り専用で集計入力へ変換する]
//   処理: [cellはすべてFreshのraw文字列として読むだけで、documentは変更しない]
//   引数: [document: 読むdocument, source: source名, row_indices: 対象row（canonical全rowまたはView filter後・selectionを呼び出し側が選ぶ）]
//   戻り値: [Vec<InputRow>: 集計入力。範囲外のrowは含めない]
// }
pub fn rows_from_document(
    document: &CsvDocument,
    source: &str,
    row_indices: impl IntoIterator<Item = usize>,
) -> Vec<InputRow> {
    let width = document.column_count();
    row_indices
        .into_iter()
        .filter(|row| *row < document.row_count())
        .map(|row| InputRow {
            source_row: SourceRowRef {
                source: source.to_owned(),
                row,
            },
            cells: (0..width)
                .map(|column| InputCell::fresh(document.cell(row, column).unwrap_or_default()))
                .collect(),
        })
        .collect()
}

// {
//   責務: [group_by: raw key毎にrowをまとめ、集計とrow provenanceを返す]
//   処理: [表示formatが同じでもrawが違うkeyは別groupにする。non-freshなkey cellのrowはgroupへ入れずexcluded_rowsに残す。non-freshな値cellは集計から除き診断に数える。COUNT(*)はrow数]
//   引数: [rows: 集計入力, request: group keyと集計列]
//   戻り値: [Result<GroupByResult, AggregationError>: 結果。列範囲外はerror]
// }
pub fn group_by(
    rows: &[InputRow],
    request: &GroupByRequest,
) -> Result<GroupByResult, AggregationError> {
    validate(rows, request)?;
    let mut order: Vec<Vec<String>> = Vec::new();
    let mut members: HashMap<Vec<String>, Vec<usize>> = HashMap::new();
    let mut excluded_rows = Vec::new();
    for (position, row) in rows.iter().enumerate() {
        let keys: Vec<_> = request
            .key_columns
            .iter()
            .map(|column| &row.cells[*column])
            .collect();
        if keys
            .iter()
            .any(|cell| cell.freshness != CellFreshness::Fresh)
        {
            excluded_rows.push(row.source_row.clone());
            continue;
        }
        let key: Vec<String> = keys.iter().map(|cell| cell.text.clone()).collect();
        if !members.contains_key(&key) {
            order.push(key.clone());
        }
        members.entry(key).or_default().push(position);
    }
    let groups = order
        .into_iter()
        .map(|keys| {
            let positions = members.remove(&keys).unwrap_or_default();
            build_group(rows, request, keys, &positions)
        })
        .collect();
    Ok(GroupByResult {
        groups,
        excluded_rows,
    })
}

// {
//   責務: [validate: 全rowが要求の列indexを持つか確認する]
//   戻り値: [Result<(), AggregationError>: 範囲外の列があればerror]
// }
fn validate(rows: &[InputRow], request: &GroupByRequest) -> Result<(), AggregationError> {
    let columns: Vec<usize> = request
        .key_columns
        .iter()
        .copied()
        .chain(request.aggregates.iter().filter_map(|spec| match spec {
            AggregateSpec::CountRows => None,
            AggregateSpec::Column { column, .. } | AggregateSpec::DistinctCount { column } => {
                Some(*column)
            }
        }))
        .collect();
    for (row, input) in rows.iter().enumerate() {
        if let Some(column) = columns.iter().find(|column| **column >= input.cells.len()) {
            return Err(AggregationError::ColumnOutOfRange {
                column: *column,
                row,
            });
        }
    }
    Ok(())
}

// {
//   責務: [build_group: 1 groupの集計値・provenance・診断を作る]
//   引数: [rows: 全入力, request: 要求, keys: group key, positions: このgroupに入ったrowの位置]
//   戻り値: [GroupRow: 結果]
// }
fn build_group(
    rows: &[InputRow],
    request: &GroupByRequest,
    keys: Vec<String>,
    positions: &[usize],
) -> GroupRow {
    let mut diagnostics = FreshnessDiagnostics::default();
    let values = request
        .aggregates
        .iter()
        .map(|spec| match spec {
            AggregateSpec::CountRows => i64::try_from(positions.len())
                .map(CalculationValue::Integer)
                .map_err(|_| CalculationFailure::ArithmeticOverflow),
            AggregateSpec::Column { function, column } => {
                let texts = fresh_texts(rows, positions, *column, &mut diagnostics);
                aggregate_texts(*function, texts)
            }
            AggregateSpec::DistinctCount { column } => {
                let distinct: BTreeSet<String> =
                    fresh_texts(rows, positions, *column, &mut diagnostics)
                        .into_iter()
                        .filter(|text| !text.is_empty())
                        .collect();
                i64::try_from(distinct.len())
                    .map(CalculationValue::Integer)
                    .map_err(|_| CalculationFailure::ArithmeticOverflow)
            }
        })
        .collect();
    GroupRow {
        keys,
        values,
        source_rows: positions
            .iter()
            .map(|position| rows[*position].source_row.clone())
            .collect(),
        diagnostics,
    }
}

// {
//   責務: [fresh_texts: 指定列のFreshなcell文字列を集め、non-freshを診断へ数える]
//   引数: [rows: 全入力, positions: 対象row, column: 列, diagnostics: 診断（集計列ごとに加算する）]
//   戻り値: [Vec<String>: Freshな文字列]
// }
fn fresh_texts(
    rows: &[InputRow],
    positions: &[usize],
    column: usize,
    diagnostics: &mut FreshnessDiagnostics,
) -> Vec<String> {
    let mut texts = Vec::new();
    for position in positions {
        let cell = &rows[*position].cells[column];
        match cell.freshness {
            CellFreshness::Fresh => texts.push(cell.text.clone()),
            CellFreshness::Stale => diagnostics.stale += 1,
            CellFreshness::Error => diagnostics.error += 1,
            CellFreshness::Pending => diagnostics.pending += 1,
        }
    }
    texts
}
