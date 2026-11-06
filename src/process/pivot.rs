//! ```text
//! 責務: [pivot: 元CSVを変えずにRows / Columns / Values / FiltersからクロスタブのDerived Tableとcell provenanceを作る]
//! 処理: [1: 入力filterでrowを絞る 2: Group Byコアで(row key, column key)毎に集計する 3: 最初に現れた順のrow key・column keyの格子へ並べ、該当rowが無いbucketは空として残す]
//! 補足: [入力rowの範囲（all canonical rows / View filter後 / selection）は呼び出し側が選ぶ。表示Sortはこのmoduleの外のDerived View側で行い、元CSV順を変えない]
//! ```
use std::collections::HashMap;

use super::aggregation::{
    AggregateSpec, AggregationError, FreshnessDiagnostics, GroupByRequest, InputRow, SourceRowRef,
    group_by,
};
use super::calculation::{CalculationFailure, CalculationValue};

/// Pivotの入力filter。指定列のraw値が`allowed`のどれかと一致するrowだけを残す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PivotFilter {
    /// 判定する列のindex。
    pub column: usize,
    /// 残すraw値。表示formatが同じでもrawが違う値は別の値として扱う。
    pub allowed: Vec<String>,
}

/// Pivotの要求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PivotRequest {
    /// Rowsにする列index。空なら全体を1行にする。
    pub row_columns: Vec<usize>,
    /// Columnsにする列index。空なら全体を1列にする。
    pub column_columns: Vec<usize>,
    /// 各cellに入れる集計（Values）。
    pub values: Vec<AggregateSpec>,
    /// 集計前の入力filter。すべてに一致したrowだけを残す。
    pub filters: Vec<PivotFilter>,
}

/// 1つのPivot cell。集計値と元rowへの参照を持つ。
#[derive(Debug, Clone, PartialEq)]
pub struct PivotCell {
    /// `PivotRequest::values`と同じ順の集計値。errorはその集計だけに閉じる。
    pub values: Vec<Result<CalculationValue, CalculationFailure>>,
    /// このcellに入った元row。drill downに使い、元データは複製しない。
    pub source_rows: Vec<SourceRowRef>,
    /// 集計から除いたnon-fresh値cellの診断。
    pub diagnostics: FreshnessDiagnostics,
}

/// Pivotの結果。
#[derive(Debug, Clone, PartialEq)]
pub struct PivotResult {
    /// Rowsのkey。最初に現れた順。
    pub row_keys: Vec<Vec<String>>,
    /// Columnsのkey。最初に現れた順。
    pub column_keys: Vec<Vec<String>>,
    /// `cells[row][column]`。該当rowが無いbucketは`None`（0や空文字にしない）。
    pub cells: Vec<Vec<Option<PivotCell>>>,
    /// non-freshなfilter cellまたはkey cellのため、どのbucketにも入れなかったrow。filterの不一致で除いたrowは含めない。
    pub excluded_rows: Vec<SourceRowRef>,
}

// {
//   責務: [pivot: filterで絞ったrowを(row key, column key)毎に集計し、クロスタブの格子と各cellのprovenanceを返す]
//   処理: [filterはfilter指定順に判定し、non-freshなfilter cellのrowはexcluded_rowsに残し、不一致のrowは黙って除く。key・値の扱いはGroup Byコアと同じ（raw key、non-fresh値は集計から除き診断に数える）。row・columnの順序は入力での初出順で決まり、同じ入力なら常に同じ結果になる]
//   引数: [rows: 集計入力, request: Rows・Columns・Values・Filters]
//   戻り値: [Result<PivotResult, AggregationError>: 結果。列範囲外はerror]
// }
pub fn pivot(rows: &[InputRow], request: &PivotRequest) -> Result<PivotResult, AggregationError> {
    validate_filters(rows, &request.filters)?;
    let mut excluded_rows = Vec::new();
    let mut kept = Vec::new();
    for row in rows {
        match filter_decision(row, &request.filters) {
            FilterDecision::Keep => kept.push(row.clone()),
            FilterDecision::Drop => {}
            FilterDecision::NonFresh => excluded_rows.push(row.source_row.clone()),
        }
    }

    let row_width = request.row_columns.len();
    let key_columns: Vec<usize> = request
        .row_columns
        .iter()
        .chain(&request.column_columns)
        .copied()
        .collect();
    let grouped = group_by(
        &kept,
        &GroupByRequest {
            key_columns,
            aggregates: request.values.clone(),
        },
    )?;
    excluded_rows.extend(grouped.excluded_rows);

    let mut row_keys: Vec<Vec<String>> = Vec::new();
    let mut column_keys: Vec<Vec<String>> = Vec::new();
    let mut row_index: HashMap<Vec<String>, usize> = HashMap::new();
    let mut column_index: HashMap<Vec<String>, usize> = HashMap::new();
    let mut placed = Vec::with_capacity(grouped.groups.len());
    for group in grouped.groups {
        let (row_key, column_key) = group.keys.split_at(row_width);
        let row = position(&mut row_keys, &mut row_index, row_key);
        let column = position(&mut column_keys, &mut column_index, column_key);
        placed.push((
            row,
            column,
            PivotCell {
                values: group.values,
                source_rows: group.source_rows,
                diagnostics: group.diagnostics,
            },
        ));
    }
    let mut cells: Vec<Vec<Option<PivotCell>>> = (0..row_keys.len())
        .map(|_| (0..column_keys.len()).map(|_| None).collect())
        .collect();
    for (row, column, cell) in placed {
        cells[row][column] = Some(cell);
    }
    Ok(PivotResult {
        row_keys,
        column_keys,
        cells,
        excluded_rows,
    })
}

/// filter判定の結果。
enum FilterDecision {
    Keep,
    Drop,
    NonFresh,
}

// {
//   責務: [position: keyの格子上のindexを返す。初めて見るkeyは末尾へ加える]
//   引数: [keys: 初出順のkey, index: keyから位置への対応, key: 探すkey]
//   戻り値: [usize: keyの位置]
// }
fn position(
    keys: &mut Vec<Vec<String>>,
    index: &mut HashMap<Vec<String>, usize>,
    key: &[String],
) -> usize {
    if let Some(position) = index.get(key) {
        return *position;
    }
    keys.push(key.to_vec());
    index.insert(key.to_vec(), keys.len() - 1);
    keys.len() - 1
}

// {
//   責務: [filter_decision: 1 rowをfilter指定順に判定する]
//   戻り値: [FilterDecision: 全一致ならKeep、先に不一致ならDrop、先にnon-freshのfilter cellがあればNonFresh]
// }
fn filter_decision(row: &InputRow, filters: &[PivotFilter]) -> FilterDecision {
    for filter in filters {
        let cell = &row.cells[filter.column];
        if cell.freshness != super::aggregation::CellFreshness::Fresh {
            return FilterDecision::NonFresh;
        }
        if !filter.allowed.contains(&cell.text) {
            return FilterDecision::Drop;
        }
    }
    FilterDecision::Keep
}

// {
//   責務: [validate_filters: 全rowがfilterの列indexを持つか確認する]
//   戻り値: [Result<(), AggregationError>: 範囲外の列があればerror]
// }
fn validate_filters(rows: &[InputRow], filters: &[PivotFilter]) -> Result<(), AggregationError> {
    for (row, input) in rows.iter().enumerate() {
        if let Some(filter) = filters
            .iter()
            .find(|filter| filter.column >= input.cells.len())
        {
            return Err(AggregationError::ColumnOutOfRange {
                column: filter.column,
                row,
            });
        }
    }
    Ok(())
}
