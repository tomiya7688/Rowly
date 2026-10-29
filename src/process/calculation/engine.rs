use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use super::expression::{MAX_EXPRESSION_DEPTH, expression_depth};
use super::types::{
    CalculationBinding, CalculationBindingId, CalculationError, CalculationExpression,
    CalculationRecalculationReport, CalculationResult, CalculationStatus, CalculationTarget,
    CalculationTrigger, CalculationValue,
};
use crate::process::{CellRef, CsvDocument};

/// ```text
/// 責務: [CalculationEngine: CSV外のcalculation bindingを評価しderived resultだけを保持する]
/// フィールド: [bindings: stable ID順のrule一覧, results: binding別derived state, revisions: ruleとresultのrevision counter]
/// 補足: [engineはCsvDocumentを書き換えず、依存変更後または明示Recalculate時に呼び出す]
/// ```
#[derive(Debug, Default, Clone)]
pub struct CalculationEngine {
    pub(super) bindings: BTreeMap<CalculationBindingId, CalculationBinding>,
    pub(super) results: BTreeMap<CalculationBindingId, CalculationResult>,
    pub(super) target_bindings: HashMap<CellRef, CalculationBindingId>,
    pub(super) dependents_by_cell: HashMap<CellRef, BTreeSet<CalculationBindingId>>,
    pub(super) binding_revision: u64,
    pub(super) derived_revision: u64,
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

        let target_is_owned = self
            .target_bindings
            .get(&target_cell)
            .is_some_and(|owner_id| owner_id != &id);
        if target_is_owned {
            return Err(CalculationError::DuplicateTarget(target_cell));
        }

        if let Some(existing) = self.bindings.get(&id)
            && existing.target == target
            && expressions_are_equivalent(&existing.expression, &expression)
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
        let previous_target = self
            .bindings
            .get(&id)
            .and_then(|binding| binding.target.single_cell());
        let mut changed_targets = BTreeSet::from([target_cell]);
        if let Some(previous_target) = previous_target {
            changed_targets.insert(previous_target);
        }
        let invalidated_dependents = self.dependent_closure(&changed_targets, &id);
        let invalidated_any_results = invalidated_dependents
            .iter()
            .any(|dependent_id| self.results.contains_key(dependent_id));
        let invalidation_revision = if invalidated_any_results {
            derived_revision
                .checked_add(1)
                .ok_or(CalculationError::RevisionOverflow)?
        } else {
            derived_revision
        };

        let previous_dependencies = self
            .bindings
            .get(&id)
            .map(|binding| binding.dependencies.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for dependency in previous_dependencies {
            self.remove_dependency_index(dependency, &id);
        }
        if let Some(previous_target) = previous_target
            && previous_target != target_cell
        {
            self.target_bindings.remove(&previous_target);
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

        for dependency in &binding.dependencies {
            self.dependents_by_cell
                .entry(*dependency)
                .or_default()
                .insert(id.clone());
        }
        self.target_bindings.insert(target_cell, id.clone());
        self.bindings.insert(id.clone(), binding);
        self.results.insert(id.clone(), pending_result);
        self.binding_revision = binding_revision;
        self.derived_revision = derived_revision;

        // 変更前後の出力cellから到達する古いresultをrevision付きで無効化する。
        self.mark_dependents_stale(invalidated_dependents, invalidation_revision);
        Ok(binding_revision)
    }

    /// ```text
    /// 責務: [remove_binding: bindingとそのderived resultをengineから削除する]
    /// 引数: [id: 削除対象stable ID]
    /// 戻り値: [Result<bool, CalculationError>: 削除結果またはrevision overflow]
    /// ```
    pub fn remove_binding(&mut self, id: &CalculationBindingId) -> Result<bool, CalculationError> {
        let Some(binding) = self.bindings.get(id) else {
            return Ok(false);
        };
        let target_cell = binding.target.single_cell();
        let invalidated_dependents = target_cell
            .map(|target_cell| self.dependent_closure(&BTreeSet::from([target_cell]), id))
            .unwrap_or_default();
        let invalidated_any_results = invalidated_dependents
            .iter()
            .any(|dependent_id| self.results.contains_key(dependent_id));
        let invalidation_revision = if invalidated_any_results {
            self.derived_revision
                .checked_add(1)
                .ok_or(CalculationError::RevisionOverflow)?
        } else {
            self.derived_revision
        };

        let Some(binding) = self.bindings.remove(id) else {
            return Ok(false);
        };
        self.results.remove(id);

        if let Some(target_cell) = target_cell {
            self.target_bindings.remove(&target_cell);
        }
        for dependency in binding.dependencies {
            self.remove_dependency_index(dependency, id);
        }
        self.mark_dependents_stale(invalidated_dependents, invalidation_revision);
        Ok(true)
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

        let mut selected = BTreeSet::new();
        let mut pending_cells = VecDeque::from_iter(changed_cells);
        while let Some(changed_cell) = pending_cells.pop_front() {
            if let Some(owner_id) = self.target_bindings.get(&changed_cell) {
                selected.insert(owner_id.clone());
            }
            if let Some(dependent_ids) = self.dependents_by_cell.get(&changed_cell) {
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
}

// {
//   責務: [expressions_are_equivalent: decimalの符号付きzeroを区別して式ASTが同じか判定する]
//   引数: [left: 比較する左式, right: 比較する右式]
//   戻り値: [bool: 更新不要なほど式が同じならtrue]
// }
fn expressions_are_equivalent(left: &CalculationExpression, right: &CalculationExpression) -> bool {
    match (left, right) {
        (CalculationExpression::Literal(left), CalculationExpression::Literal(right)) => {
            calculation_values_are_equivalent(left, right)
        }
        (CalculationExpression::Cell(left), CalculationExpression::Cell(right)) => left == right,
        (CalculationExpression::Negate(left), CalculationExpression::Negate(right)) => {
            expressions_are_equivalent(left, right)
        }
        (
            CalculationExpression::Arithmetic {
                left: left_left,
                operator: left_operator,
                right: left_right,
            },
            CalculationExpression::Arithmetic {
                left: right_left,
                operator: right_operator,
                right: right_right,
            },
        ) => {
            left_operator == right_operator
                && expressions_are_equivalent(left_left, right_left)
                && expressions_are_equivalent(left_right, right_right)
        }
        (
            CalculationExpression::Call {
                function: left_function,
                arguments: left_arguments,
            },
            CalculationExpression::Call {
                function: right_function,
                arguments: right_arguments,
            },
        ) => {
            left_function == right_function
                && left_arguments.len() == right_arguments.len()
                && left_arguments
                    .iter()
                    .zip(right_arguments)
                    .all(|(left, right)| expressions_are_equivalent(left, right))
        }
        _ => false,
    }
}

// {
//   責務: [calculation_values_are_equivalent: Decimal bit patternを含めてscalar literalの同一性を判定する]
//   引数: [left: 比較する左value, right: 比較する右value]
//   戻り値: [bool: 評価・表示上の差がなければtrue]
// }
fn calculation_values_are_equivalent(left: &CalculationValue, right: &CalculationValue) -> bool {
    match (left, right) {
        (CalculationValue::Text(left), CalculationValue::Text(right)) => left == right,
        (CalculationValue::Integer(left), CalculationValue::Integer(right)) => left == right,
        (CalculationValue::Decimal(left), CalculationValue::Decimal(right)) => {
            left.to_bits() == right.to_bits()
        }
        _ => false,
    }
}
