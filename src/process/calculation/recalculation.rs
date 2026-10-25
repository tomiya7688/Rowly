//! CalculationEngineの再計算とderived result保存を扱う。

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::engine::CalculationEngine;
use super::expression::evaluate_expression;
use super::types::{
    CalculationBindingId, CalculationDependencySnapshot, CalculationError, CalculationFailure,
    CalculationRecalculationReport, CalculationResult, CalculationStatus, CalculationTrigger,
    CalculationValue,
};
use crate::process::{CellRef, CsvDocument};

impl CalculationEngine {
    // ```text
    // 責務: [recalculate_selected: 選択ruleをtopological orderで処理してderived stateを更新する]
    // 処理: [cycleを検出し、triggerに従って式を評価またはstale化する]
    // 引数: [document: raw CSV reader, selected: 評価対象ID, force_all: Manual triggerも明示的に評価するか]
    // 戻り値: [Result<CalculationRecalculationReport, CalculationError>: 最終状態ごとのID一覧]
    // 補足: [計算式はCsvDocumentへ書き込まない]
    // ```
    pub(super) fn recalculate_selected(
        &mut self,
        document: &CsvDocument,
        selected: BTreeSet<CalculationBindingId>,
        force_all: bool,
    ) -> Result<CalculationRecalculationReport, CalculationError> {
        let mut report = CalculationRecalculationReport::default();
        if selected.is_empty() {
            return Ok(report);
        }

        let (evaluation_order, blocked_by_cycle) = self.evaluation_order(&selected);
        for id in blocked_by_cycle {
            let snapshots = self.dependency_snapshots(document, &id);
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
            if let Some(target_cell) = binding.target.single_cell()
                && document.cell_ref(target_cell).is_none()
            {
                let snapshots =
                    self.dependency_snapshots_for_cells(document, &binding.dependencies);
                self.store_result(
                    id.clone(),
                    None,
                    CalculationStatus::Error(CalculationFailure::MissingCell(target_cell)),
                    snapshots,
                )?;
                report.failed.push(id);
                continue;
            }
            if !force_all && binding.trigger == CalculationTrigger::Manual {
                let snapshots = self.dependency_snapshots(document, &id);
                self.store_stale_result(id.clone(), snapshots)?;
                report.stale.push(id);
                continue;
            }

            let target_bindings = &self.target_bindings;
            let results = &self.results;
            let evaluated = {
                let mut read_cell = |reference| {
                    Self::read_calculation_cell(document, target_bindings, results, reference)
                };
                evaluate_expression(&binding.expression, 0, &mut read_cell)
            };
            let snapshots = self.dependency_snapshots_for_cells(document, &binding.dependencies);
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
    // 責務: [read_calculation_cell: bound targetのfresh derived resultまたはraw CSV cellを読む]
    // 引数: [document: canonical CSV, target_bindings: output owner index, reference: 参照cell]
    // 戻り値: [Result<CalculationValue, CalculationFailure>: Text scalarまたは依存状態error]
    // ```
    fn read_calculation_cell(
        document: &CsvDocument,
        target_bindings: &HashMap<CellRef, CalculationBindingId>,
        results: &BTreeMap<CalculationBindingId, CalculationResult>,
        reference: CellRef,
    ) -> Result<CalculationValue, CalculationFailure> {
        if let Some(binding_id) = target_bindings.get(&reference) {
            if let Some(result) = results.get(binding_id) {
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
        id: &CalculationBindingId,
    ) -> Vec<CalculationDependencySnapshot> {
        let dependencies = self
            .bindings
            .get(id)
            .map(|binding| binding.dependencies.clone())
            .unwrap_or_default();
        self.dependency_snapshots_for_cells(document, &dependencies)
    }

    // ```text
    // 責務: [dependency_snapshots_for_cells: 指定cell集合のraw値とbinding/result revisionをsnapshot化する]
    // 引数: [document: CSV reader, target_bindings: output owner index, dependencies: 直接参照cell集合]
    // 戻り値: [Vec<CalculationDependencySnapshot>: BTreeSet順のsnapshot]
    // ```
    fn dependency_snapshots_for_cells(
        &self,
        document: &CsvDocument,
        dependencies: &BTreeSet<CellRef>,
    ) -> Vec<CalculationDependencySnapshot> {
        dependencies
            .iter()
            .map(|reference| {
                let binding_id = self.target_bindings.get(reference);
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
