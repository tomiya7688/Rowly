//! ```text
//! 責務: [
//! diff: 2つのCSV table（文字列の行列）の差分を、source identityとcell位置（provenance）付きで読み取り専用に計算する
//! ]
//! 処理: [
//! 1: header行（0行目）を位置ごとに比較し、列数が違えばStructureChangedを出す
//! 2: データ行を、先頭・末尾の一致を除いたうえで最長共通部分列（LCS）で対応付ける
//! 3: 対応しない区間は、行数が同じなら位置で対にしてcell差分にし（Positional）、違えば行の挿入・削除にする
//! 4: 同じ内容の行が削除側と挿入側の両方にある場合は、移動と断定せずRowMovedOrAmbiguousにまとめる
//! ]
//! 補足: [
//! 引数は共有参照だけを受け取り、計算でtableを変更しない
//! 比較はraw文字列で行い、表示形式は見ない。行が短い（ragged）場合、存在しないcellはNoneとして区別する
//! CSVにstable row IDがないため、重複行や移動は行の対応を断定しない
//! 行数の積がLCS_CELL_LIMITを超える場合は対応付けを省略し、その旨をalignment_skippedに残す
//! ]
//! ```
mod three_way;

use std::collections::HashMap;

use super::CellRef;
pub use three_way::{CellMerge, MergeKind, MergeSide, ThreeWayDiff, diff_three_way};

/// 対応付けに使うLCS表の最大cell数（これを超えると対応付けを省略する）。
const LCS_CELL_LIMIT: usize = 4_000_000;

/// ```text
/// 責務: [
/// Alignment: 行の対応が確定的か、位置による仮定かを表す
/// ]
/// フィールド: [
/// Anchored: 前後の一致行（または表の端）に挟まれた1行対1行で、対応が確定している
/// Positional: 不一致区間の行数が同じため、位置で対にした仮定
/// ]
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    Anchored,
    Positional,
}

/// ```text
/// 責務: [
/// DiffKind: 差分1件の種類と、before/afterの位置・値を表す
/// ]
/// フィールド: [
/// CellChanged: cellの値が変わった（位置、値、対応の確かさ）。値のNoneはcellが存在しない
/// HeaderChanged: header行の列名が変わった、または列が増減した
/// RowInserted: afterだけにある行（afterのrow index）
/// RowDeleted: beforeだけにある行（beforeのrow index）
/// RowMovedOrAmbiguous: 同じ内容の行が両側にあり、移動・重複・入替を区別できない
/// StructureChanged: header行の列数が変わった
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffKind {
    CellChanged {
        before: CellRef,
        after: CellRef,
        before_value: Option<String>,
        after_value: Option<String>,
        alignment: Alignment,
    },
    HeaderChanged {
        column: usize,
        before_value: Option<String>,
        after_value: Option<String>,
    },
    RowInserted {
        row: usize,
    },
    RowDeleted {
        row: usize,
    },
    RowMovedOrAmbiguous {
        before_rows: Vec<usize>,
        after_rows: Vec<usize>,
    },
    StructureChanged {
        before_columns: usize,
        after_columns: usize,
    },
}

/// ```text
/// 責務: [
/// DiffChange: 1件の差分に、比較したphysical sourceの識別子を付ける
/// ]
/// フィールド: [
/// source_id: 比較したsourceの識別子。同じA2でもsourceが違えば別cellとして扱う
/// kind: 差分の種類と位置
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffChange {
    pub source_id: String,
    pub kind: DiffKind,
}

/// ```text
/// 責務: [
/// TableDiff: 1つのsourceの2-way差分の一覧を保持する
/// ]
/// フィールド: [
/// changes: 差分一覧。header、データ行の順に並ぶ
/// alignment_skipped: 規模が大きく行の対応付け（LCS）を省略した場合true
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableDiff {
    pub changes: Vec<DiffChange>,
    pub alignment_skipped: bool,
}

// {
// 責務: [
// cell_at: 行のcell値を、存在しなければNoneで返す
// ]
// 引数: [
// row: &[String]
// column: usize
// ]
// 戻り値: [
// Option<String>: cell値。列がなければNone
// ]
// }
pub(super) fn cell_at(row: &[String], column: usize) -> Option<String> {
    row.get(column).cloned()
}

// {
// 責務: [
// lcs_pairs: 2つの行列の最長共通部分列の対応（添字の組）を返す
// ]
// 処理: [
// 1: 行数の積が上限を超えたらNoneを返す
// 2: 後ろから埋めたLCS表を、前から辿って一致行の組を集める
// ]
// 引数: [
// before: &[Vec<String>]
// after: &[Vec<String>]
// ]
// 戻り値: [
// Option<Vec<(usize, usize)>>: 一致行の組。上限超過ならNone
// ]
// }
fn lcs_pairs(before: &[Vec<String>], after: &[Vec<String>]) -> Option<Vec<(usize, usize)>> {
    if before.is_empty() || after.is_empty() {
        return Some(Vec::new());
    }
    if before.len().saturating_mul(after.len()) > LCS_CELL_LIMIT {
        return None;
    }
    let width = after.len() + 1;
    let mut table = vec![0u32; (before.len() + 1) * width];
    for i in (0..before.len()).rev() {
        for j in (0..after.len()).rev() {
            table[i * width + j] = if before[i] == after[j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut pairs = Vec::new();
    while i < before.len() && j < after.len() {
        if before[i] == after[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    Some(pairs)
}

// {
// 責務: [
// push_row_cells: 対応づけた2行のcell差分を差分一覧へ追加する
// ]
// 処理: [
// 1: 2行の長い方の幅まで、同じ列同士を比較する
// 2: 値（または存在）が違う列だけCellChangedを追加する
// ]
// 引数: [
// changes: &mut Vec<DiffKind>
// before: (usize, &[String])  header込みのrow indexと行
// after: (usize, &[String])  header込みのrow indexと行
// alignment: Alignment
// ]
// 戻り値: []
// }
fn push_row_cells(
    changes: &mut Vec<DiffKind>,
    before: (usize, &[String]),
    after: (usize, &[String]),
    alignment: Alignment,
) {
    for column in 0..before.1.len().max(after.1.len()) {
        let (before_value, after_value) = (cell_at(before.1, column), cell_at(after.1, column));
        if before_value != after_value {
            changes.push(DiffKind::CellChanged {
                before: CellRef::new(before.0, column),
                after: CellRef::new(after.0, column),
                before_value,
                after_value,
                alignment,
            });
        }
    }
}

// {
// 責務: [
// merge_ambiguous: 同じ内容の行が削除側と挿入側の両方にあるものを、曖昧な差分へまとめる
// ]
// 処理: [
// 1: 削除・挿入された行を内容ごとに集める（添字はデータ行の0始まり）
// 2: 両側にある内容、または片側に複数ある重複内容は、全出現位置を1件のRowMovedOrAmbiguousにまとめる
// 3: 片側だけの行はそのまま残す
// ]
// 引数: [
// changes: Vec<DiffKind>  行の挿入・削除を含む差分
// before_rows: &[Vec<String>]
// after_rows: &[Vec<String>]
// ]
// 戻り値: [
// Vec<DiffKind>: 曖昧な行をまとめた差分
// ]
// }
fn merge_ambiguous(
    changes: Vec<DiffKind>,
    before_rows: &[Vec<String>],
    after_rows: &[Vec<String>],
) -> Vec<DiffKind> {
    let mut groups: HashMap<&Vec<String>, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for change in &changes {
        match change {
            DiffKind::RowDeleted { row } => {
                groups.entry(&before_rows[*row]).or_default().0.push(*row);
            }
            DiffKind::RowInserted { row } => {
                groups.entry(&after_rows[*row]).or_default().1.push(*row);
            }
            _ => {}
        }
    }
    // 差分に現れた内容について、変更されていない行も含む全出現位置を集める。
    // 重複行は、どの出現がどれに対応するか断定できないため曖昧として扱う。
    let mut occurrences: HashMap<&Vec<String>, (Vec<usize>, Vec<usize>)> = HashMap::new();
    for (index, row) in before_rows.iter().enumerate() {
        if groups.contains_key(row) {
            occurrences.entry(row).or_default().0.push(index);
        }
    }
    for (index, row) in after_rows.iter().enumerate() {
        if groups.contains_key(row) {
            occurrences.entry(row).or_default().1.push(index);
        }
    }
    let mut emitted: Vec<&Vec<String>> = Vec::new();
    let mut merged = Vec::new();
    for change in &changes {
        let content = match change {
            DiffKind::RowDeleted { row } => &before_rows[*row],
            DiffKind::RowInserted { row } => &after_rows[*row],
            other => {
                merged.push(other.clone());
                continue;
            }
        };
        let (deleted, inserted) = &groups[content];
        let (all_before, all_after) = &occurrences[content];
        let ambiguous = (!deleted.is_empty() && !inserted.is_empty())
            || all_before.len() > 1
            || all_after.len() > 1;
        if !ambiguous {
            merged.push(change.clone());
        } else if !emitted.contains(&content) {
            emitted.push(content);
            merged.push(DiffKind::RowMovedOrAmbiguous {
                before_rows: all_before.clone(),
                after_rows: all_after.clone(),
            });
        }
    }
    merged
}

// {
// 責務: [
// offset_rows: データ行の0始まり添字を、header込みのrow indexへ直す（挿入・削除・曖昧のみ）
// ]
// 引数: [
// kind: DiffKind
// ]
// 戻り値: [
// DiffKind: row indexに1を足した差分。CellChanged等はそのまま
// ]
// }
fn offset_rows(kind: DiffKind) -> DiffKind {
    match kind {
        DiffKind::RowInserted { row } => DiffKind::RowInserted { row: row + 1 },
        DiffKind::RowDeleted { row } => DiffKind::RowDeleted { row: row + 1 },
        DiffKind::RowMovedOrAmbiguous {
            before_rows,
            after_rows,
        } => DiffKind::RowMovedOrAmbiguous {
            before_rows: before_rows.into_iter().map(|row| row + 1).collect(),
            after_rows: after_rows.into_iter().map(|row| row + 1).collect(),
        },
        other => other,
    }
}

/// ```text
/// 責務: [
/// diff_tables: 同じsourceの2つのtableを比較し、provenance付きの差分一覧を返す
/// ]
/// 処理: [
/// 1: header行を位置ごとに比較する
/// 2: データ行を先頭・末尾の一致除去とLCSで対応付け、不一致区間を差分にする
/// 3: 同じ内容の行が両側にある挿入・削除を曖昧な差分へまとめる
/// ]
/// 引数: [
/// source_id: 比較するphysical sourceの識別子
/// before: 変更前のtable（先頭行がheader）
/// after: 変更後のtable（先頭行がheader）
/// ]
/// 戻り値: [
/// TableDiff: 差分一覧と、対応付けを省略したかどうか
/// ]
/// 副作用: [
/// なし（共有参照だけを受け取り、tableを変更しない）
/// ]
/// ```
pub fn diff_tables(source_id: &str, before: &[Vec<String>], after: &[Vec<String>]) -> TableDiff {
    let mut changes = Vec::new();
    let before_header = before.first().map_or(&[][..], Vec::as_slice);
    let after_header = after.first().map_or(&[][..], Vec::as_slice);
    if before_header.len() != after_header.len() {
        changes.push(DiffKind::StructureChanged {
            before_columns: before_header.len(),
            after_columns: after_header.len(),
        });
    }
    for column in 0..before_header.len().max(after_header.len()) {
        let (before_value, after_value) = (
            cell_at(before_header, column),
            cell_at(after_header, column),
        );
        if before_value != after_value {
            changes.push(DiffKind::HeaderChanged {
                column,
                before_value,
                after_value,
            });
        }
    }

    // データ行は1行目以降。比較用の添字は0始まりで、出力時にheader分の1を足す。
    let before_data = before.get(1..).unwrap_or(&[]);
    let after_data = after.get(1..).unwrap_or(&[]);
    let prefix = before_data
        .iter()
        .zip(after_data)
        .take_while(|(b, a)| b == a)
        .count();
    let suffix = before_data[prefix..]
        .iter()
        .rev()
        .zip(after_data[prefix..].iter().rev())
        .take_while(|(b, a)| b == a)
        .count();
    let middle_before = &before_data[prefix..before_data.len() - suffix];
    let middle_after = &after_data[prefix..after_data.len() - suffix];

    let pairs = lcs_pairs(middle_before, middle_after);
    let alignment_skipped = pairs.is_none();
    let mut anchors = pairs.unwrap_or_default();
    anchors.push((middle_before.len(), middle_after.len()));

    let mut row_changes = Vec::new();
    let (mut before_cursor, mut after_cursor) = (0, 0);
    for (before_end, after_end) in anchors {
        let deleted = before_cursor..before_end;
        let inserted = after_cursor..after_end;
        if deleted.len() == inserted.len() {
            // 前後の一致行に挟まれた1行対1行は確定的、複数行は位置で対にした仮定。
            let alignment = if deleted.len() == 1 {
                Alignment::Anchored
            } else {
                Alignment::Positional
            };
            for (b, a) in deleted.zip(inserted) {
                let (b, a) = (prefix + b, prefix + a);
                push_row_cells(
                    &mut row_changes,
                    (b + 1, &before_data[b]),
                    (a + 1, &after_data[a]),
                    alignment,
                );
            }
        } else {
            row_changes.extend(deleted.map(|b| DiffKind::RowDeleted { row: prefix + b }));
            row_changes.extend(inserted.map(|a| DiffKind::RowInserted { row: prefix + a }));
        }
        before_cursor = before_end + 1;
        after_cursor = after_end + 1;
    }
    changes.extend(
        merge_ambiguous(row_changes, before_data, after_data)
            .into_iter()
            .map(offset_rows),
    );
    TableDiff {
        changes: changes
            .into_iter()
            .map(|kind| DiffChange {
                source_id: source_id.to_owned(),
                kind,
            })
            .collect(),
        alignment_skipped,
    }
}

#[cfg(test)]
mod tests;
