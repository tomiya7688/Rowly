//! ```text
//! 責務: [
//! three_way: Base / Local / Other の3つのtableから、cell単位の3-way差分（競合を含む）を読み取り専用に計算する
//! ]
//! 処理: [
//! 1: BaseとLocal、BaseとOtherをそれぞれ2-way差分にする
//! 2: Base上の同じcellに対する両側の変更を突き合わせ、片側のみ・同値・競合へ分類する
//! 3: cell以外（行の挿入・削除・曖昧・列数変更）は、競合を断定せず側ごとに一覧で返す
//! ]
//! 補足: [
//! 競合は2-way差分へ潰さず、Base・Local・Otherの値を同時に保持する
//! 片側が行を削除し、もう片側がその行のcellを変更した場合も競合にする
//! 行の対応が位置による仮定（Positional）の場合は、ambiguousを立てて確定扱いにしない
//! 解決は行わない。解決操作は通常のCommand Requestへ渡す
//! ]
//! ```
use std::collections::{HashMap, HashSet};

use super::{Alignment, DiffKind, cell_at, diff_tables};
use crate::process::CellRef;

/// ```text
/// 責務: [
/// MergeSide: Base上のcellに対する、片側の変更内容を表す
/// ]
/// フィールド: [
/// Unchanged: その側ではcellが変わっていない
/// Changed: 値が変わった（Noneはcellが存在しなくなった）
/// RowDeleted: そのcellを含む行がその側で削除された
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeSide {
    Unchanged,
    Changed(Option<String>),
    RowDeleted,
}

/// ```text
/// 責務: [
/// MergeKind: 3-wayでのcellの分類を表す
/// ]
/// フィールド: [
/// LocalOnly: Localだけが変更した
/// OtherOnly: Otherだけが変更した
/// BothSame: 両側が同じ値へ変更した
/// Conflict: 両側が違う値へ変更した、または片側の変更と他方の行削除が衝突した
/// ]
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeKind {
    LocalOnly,
    OtherOnly,
    BothSame,
    Conflict,
}

/// ```text
/// 責務: [
/// CellMerge: Base上の1 cellについて、Base・Local・Otherを同時に保持する
/// ]
/// フィールド: [
/// source_id: 比較したsourceの識別子
/// base: Base上のcell位置
/// base_value: Baseの値（Baseに存在しなければNone）
/// local: Local側の変更
/// other: Other側の変更
/// kind: 分類
/// ambiguous: いずれかの側の行対応が位置による仮定である
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellMerge {
    pub source_id: String,
    pub base: CellRef,
    pub base_value: Option<String>,
    pub local: MergeSide,
    pub other: MergeSide,
    pub kind: MergeKind,
    pub ambiguous: bool,
}

/// ```text
/// 責務: [
/// ThreeWayDiff: 1つのsourceの3-way差分を保持する
/// ]
/// フィールド: [
/// cells: 変更のあったcellの分類。Base上の位置順
/// local_structure: Localの行・列数の変更（cell以外）
/// other_structure: Otherの行・列数の変更（cell以外）
/// alignment_skipped: どちらかの2-way差分で対応付けを省略した
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreeWayDiff {
    pub cells: Vec<CellMerge>,
    pub local_structure: Vec<DiffKind>,
    pub other_structure: Vec<DiffKind>,
    pub alignment_skipped: bool,
}

// {
// 責務: [
// SideChanges: 片側の2-way差分を、Base上のcellと行で引ける形にまとめる
// ]
// フィールド: [
// cells: Base上のcell位置から（変更後の値, 位置による仮定か）
// deleted_rows: 削除されたBaseのrow index
// structure: cell以外の差分
// ]
// }
#[derive(Default)]
struct SideChanges {
    cells: HashMap<CellRef, (Option<String>, bool)>,
    deleted_rows: HashSet<usize>,
    structure: Vec<DiffKind>,
}

// {
// 責務: [
// collect_side: 2-way差分の一覧を、Base基準のSideChangesへ振り分ける
// ]
// 引数: [
// kinds: impl Iterator<Item = DiffKind>
// ]
// 戻り値: [
// SideChanges: 振り分けた結果
// ]
// }
fn collect_side(kinds: impl Iterator<Item = DiffKind>) -> SideChanges {
    let mut side = SideChanges::default();
    for kind in kinds {
        match kind {
            DiffKind::CellChanged {
                before,
                after_value,
                alignment,
                ..
            } => {
                side.cells
                    .insert(before, (after_value, alignment == Alignment::Positional));
            }
            DiffKind::HeaderChanged {
                column,
                after_value,
                ..
            } => {
                side.cells
                    .insert(CellRef::new(0, column), (after_value, false));
            }
            other => {
                if let DiffKind::RowDeleted { row } = other {
                    side.deleted_rows.insert(row);
                }
                side.structure.push(other);
            }
        }
    }
    side
}

// {
// 責務: [
// side_of: Base上のcellに対する片側の変更を求める
// ]
// 引数: [
// side: &SideChanges
// cell: CellRef
// ]
// 戻り値: [
// MergeSide: その側の変更内容
// ]
// }
fn side_of(side: &SideChanges, cell: CellRef) -> MergeSide {
    if let Some((value, _)) = side.cells.get(&cell) {
        MergeSide::Changed(value.clone())
    } else if side.deleted_rows.contains(&cell.row()) {
        MergeSide::RowDeleted
    } else {
        MergeSide::Unchanged
    }
}

/// ```text
/// 責務: [
/// diff_three_way: Base / Local / Other を比較し、cell単位の分類と競合を返す
/// ]
/// 処理: [
/// 1: BaseとLocal、BaseとOtherの2-way差分を作る
/// 2: 変更のあった全cellについて、両側の変更を突き合わせて分類する
/// 3: cell以外の変更を側ごとにそのまま返す
/// ]
/// 引数: [
/// source_id: 比較するphysical sourceの識別子
/// base: 共通の祖先table
/// local: 自分側のtable
/// other: 相手側のtable
/// ]
/// 戻り値: [
/// ThreeWayDiff: cell分類、側ごとの構造変更、対応付け省略の有無
/// ]
/// 副作用: [
/// なし（共有参照だけを受け取り、tableを変更しない）
/// ]
/// ```
pub fn diff_three_way(
    source_id: &str,
    base: &[Vec<String>],
    local: &[Vec<String>],
    other: &[Vec<String>],
) -> ThreeWayDiff {
    let local_diff = diff_tables(source_id, base, local);
    let other_diff = diff_tables(source_id, base, other);
    let alignment_skipped = local_diff.alignment_skipped || other_diff.alignment_skipped;
    let local_side = collect_side(local_diff.changes.into_iter().map(|change| change.kind));
    let other_side = collect_side(other_diff.changes.into_iter().map(|change| change.kind));

    let mut keys: Vec<CellRef> = local_side
        .cells
        .keys()
        .chain(other_side.cells.keys())
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    keys.sort();

    let cells = keys
        .into_iter()
        .map(|cell| {
            let (local_change, other_change) =
                (side_of(&local_side, cell), side_of(&other_side, cell));
            let kind = match (&local_change, &other_change) {
                (MergeSide::Changed(a), MergeSide::Changed(b)) if a == b => MergeKind::BothSame,
                (MergeSide::Changed(_), MergeSide::Unchanged) => MergeKind::LocalOnly,
                (MergeSide::Unchanged, MergeSide::Changed(_)) => MergeKind::OtherOnly,
                _ => MergeKind::Conflict,
            };
            let positional = |side: &SideChanges| side.cells.get(&cell).is_some_and(|(_, p)| *p);
            CellMerge {
                source_id: source_id.to_owned(),
                base: cell,
                base_value: base
                    .get(cell.row())
                    .and_then(|row| cell_at(row, cell.column())),
                ambiguous: positional(&local_side) || positional(&other_side),
                local: local_change,
                other: other_change,
                kind,
            }
        })
        .collect();

    ThreeWayDiff {
        cells,
        local_structure: local_side.structure,
        other_structure: other_side.structure,
        alignment_skipped,
    }
}
