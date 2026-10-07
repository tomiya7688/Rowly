use std::collections::{BTreeSet, VecDeque};

use super::{CalculationBindingId, CalculationEngine, CalculationStatus};
use crate::process::CellRef;

impl CalculationEngine {
    // ```text
    // 責務: [remove_dependency_index: bindingを1つのcell依存indexから外し空entryを削除する]
    // 引数: [self: reverse dependency index, dependency: 参照cell, id: indexから外すbinding ID]
    // 戻り値: [(): reverse dependency indexを更新する]
    // ```
    pub(super) fn remove_dependency_index(
        &mut self,
        dependency: CellRef,
        id: &CalculationBindingId,
    ) {
        let Some(dependent_ids) = self.dependents_by_cell.get_mut(&dependency) else {
            return;
        };
        dependent_ids.remove(id);
        if dependent_ids.is_empty() {
            self.dependents_by_cell.remove(&dependency);
        }
    }

    // ```text
    // 責務: [mark_dependents_stale: changed output cellから到達する下流resultをstaleまたはpendingにする]
    // 引数: [self: binding graphとresult状態, changed_targets: graph変更で値が変わるtarget cells]
    // 戻り値: [(): affected result stateを再評価待ちへ移す]
    // ```
    pub(super) fn mark_dependents_stale(
        &mut self,
        changed_targets: impl IntoIterator<Item = CellRef>,
    ) {
        let mut pending_cells = VecDeque::from_iter(changed_targets);
        let mut visited_bindings = BTreeSet::new();
        while let Some(changed_cell) = pending_cells.pop_front() {
            let dependent_ids = self
                .dependents_by_cell
                .get(&changed_cell)
                .map(|ids| ids.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();

            for dependent_id in dependent_ids {
                if !visited_bindings.insert(dependent_id.clone()) {
                    continue;
                }
                if let Some(binding) = self.bindings.get(&dependent_id)
                    && let Some(target_cell) = binding.target.single_cell()
                {
                    pending_cells.push_back(target_cell);
                }
                if let Some(result) = self.results.get_mut(&dependent_id) {
                    result.status = if result.value.is_some() {
                        CalculationStatus::Stale
                    } else {
                        CalculationStatus::Pending
                    };
                }
            }
        }
    }
}
