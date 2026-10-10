use std::collections::BTreeSet;

use thiserror::Error;

use super::{
    CalculationBindingId, CalculationEngine, CalculationError, CalculationStatus,
};
use crate::process::{CellRef, CsvDocument, DocumentError};

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [CalculationMaterialization: 明示MaterializeでCSVへ反映したbinding、target、値、unbind有無を返す]
/// ```
pub struct CalculationMaterialization {
    pub binding_id: CalculationBindingId,
    pub target: CellRef,
    pub value: String,
    pub binding_removed: bool,
}

#[derive(Debug, Error)]
/// ```text
/// 責務: [CalculationMaterializationError: derived resultをCSVへ反映できない理由を分類する]
/// ```
pub enum CalculationMaterializationError {
    #[error("calculation binding `{0}` does not exist")]
    UnknownBinding(String),

    #[error("calculation binding `{0}` does not target one materializable cell")]
    UnsupportedTarget(String),

    #[error("calculation binding `{binding_id}` is not materializable: {status:?}")]
    ResultUnavailable {
        binding_id: String,
        status: CalculationStatus,
    },

    #[error("calculation binding `{0}` has no derived value")]
    MissingValue(String),

    #[error("calculation binding `{0}` has a stale dependency snapshot")]
    StaleResult(String),

    #[error(transparent)]
    Calculation(#[from] CalculationError),

    #[error(transparent)]
    Document(#[from] DocumentError),
}

impl CalculationEngine {
    /// ```text
    /// 責務: [materialize: freshなderived resultを通常のCSV cell editとして反映する]
    /// 引数: [document: canonical CSV document, id: materialize対象binding ID]
    /// 戻り値: [CalculationMaterialization: 書込targetと値。bindingは残す]
    /// エラー: [CalculationMaterializationError: result状態、dependency freshness、validation、document edit失敗]
    /// 副作用: [CsvDocumentの通常history/dirty stateを更新する。bindingは変更しない]
    /// ```
    pub fn materialize(
        &self,
        document: &mut CsvDocument,
        id: &CalculationBindingId,
    ) -> Result<CalculationMaterialization, CalculationMaterializationError> {
        let (target, value) = self.materializable_value(document, id)?;
        document.set_cell_ref(target, value.clone())?;

        Ok(CalculationMaterialization {
            binding_id: id.clone(),
            target,
            value,
            binding_removed: false,
        })
    }

    /// ```text
    /// 責務: [materialize_and_unbind: fresh resultをCSVへ反映し、そのbindingだけを明示削除する]
    /// 処理: [engine cloneでunbindを事前完了させ、CSV edit成功後にclone済みengineへ置換する]
    /// 引数: [document: canonical CSV document, id: materializeと削除の対象binding ID]
    /// 戻り値: [CalculationMaterialization: 書込結果とbinding削除済みフラグ]
    /// エラー: [CalculationMaterializationError: freshness、unbind準備、validation、document edit失敗]
    /// 補足: [document edit失敗時は元engineを保持し、unbind準備失敗時はCSVを書き換えない]
    /// ```
    pub fn materialize_and_unbind(
        &mut self,
        document: &mut CsvDocument,
        id: &CalculationBindingId,
    ) -> Result<CalculationMaterialization, CalculationMaterializationError> {
        let (target, value) = self.materializable_value(document, id)?;

        let mut updated_engine = self.clone();
        if !updated_engine.remove_binding(id)? {
            return Err(CalculationMaterializationError::UnknownBinding(
                id.as_str().to_owned(),
            ));
        }

        document.set_cell_ref(target, value.clone())?;
        *self = updated_engine;

        Ok(CalculationMaterialization {
            binding_id: id.clone(),
            target,
            value,
            binding_removed: true,
        })
    }

    // ```text
    // 責務: [materializable_value: target、result状態、transitive dependency freshnessを検証して書込候補を返す]
    // 引数: [document: 現在CSV, id: 対象binding]
    // 戻り値: [(CellRef, String): materialize可能なtarget/value]
    // ```
    fn materializable_value(
        &self,
        document: &CsvDocument,
        id: &CalculationBindingId,
    ) -> Result<(CellRef, String), CalculationMaterializationError> {
        let binding = self.bindings.get(id).ok_or_else(|| {
            CalculationMaterializationError::UnknownBinding(id.as_str().to_owned())
        })?;
        let target = binding.target.single_cell().ok_or_else(|| {
            CalculationMaterializationError::UnsupportedTarget(id.as_str().to_owned())
        })?;
        let result = self.results.get(id).ok_or_else(|| {
            CalculationMaterializationError::ResultUnavailable {
                binding_id: id.as_str().to_owned(),
                status: CalculationStatus::Pending,
            }
        })?;
        if result.status != CalculationStatus::Evaluated {
            return Err(CalculationMaterializationError::ResultUnavailable {
                binding_id: id.as_str().to_owned(),
                status: result.status.clone(),
            });
        }

        let mut visiting = BTreeSet::new();
        self.ensure_result_fresh(document, id, &mut visiting)?;

        let value = result
            .value
            .clone()
            .ok_or_else(|| CalculationMaterializationError::MissingValue(id.as_str().to_owned()))?;
        Ok((target, value))
    }

    // ```text
    // 責務: [ensure_result_fresh: evaluated resultが現在CSVと上流derived revisionsに一致するか再帰検証する]
    // 処理: [raw dependencyは値比較、bound dependencyはrevision比較と上流freshness検証を行う]
    // 引数: [document: 現在CSV, id: 検証binding, visiting: 防御的cycle検知集合]
    // 戻り値: [(): materialize可能なfresh resultの場合成功]
    // ```
    fn ensure_result_fresh(
        &self,
        document: &CsvDocument,
        id: &CalculationBindingId,
        visiting: &mut BTreeSet<CalculationBindingId>,
    ) -> Result<(), CalculationMaterializationError> {
        if !visiting.insert(id.clone()) {
            return Err(CalculationMaterializationError::StaleResult(
                id.as_str().to_owned(),
            ));
        }

        let checked = (|| {
            let binding = self.bindings.get(id).ok_or_else(|| {
                CalculationMaterializationError::UnknownBinding(id.as_str().to_owned())
            })?;
            let result = self.results.get(id).ok_or_else(|| {
                CalculationMaterializationError::StaleResult(id.as_str().to_owned())
            })?;
            if result.status != CalculationStatus::Evaluated
                || result.binding_revision != binding.revision
            {
                return Err(CalculationMaterializationError::StaleResult(
                    id.as_str().to_owned(),
                ));
            }

            for dependency in &result.dependencies {
                if let Some(dependency_id) = self.target_bindings.get(&dependency.cell) {
                    let dependency_binding =
                        self.bindings.get(dependency_id).ok_or_else(|| {
                            CalculationMaterializationError::StaleResult(id.as_str().to_owned())
                        })?;
                    let dependency_result = self.results.get(dependency_id).ok_or_else(|| {
                        CalculationMaterializationError::StaleResult(id.as_str().to_owned())
                    })?;
                    if dependency.binding_revision != Some(dependency_binding.revision)
                        || dependency.derived_revision != Some(dependency_result.derived_revision)
                    {
                        return Err(CalculationMaterializationError::StaleResult(
                            id.as_str().to_owned(),
                        ));
                    }
                    self.ensure_result_fresh(document, dependency_id, visiting)?;
                } else {
                    if dependency.binding_revision.is_some()
                        || dependency.derived_revision.is_some()
                        || document.cell_ref(dependency.cell).map(str::to_owned)
                            != dependency.raw_value
                    {
                        return Err(CalculationMaterializationError::StaleResult(
                            id.as_str().to_owned(),
                        ));
                    }
                }
            }

            Ok(())
        })();

        visiting.remove(id);
        checked
    }
}
