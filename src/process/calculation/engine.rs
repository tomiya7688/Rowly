use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use super::expression::{MAX_EXPRESSION_DEPTH, evaluate_expression, expression_depth};
use super::types::{
    CalculationBinding, CalculationBindingId, CalculationDependencySnapshot, CalculationError,
    CalculationExpression, CalculationFailure, CalculationRecalculationReport, CalculationResult,
    CalculationStatus, CalculationTarget, CalculationTrigger, CalculationValue,
};
use crate::process::{CellRef, CsvDocument};

/// ```text
/// 責務: [CalculationEngine: CSV外のcalculation bindingを評価しderived resultだけを保持する]
/// フィールド: [bindings: stable ID順のrule一覧, results: binding別derived state, revisions: ruleとresultのrevision counter]
/// 補足: [engineはCsvDocumentを書き換えず、依存変更後または明示Recalculate時に呼び出す]
/// ```
#[derive(Debug, Default)]
pub struct CalculationEngine {
    bindings: BTreeMap<CalculationBindingId, CalculationBinding>,
    results: BTreeMap<CalculationBindingId, CalculationResult>,
    binding_revision: u64,
    derived_revision: u64,
}

impl CalculationEngine {
    /// ```text
    /// 責務: [set_binding: 一意な既存cellへpure calculation ruleを追加または置換する]
    /// 処理: [targetとIDを検証し、expression dependencyと新revisionを記録してpending resultを作る]
    /// 引数: [document: canonical CSV読取元, id: stable binding ID, target: 結果cell, expression: pure AST, trigger: 再評価条件]
    /// 戻り値: [Result<u64, CalculationError>: 登録したbinding revisionまたは構成error]
    /// ```
    pub fn set_binding(
        &mut self,
        document: &CsvDocument,
        id: CalculationBindingId,
        target: CalculationTarget,
        expression: CalculationExpression,
        trigger: CalculationTrigger,
    ) -> Result<u64, CalculationError> {
        let Some(target_cell) = target.single_cell() else {
            return Err(CalculationError::UnsupportedTarget);
        };
        if document.cell_ref(target_cell).is_none() {
            return Err(CalculationError::MissingTarget(target_cell));
        }

        let target_is_owned = self.bindings.iter().any(|(existing_id, binding)| {
            existing_id != &id && binding.target.single_cell() == Some(target_cell)
        });
        if target_is_owned {
            return Err(CalculationError::DuplicateTarget(target_cell));
        }

        if let Some(existing) = self.bindings.get(&id)
            && existing.target == target
            && existing.expression == expression
            && existing.trigger == trigger
        {
            return Ok(existing.revision);
        }

        let binding_revision = self
            .binding_revision
            .checked_add(1)
            .ok_or(CalculationError::RevisionOverflow)?;
        let derived_revision = self
            .derived_revision
            .checked_add(1)
            .ok_or(CalculationError::RevisionOverflow)?;
        let dependencies = expression.dependencies();
        if expression_depth(&expression) > MAX_EXPRESSION_DEPTH {
            return Err(CalculationError::ExpressionTooDeep);
        }

        let binding = CalculationBinding {
            id: id.clone(),
            target,
            expression,
            dependencies,
            trigger,
            revision: binding_revision,
        };
        let pending_result = CalculationResult {
            value: None,
            status: CalculationStatus::Pending,
            binding_revision,
            derived_revision,
            dependencies: Vec::new(),
        };

        self.bindings.insert(id.clone(), binding);
        self.results.insert(id, pending_result);
        self.binding_revision = binding_revision;
        self.derived_revision = derived_revision;
        Ok(binding_revision)
    }

    /// ```text
    /// 責務: [remove_binding: bindingとそのderived resultをengineから削除する]
    /// 引数: [id: 削除対象stable ID]
    /// 戻り値: [bool: bindingが存在して削除された場合true]
    /// ```
    pub fn remove_binding(&mut self, id: &CalculationBindingId) -> bool {
        self.results.remove(id);
        self.bindings.remove(id).is_some()
    }

    /// ```text
    /// 責務: [binding: stable IDで登録済みruleを参照する]
    /// 引数: [id: 検索するbinding ID]
    /// 戻り値: [Option<&CalculationBinding>: 存在するimmutable rule]
    /// ```
    pub fn binding(&self, id: &CalculationBindingId) -> Option<&CalculationBinding> {
        self.bindings.get(id)
    }

    /// ```text
    /// 責務: [result: stable IDに対応するderived resultを参照する]
    /// 引数: [id: 検索するbinding ID]
    /// 戻り値: [Option<&CalculationResult>: 計算値と状態のimmutable snapshot]
    /// ```
    pub fn result(&self, id: &CalculationBindingId) -> Option<&CalculationResult> {
        self.results.get(id)
    }

    /// ```text
    /// 責務: [bindings: stable ID順で登録bindingを列挙する]
    /// 引数: [self: 対象engine]
    /// 戻り値: [impl Iterator<Item = &CalculationBinding>: 遅延binding iterator]
    /// ```
    pub fn bindings(&self) -> impl Iterator<Item = &CalculationBinding> {
        self.bindings.values()
    }

    /// ```text
    /// 責務: [recalculate_all: 全bindingをdependency順に明示再計算する]
    /// 引数: [document: canonical CSV read-only source]
    /// 戻り値: [Result<CalculationRecalculationReport, CalculationError>: 評価済み、stale、failed ID一覧]
    /// 副作用: [engine内のderived resultだけを更新しCsvDocumentは変更しない]
    /// ```
    pub fn recalculate_all(
        &mut self,
        document: &CsvDocument,
    ) -> Result<CalculationRecalculationReport, CalculationError> {
        let selected = self.bindings.keys().cloned().collect();
        self.recalculate_selected(document, selected, true)
    }

    /// ```text
    /// 責務: [recalculate_for_changes: changed cellから依存するbindingと下流ruleを再評価する]
    /// 処理: [dependency closureを求め、DependencyChange ruleを評価しManual ruleをstaleにする]
    /// 引数: [document: 最新canonical CSV source, changed_cells: 変更済みzero-based cell iterator]
    /// 戻り値: [Result<CalculationRecalculationReport, CalculationError>: 評価状態ごとのbinding ID一覧]
    /// 副作用: [engine内のderived resultだけを更新しCsvDocumentは変更しない]
    /// ```
    pub fn recalculate_for_changes(
        &mut self,
        document: &CsvDocument,
        changed_cells: impl IntoIterator<Item = CellRef>,
    ) -> Result<CalculationRecalculationReport, CalculationError> {
        let changed_cells = changed_cells.into_iter().collect::<BTreeSet<_>>();
        if changed_cells.is_empty() {
            return Ok(CalculationRecalculationReport::default());
        }

        // 依存先ごとの逆引きを一度作り、変更cellから到達できるruleだけをqueueでたどる。
        let mut dependents_by_cell = HashMap::<CellRef, Vec<CalculationBindingId>>::new();
        for (id, binding) in &self.bindings {
            for dependency in &binding.dependencies {
                dependents_by_cell
                    .entry(*dependency)
                    .or_default()
                    .push(id.clone());
            }
        }

        let mut selected = BTreeSet::new();
        let mut pending_cells = VecDeque::from_iter(changed_cells);
        while let Some(changed_cell) = pending_cells.pop_front() {
            if let Some(dependent_ids) = dependents_by_cell.get(&changed_cell) {
                for dependent_id in dependent_ids {
                    if !selected.insert(dependent_id.clone()) {
                        continue;
                    }
                    if let Some(binding) = self.bindings.get(dependent_id)
                        && let Some(target_cell) = binding.target.single_cell()
                    {
                        pending_cells.push_back(target_cell);
                    }
                }
            }
        }

        self.recalculate_selected(document, selected, false)
    }

    // ```text
    // 責務: [target_bindings: 単一cell targetからbinding IDを引けるindexを作る]
    // 引数: [self: 対象engine]
    // 戻り値: [HashMap<CellRef, CalculationBindingId>: target owner index]
    // ```
    fn target_bindings(&self) -> HashMap<CellRef, CalculationBindingId> {
        self.bindings
            .iter()
            .filter_map(|(id, binding)| {
                binding
                    .target
                    .single_cell()
                    .map(|target| (target, id.clone()))
            })
            .collect()
    }

    // ```text
    // 責務: [recalculate_selected: 選択ruleをtopological orderで処理してderived stateを更新する]
    // 処理: [cycleを検出し、triggerに従って式を評価またはstale化する]
    // 引数: [document: raw CSV reader, selected: 評価対象ID, force_all: Manual triggerも明示的に評価するか]
    // 戻り値: [Result<CalculationRecalculationReport, CalculationError>: 最終状態ごとのID一覧]
    // 補足: [計算式はCsvDocumentへ書き込まない]
    // ```
    fn recalculate_selected(
        &mut self,
        document: &CsvDocument,
        selected: BTreeSet<CalculationBindingId>,
        force_all: bool,
    ) -> Result<CalculationRecalculationReport, CalculationError> {
        let mut report = CalculationRecalculationReport::default();
        if selected.is_empty() {
            return Ok(report);
        }

        let target_bindings = self.target_bindings();
        let (evaluation_order, blocked_by_cycle) =
            self.evaluation_order(&selected, &target_bindings);
        for id in blocked_by_cycle {
            let snapshots = self.dependency_snapshots(document, &target_bindings, &id);
            self.store_result(
                id.clone(),
                None,
                CalculationStatus::Error(CalculationFailure::CycleDetected),
                snapshots,
            )?;
            report.failed.push(id);
        }

        for id in evaluation_order {
            let Some(binding) = self.bindings.get(&id).cloned() else {
                continue;
            };
            if !force_all && binding.trigger == CalculationTrigger::Manual {
                let snapshots = self.dependency_snapshots(document, &target_bindings, &id);
                self.store_stale_result(id.clone(), snapshots)?;
                report.stale.push(id);
                continue;
            }

            let mut read_cell =
                |reference| self.read_calculation_cell(document, &target_bindings, reference);
            let evaluated = evaluate_expression(&binding.expression, 0, &mut read_cell);
            let snapshots = self.dependency_snapshots_for_cells(
                document,
                &target_bindings,
                &binding.dependencies,
            );
            match evaluated {
                Ok(value) => {
                    self.store_result(
                        id.clone(),
                        Some(value.into_text()),
                        CalculationStatus::Evaluated,
                        snapshots,
                    )?;
                    report.evaluated.push(id);
                }
                Err(failure) => {
                    self.store_result(
                        id.clone(),
                        None,
                        CalculationStatus::Error(failure),
                        snapshots,
                    )?;
                    report.failed.push(id);
                }
            }
        }

        Ok(report)
    }

    // ```text
    // 責務: [evaluation_order: selected dependency graphをKahn algorithmで順序付けし残りをcycle blockedにする]
    // 引数: [selected: 対象binding IDs, target_bindings: output cell owner index]
    // 戻り値: [(Vec<ID>, Vec<ID>): dependency順の評価IDとcycleまたはcycle依存で評価不可のID]
    // ```
    fn evaluation_order(
        &self,
        selected: &BTreeSet<CalculationBindingId>,
        target_bindings: &HashMap<CellRef, CalculationBindingId>,
    ) -> (Vec<CalculationBindingId>, Vec<CalculationBindingId>) {
        let mut indegree = selected
            .iter()
            .cloned()
            .map(|id| (id, 0usize))
            .collect::<BTreeMap<_, _>>();
        let mut dependents =
            BTreeMap::<CalculationBindingId, BTreeSet<CalculationBindingId>>::new();

        for (id, binding) in &self.bindings {
            if !selected.contains(id) {
                continue;
            }
            for dependency in &binding.dependencies {
                let Some(dependency_id) = target_bindings.get(dependency) else {
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

    // ```text
    // 責務: [read_calculation_cell: bound targetのfresh derived resultまたはraw CSV cellを読む]
    // 引数: [document: canonical CSV, target_bindings: output owner index, reference: 参照cell]
    // 戻り値: [Result<CalculationValue, CalculationFailure>: Text scalarまたは依存状態error]
    // ```
    fn read_calculation_cell(
        &self,
        document: &CsvDocument,
        target_bindings: &HashMap<CellRef, CalculationBindingId>,
        reference: CellRef,
    ) -> Result<CalculationValue, CalculationFailure> {
        if let Some(binding_id) = target_bindings.get(&reference) {
            if let Some(result) = self.results.get(binding_id) {
                if result.status == CalculationStatus::Evaluated {
                    if let Some(value) = &result.value {
                        return Ok(CalculationValue::Text(value.clone()));
                    }
                }
            }
            return Err(CalculationFailure::DependencyUnavailable(
                binding_id.clone(),
            ));
        }

        document
            .cell_ref(reference)
            .map(|value| CalculationValue::Text(value.to_owned()))
            .ok_or(CalculationFailure::MissingCell(reference))
    }

    // ```text
    // 責務: [dependency_snapshots: bindingの直接dependencyを現在のraw/derived revisionと一緒に記録する]
    // 引数: [document: CSV reader, target_bindings: output owner index, id: 対象binding ID]
    // 戻り値: [Vec<CalculationDependencySnapshot>: stable row/column順のdependency記録]
    // ```
    fn dependency_snapshots(
        &self,
        document: &CsvDocument,
        target_bindings: &HashMap<CellRef, CalculationBindingId>,
        id: &CalculationBindingId,
    ) -> Vec<CalculationDependencySnapshot> {
        let dependencies = self
            .bindings
            .get(id)
            .map(|binding| binding.dependencies.clone())
            .unwrap_or_default();
        self.dependency_snapshots_for_cells(document, target_bindings, &dependencies)
    }

    // ```text
    // 責務: [dependency_snapshots_for_cells: 指定cell集合のraw値とbinding/result revisionをsnapshot化する]
    // 引数: [document: CSV reader, target_bindings: output owner index, dependencies: 直接参照cell集合]
    // 戻り値: [Vec<CalculationDependencySnapshot>: BTreeSet順のsnapshot]
    // ```
    fn dependency_snapshots_for_cells(
        &self,
        document: &CsvDocument,
        target_bindings: &HashMap<CellRef, CalculationBindingId>,
        dependencies: &BTreeSet<CellRef>,
    ) -> Vec<CalculationDependencySnapshot> {
        dependencies
            .iter()
            .map(|reference| {
                let binding_id = target_bindings.get(reference);
                let binding = binding_id.and_then(|id| self.bindings.get(id));
                let derived_result = binding_id.and_then(|id| self.results.get(id));
                CalculationDependencySnapshot {
                    cell: *reference,
                    raw_value: document.cell_ref(*reference).map(str::to_owned),
                    binding_revision: binding.map(|binding| binding.revision),
                    derived_revision: derived_result.map(|result| result.derived_revision),
                }
            })
            .collect()
    }

    // ```text
    // 責務: [store_result: 評価値、状態、binding revision、dependency snapshotを新result revisionで保存する]
    // 引数: [id: 対象binding, value: 評価済みtext, status: 評価状態, dependencies: 読み取ったsnapshot]
    // 戻り値: [Result<(), CalculationError>: result保存またはrevision overflow]
    // ```
    fn store_result(
        &mut self,
        id: CalculationBindingId,
        value: Option<String>,
        status: CalculationStatus,
        dependencies: Vec<CalculationDependencySnapshot>,
    ) -> Result<(), CalculationError> {
        let Some(binding) = self.bindings.get(&id) else {
            return Ok(());
        };
        let binding_revision = binding.revision;
        let derived_revision = self
            .derived_revision
            .checked_add(1)
            .ok_or(CalculationError::RevisionOverflow)?;
        self.results.insert(
            id,
            CalculationResult {
                value,
                status,
                binding_revision,
                derived_revision,
                dependencies,
            },
        );
        self.derived_revision = derived_revision;
        Ok(())
    }

    // ```text
    // 責務: [store_stale_result: 前回のderived valueを保持したまま新dependency snapshotでstale化する]
    // 引数: [id: 対象binding, dependencies: 変更後dependency snapshot]
    // 戻り値: [Result<(), CalculationError>: stale result保存またはrevision overflow]
    // ```
    fn store_stale_result(
        &mut self,
        id: CalculationBindingId,
        dependencies: Vec<CalculationDependencySnapshot>,
    ) -> Result<(), CalculationError> {
        let previous_value = self
            .results
            .get(&id)
            .and_then(|result| result.value.clone());
        self.store_result(id, previous_value, CalculationStatus::Stale, dependencies)
    }
}
