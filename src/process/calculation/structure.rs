//! 行・列の挿入削除に合わせてBindingのtargetと式内の参照を付け替える。

use std::collections::BTreeMap;

use super::CalculationEngine;
use super::types::{
    CalculationBindingId, CalculationError, CalculationResult, CalculationStatus, CalculationTarget,
};
use crate::process::CellRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [StructureEdit: CsvDocumentへ適用した行・列の構造編集をcalculation engineへ伝える]
/// 補足: [indexはzero-based。DeleteはRowly Document APIと同じ[index, index+count)を削除する]
/// ```
pub enum StructureEdit {
    /// ``index`` の位置へ``count``行を挿入した。
    InsertRows {
        /// 挿入位置。
        index: usize,
        /// 挿入行数。
        count: usize,
    },
    /// ``index`` から``count``行を削除した。
    DeleteRows {
        /// 削除開始位置。
        index: usize,
        /// 削除行数。
        count: usize,
    },
    /// ``index`` の位置へ``count``列を挿入した。
    InsertColumns {
        /// 挿入位置。
        index: usize,
        /// 挿入列数。
        count: usize,
    },
    /// ``index`` から``count``列を削除した。
    DeleteColumns {
        /// 削除開始位置。
        index: usize,
        /// 削除列数。
        count: usize,
    },
}

impl StructureEdit {
    /// {
    ///   責務: [map_cell: 編集前のcell位置を編集後の位置へ写す]
    ///   引数: [cell: 編集前の位置]
    ///   戻り値: [Option<CellRef>: 編集後の位置。削除された行・列上ならNone]
    /// }
    pub fn map_cell(self, cell: CellRef) -> Option<CellRef> {
        match self {
            Self::InsertRows { index, count } => Some(CellRef::new(
                insert_axis(cell.row(), index, count)?,
                cell.column(),
            )),
            Self::DeleteRows { index, count } => Some(CellRef::new(
                delete_axis(cell.row(), index, count)?,
                cell.column(),
            )),
            Self::InsertColumns { index, count } => Some(CellRef::new(
                cell.row(),
                insert_axis(cell.column(), index, count)?,
            )),
            Self::DeleteColumns { index, count } => Some(CellRef::new(
                cell.row(),
                delete_axis(cell.column(), index, count)?,
            )),
        }
    }
}

// {
//   責務: [insert_axis: 挿入位置以降の座標を挿入数だけ後ろへ移す]
//   引数: [position: 旧座標, index: 挿入位置, count: 挿入数]
//   戻り値: [Option<usize>: 新座標。usizeを超える場合はNone]
// }
fn insert_axis(position: usize, index: usize, count: usize) -> Option<usize> {
    if position >= index {
        position.checked_add(count)
    } else {
        Some(position)
    }
}

// {
//   責務: [delete_axis: 削除範囲内の座標を削除扱いにし、後続の座標を前へ詰める]
//   引数: [position: 旧座標, index: 削除開始位置, count: 削除数]
//   戻り値: [Option<usize>: 新座標。削除範囲内ならNone]
// }
fn delete_axis(position: usize, index: usize, count: usize) -> Option<usize> {
    if position < index {
        Some(position)
    } else if position - index < count {
        None
    } else {
        Some(position - count)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// ```text
/// 責務: [StructureEditReport: 構造編集で付け替えたBindingと、target削除で取り除いたBindingを報告する]
/// ```
pub struct StructureEditReport {
    /// target・式を付け替えて再計算待ちにしたbinding ID。
    pub rewritten: Vec<CalculationBindingId>,
    /// targetの行・列が削除されたため取り除いたbinding ID。
    pub removed: Vec<CalculationBindingId>,
    /// `dsl:<cell>` 形式のIDを新しいtargetへ付け替えた(旧ID, 新ID)の一覧。
    pub renamed: Vec<(CalculationBindingId, CalculationBindingId)>,
}

impl CalculationEngine {
    /// ```text
    /// 責務: [apply_structure_edit: 構造編集後もBindingが同じ論理cellを指すようtargetと式の参照を付け替える]
    /// 処理: [target位置と全cell参照（相対・絶対とも）を移動し、削除された参照先はDeletedReference、削除されたtargetのbindingは取り除く。全resultは再計算待ちにする]
    /// 引数: [edit: CsvDocumentへ適用済みの構造編集]
    /// 戻り値: [Result<StructureEditReport, CalculationError>: 付け替え・削除したID、revision overflow]
    /// 副作用: [engine内のrule・index・derived resultだけを更新しCsvDocumentは変更しない。呼び出し側がrecalculate_allで再評価する]
    /// ```
    pub fn apply_structure_edit(
        &mut self,
        edit: StructureEdit,
    ) -> Result<StructureEditReport, CalculationError> {
        let (StructureEdit::InsertRows { count, .. }
        | StructureEdit::DeleteRows { count, .. }
        | StructureEdit::InsertColumns { count, .. }
        | StructureEdit::DeleteColumns { count, .. }) = edit;
        if count == 0 {
            // 位置が変わらない編集ではruleもresultも変えない。
            return Ok(StructureEditReport::default());
        }
        let binding_revision = self
            .binding_revision
            .checked_add(1)
            .ok_or(CalculationError::RevisionOverflow)?;
        let derived_revision = self
            .derived_revision
            .checked_add(1)
            .ok_or(CalculationError::RevisionOverflow)?;
        let map = |cell: CellRef| edit.map_cell(cell);

        let mut report = StructureEditReport::default();
        let mut bindings = BTreeMap::new();
        let mut old_results = std::mem::take(&mut self.results);
        let mut results = BTreeMap::new();
        for (id, mut binding) in std::mem::take(&mut self.bindings) {
            let mut new_id = id.clone();
            // engineはset_bindingで単一cell targetだけを受け付けるため、Range・ColumnRelativeは登録されない。
            // 将来それらを許可する場合は、ここでtargetの写像を定義してから通す。
            match binding.target {
                CalculationTarget::Cell(target) => {
                    let Some(new_target) = edit.map_cell(target) else {
                        report.removed.push(id.clone());
                        continue;
                    };
                    // DSL宣言とGUIは`dsl:<target>`をIDとして導出するため、IDもtargetへ追従させる。
                    // cell写像は単射なので、付け替え後のIDどうしは衝突しない。
                    if id.as_str() == format!("dsl:{target}") {
                        new_id = CalculationBindingId::new(format!("dsl:{new_target}"))?;
                    }
                    binding.target = CalculationTarget::Cell(new_target);
                }
                CalculationTarget::Range(_) | CalculationTarget::ColumnRelative { .. } => {}
            }
            binding.expression = binding.expression.remap_cell_references(&map);
            binding.dependencies = binding.expression.dependencies();
            binding.revision = binding_revision;
            if new_id != id {
                report.renamed.push((id.clone(), new_id.clone()));
                binding.id = new_id.clone();
            }
            if let Some(result) = old_results.remove(&id) {
                results.insert(new_id.clone(), result);
            }
            report.rewritten.push(new_id.clone());
            bindings.insert(new_id, binding);
        }
        self.bindings = bindings;
        self.results = results;
        self.rebuild_indexes();

        for result in self.results.values_mut() {
            *result = pending_result(result, binding_revision, derived_revision);
        }
        self.binding_revision = binding_revision;
        self.derived_revision = derived_revision;
        Ok(report)
    }

    // {
    //   責務: [rebuild_indexes: bindingsからtargetとreverse dependencyのindexを作り直す]
    //   戻り値: [(): indexを更新する]
    // }
    fn rebuild_indexes(&mut self) {
        self.target_bindings.clear();
        self.dependents_by_cell.clear();
        for binding in self.bindings.values() {
            if let Some(target) = binding.target.single_cell() {
                self.target_bindings.insert(target, binding.id.clone());
            }
            for dependency in &binding.dependencies {
                self.dependents_by_cell
                    .entry(*dependency)
                    .or_default()
                    .insert(binding.id.clone());
            }
        }
    }
}

// {
//   責務: [pending_result: 旧resultを値を保ったまま再計算待ち（値なしはPending、ありはStale）にする]
//   引数: [previous: 旧result, binding_revision: 新binding revision, derived_revision: 新derived revision]
//   戻り値: [CalculationResult: 再計算待ちのresult]
// }
fn pending_result(
    previous: &CalculationResult,
    binding_revision: u64,
    derived_revision: u64,
) -> CalculationResult {
    CalculationResult {
        value: previous.value.clone(),
        status: if previous.value.is_some() {
            CalculationStatus::Stale
        } else {
            CalculationStatus::Pending
        },
        binding_revision,
        derived_revision,
        dependencies: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // {
    //   責務: [map_cell_handles_insert_and_delete_boundaries: 挿入削除の境界位置の写像を検証する]
    //   戻り値: [(): 挿入位置以降の移動、削除範囲のNone、後続の詰めが仕様どおりなら成功する]
    // }
    #[test]
    fn map_cell_handles_insert_and_delete_boundaries() {
        let at = |row, column| CellRef::new(row, column);
        let insert = StructureEdit::InsertRows { index: 2, count: 3 };
        assert_eq!(insert.map_cell(at(1, 0)), Some(at(1, 0)));
        assert_eq!(insert.map_cell(at(2, 0)), Some(at(5, 0)));
        let delete = StructureEdit::DeleteRows { index: 2, count: 2 };
        assert_eq!(delete.map_cell(at(1, 4)), Some(at(1, 4)));
        assert_eq!(delete.map_cell(at(2, 4)), None);
        assert_eq!(delete.map_cell(at(3, 4)), None);
        assert_eq!(delete.map_cell(at(4, 4)), Some(at(2, 4)));
        let columns = StructureEdit::DeleteColumns { index: 0, count: 1 };
        assert_eq!(columns.map_cell(at(7, 0)), None);
        assert_eq!(columns.map_cell(at(7, 1)), Some(at(7, 0)));
        let overflow = StructureEdit::InsertColumns {
            index: 0,
            count: usize::MAX,
        };
        assert_eq!(overflow.map_cell(at(0, 1)), None);
    }
}
