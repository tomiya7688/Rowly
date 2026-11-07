//! ```text
//! 責務: [
//! remove_duplicates: 指定したkey列のraw値が一致する行を、first/lastの方針で原子的に削除する明示データ変換を提供する
//! ]
//! 処理: [
//! 1: key列と残す方針から削除計画（DuplicatePlan）を作り、削除行をpreviewできる
//! 2: 計画をdocumentへ適用し、全行削除を1回のUndoにまとめる
//! ]
//! 補足: [
//! 比較はCSVのraw文字列の完全一致だけで行い、表示書式・型解釈・大文字小文字の違いは同一視しない
//! header行（0行目）は比較にも削除にも含めない
//! 1つのCsvDocumentは1つのphysical sourceなので、source間の行は比較しない。複数sourceを跨ぐ重複除去は別の明示policyが必要で、ここでは扱わない
//! ]
//! ```
use std::collections::HashMap;

use super::{CsvDocument, DocumentError};

/// ```text
/// 責務: [
/// KeepPolicy: 重複した行のうち、どれを残すかを表す
/// ]
/// フィールド: [
/// First: 最初に現れた行を残す
/// Last: 最後に現れた行を残す
/// ]
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeepPolicy {
    First,
    Last,
}

/// ```text
/// 責務: [
/// RemovedRow: 削除予定の1行と、その行と同じkeyで残る行を保持する
/// ]
/// フィールド: [
/// row: 削除する行番号（0始まり、headerは0行目）
/// kept_row: 同じkeyで残る行番号
/// cells: 計画時の行内容（古い計画の検出に使う）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemovedRow {
    pub row: usize,
    pub kept_row: usize,
    pub cells: Vec<String>,
}

/// ```text
/// 責務: [
/// DuplicatePlan: 重複除去で削除する行を保持し、適用前に確認できるようにする
/// ]
/// フィールド: [
/// key_columns: 比較に使う列
/// policy: 残す方針
/// scanned: 比較したデータ行数
/// removed: 削除予定の行（行番号の昇順）
/// ]
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DuplicatePlan {
    key_columns: Vec<usize>,
    policy: KeepPolicy,
    scanned: usize,
    removed: Vec<RemovedRow>,
}

impl DuplicatePlan {
    /// ```text
    /// 責務: [
    /// scanned: 比較したデータ行数を返す
    /// ]
    /// 引数: [
    /// self: 対象の計画
    /// ]
    /// 戻り値: [
    /// usize: 比較したデータ行数
    /// ]
    /// ```
    pub fn scanned(&self) -> usize {
        self.scanned
    }

    /// ```text
    /// 責務: [
    /// removed: 削除予定の行を行番号の昇順で返す
    /// ]
    /// 引数: [
    /// self: 対象の計画
    /// ]
    /// 戻り値: [
    /// &[RemovedRow]: 削除予定の行と、同じkeyで残る行
    /// ]
    /// ```
    pub fn removed(&self) -> &[RemovedRow] {
        &self.removed
    }

    /// ```text
    /// 責務: [
    /// removed_count: 削除予定の行数を返す
    /// ]
    /// 引数: [
    /// self: 対象の計画
    /// ]
    /// 戻り値: [
    /// usize: preview用の削除行数
    /// ]
    /// ```
    pub fn removed_count(&self) -> usize {
        self.removed.len()
    }
}

impl CsvDocument {
    /// ```text
    /// 責務: [
    /// plan_remove_duplicates: 重複除去で削除される行を、documentを変更せずに計算する
    /// ]
    /// 処理: [
    /// 1: key列が空でなく、すべてtable内にあるか確認する
    /// 2: データ行を上から走査し、keyのraw値が同じ行をまとめる
    /// 3: 残す方針に従って、各グループの残す行以外を削除予定にする
    /// ]
    /// 引数: [
    /// key_columns: 比較に使う列（1列以上。重複指定は1列として扱う）
    /// policy: 残す方針
    /// ]
    /// 戻り値: [
    /// DuplicatePlan: 比較件数と削除予定の行
    /// ]
    /// エラー: [
    /// DocumentError: key列が空、またはtable外の列を含む場合
    /// ]
    /// ```
    pub fn plan_remove_duplicates(
        &self,
        key_columns: &[usize],
        policy: KeepPolicy,
    ) -> Result<DuplicatePlan, DocumentError> {
        if key_columns.is_empty() {
            return Err(DocumentError::Edit(
                "at least one key column is required to remove duplicates".into(),
            ));
        }
        if let Some(column) = key_columns
            .iter()
            .find(|column| **column >= self.column_count())
        {
            return Err(DocumentError::Edit(format!(
                "column {column} is outside the existing CSV table"
            )));
        }
        let mut columns = key_columns.to_vec();
        columns.sort_unstable();
        columns.dedup();

        // keyごとに、現時点で残す行の番号を持つ。欠けたcellは空文字と区別するためNoneにする
        let mut kept: HashMap<Vec<Option<&str>>, usize> = HashMap::new();
        let mut removed = Vec::new();
        for row in 1..self.row_count() {
            let key: Vec<Option<&str>> = columns.iter().map(|c| self.cell(row, *c)).collect();
            match kept.get(&key).copied() {
                None => {
                    kept.insert(key, row);
                }
                Some(existing) => match policy {
                    KeepPolicy::First => removed.push(self.removed_row(row, existing)),
                    KeepPolicy::Last => {
                        removed.push(self.removed_row(existing, row));
                        kept.insert(key, row);
                    }
                },
            }
        }
        removed.sort_by_key(|entry| entry.row);
        Ok(DuplicatePlan {
            key_columns: columns,
            policy,
            scanned: self.row_count().saturating_sub(1),
            removed,
        })
    }

    // {
    // 責務: [
    // removed_row: 削除予定の行の内容を控えてRemovedRowを作る
    // ]
    // 引数: [
    // row: usize
    // kept_row: usize
    // ]
    // 戻り値: [
    // RemovedRow: 削除予定の行
    // ]
    // }
    fn removed_row(&self, row: usize, kept_row: usize) -> RemovedRow {
        RemovedRow {
            row,
            kept_row,
            cells: (0..self.column_count())
                .map(|column| self.cell(row, column).unwrap_or_default().to_owned())
                .collect(),
        }
    }

    /// ```text
    /// 責務: [
    /// apply_duplicate_plan: 重複除去の計画をdocumentへ原子的に適用し、1回のUndoにまとめる
    /// ]
    /// 処理: [
    /// 1: 計画と同じ条件で計画し直し、削除行と内容が一致するか確認する
    /// 2: transactionを開始し、行番号の大きい方から1行ずつ削除する
    /// 3: 途中で失敗したらrollbackして何も変えず、成功したらcommitする
    /// ]
    /// 引数: [
    /// plan: plan_remove_duplicatesで作った計画
    /// ]
    /// 戻り値: [
    /// usize: 削除した行数
    /// ]
    /// 副作用: [
    /// 削除があれば履歴1件とdirty状態が更新される。削除0件なら何も記録しない
    /// ]
    /// エラー: [
    /// DocumentError: 計画後にdocumentが変わった、transactionが既に開始されている、または削除に失敗した場合（何も変更しない）
    /// ]
    /// ```
    pub fn apply_duplicate_plan(&mut self, plan: &DuplicatePlan) -> Result<usize, DocumentError> {
        let current = self.plan_remove_duplicates(&plan.key_columns, plan.policy)?;
        if &current != plan {
            return Err(DocumentError::Edit(
                "rows changed after duplicates were planned; plan again".into(),
            ));
        }
        if plan.removed.is_empty() {
            return Ok(0);
        }
        self.begin_transaction()?;
        for entry in plan.removed.iter().rev() {
            if let Err(error) = self.delete_rows(entry.row, 1) {
                self.rollback_transaction()?;
                return Err(error);
            }
        }
        self.commit_transaction()?;
        Ok(plan.removed_count())
    }

    /// ```text
    /// 責務: [
    /// remove_duplicates: 計画と適用を続けて行い、削除した行数を返す
    /// ]
    /// 引数: [
    /// key_columns: 比較に使う列
    /// policy: 残す方針
    /// ]
    /// 戻り値: [
    /// usize: 削除した行数
    /// ]
    /// エラー: [
    /// DocumentError: key列が不正、または削除に失敗した場合（何も変更しない）
    /// ]
    /// ```
    pub fn remove_duplicates(
        &mut self,
        key_columns: &[usize],
        policy: KeepPolicy,
    ) -> Result<usize, DocumentError> {
        let plan = self.plan_remove_duplicates(key_columns, policy)?;
        self.apply_duplicate_plan(&plan)
    }
}

#[cfg(test)]
mod tests;
