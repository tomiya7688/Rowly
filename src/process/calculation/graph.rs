use std::collections::{BTreeMap, BTreeSet};

use super::{CalculationBindingId, CalculationEngine};

impl CalculationEngine {
    // ```text
    // 責務: [evaluation_order: 選択されたdependency graphをKahn algorithmで順序付けする]
    // 引数: [self: targetとbindingを保持するengine, selected: 評価対象binding IDs]
    // 戻り値: [(Vec<ID>, Vec<ID>): dependency順の評価IDとcycleまたはcycle依存で評価不可のID]
    // ```
    pub(super) fn evaluation_order(
        &self,
        selected: &BTreeSet<CalculationBindingId>,
    ) -> (Vec<CalculationBindingId>, Vec<CalculationBindingId>) {
        let mut indegree = selected
            .iter()
            .cloned()
            .map(|id| (id, 0usize))
            .collect::<BTreeMap<_, _>>();
        let mut dependents =
            BTreeMap::<CalculationBindingId, BTreeSet<CalculationBindingId>>::new();

        for id in selected {
            let Some(binding) = self.bindings.get(id) else {
                continue;
            };
            for dependency in &binding.dependencies {
                let Some(dependency_id) = self.target_bindings.get(dependency) else {
                    continue;
                };
                if selected.contains(dependency_id)
                    && dependents
                        .entry(dependency_id.clone())
                        .or_default()
                        .insert(id.clone())
                {
                    *indegree.entry(id.clone()).or_default() += 1;
                }
            }
        }

        let mut ready = indegree
            .iter()
            .filter_map(|(id, count)| (*count == 0).then_some(id.clone()))
            .collect::<BTreeSet<_>>();
        let mut order = Vec::with_capacity(selected.len());

        while let Some(id) = ready.pop_first() {
            order.push(id.clone());
            if let Some(next_ids) = dependents.get(&id) {
                for next_id in next_ids {
                    let Some(count) = indegree.get_mut(next_id) else {
                        continue;
                    };
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(next_id.clone());
                    }
                }
            }
        }

        let processed = order.iter().cloned().collect::<BTreeSet<_>>();
        let blocked = selected.difference(&processed).cloned().collect::<Vec<_>>();
        (order, blocked)
    }
}
