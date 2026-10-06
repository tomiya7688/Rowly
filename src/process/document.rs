use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

use csv::{ReaderBuilder, Terminator, WriterBuilder};
use thiserror::Error;

use crate::data::{
    ContentFingerprint, SourceEncoding, Table, fingerprint_file, read_csv, write_csv_utf8,
    write_csv_utf8_if_unchanged,
};

use super::{
    CellRange, CellRef, ColumnError, ColumnType, ReferenceError, ValidationReport, ValidationRule,
    ValidationTarget, ValidationViolation,
    history::{CellChange, ColumnChange, EditCommand, EditHistory, EditOperation, RowEdit},
    metadata::{ColumnMetadata, sidecar_path},
};

#[derive(Debug)]
/// ```text
/// 責務: [
/// CsvDocument: CSV内容、保存基準、編集履歴、検証規則と外部変更状態を管理する
/// ]
/// フィールド: [
/// path: 操作対象CSVのパス
/// source_encoding: 現在のCSV文字コード
/// table: 編集中のtable
/// baseline: 最後に読み込んだ、または保存したtable
/// history: undo/redoと保存状態を管理する編集履歴
/// validation_rules: sessionの列検証規則
/// metadata: 列型宣言
/// metadata_error: メタデータ処理で発生したエラー
/// save_metadata_sidecar: sidecar保存の可否
/// disk_fingerprint: 最後に確認したディスク内容の指紋
/// external_conflicts: 自動統合できなかった外部変更
/// ]
/// ```
pub struct CsvDocument {
    path: PathBuf,
    source_encoding: SourceEncoding,
    table: Table,
    baseline: Table,
    history: EditHistory,
    validation_rules: BTreeMap<ValidationTarget, ValidationRule>,
    metadata: ColumnMetadata,
    metadata_error: Option<String>,
    save_metadata_sidecar: bool,
    disk_fingerprint: ContentFingerprint,
    external_conflicts: Vec<ExternalConflictDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ExternalConflictDraft: 自動マージできなかった変更の基準・ローカル・ディスクの各スナップショットを保持する
/// ]
/// フィールド: [
/// baseline: 外部編集前の共通内容
/// local: Rowly内の編集内容
/// disk: 外部編集後の内容
/// cell_conflicts: 値が衝突したcell一覧
/// structural_conflict: 構造上の競合理由
/// ]
/// 補足: [
/// 競合時はdisk snapshotが現在内容となり、安全に適用できたlocal cell変更も現在内容に残る
/// ]
/// ```
pub struct ExternalConflictDraft {
    pub baseline: Vec<Vec<String>>,
    pub local: Vec<Vec<String>>,
    pub disk: Vec<Vec<String>>,
    pub cell_conflicts: Vec<ExternalCellConflict>,
    pub structural_conflict: Option<ExternalStructureConflict>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ExternalCellConflict: 同じcellに対するローカルとディスクの競合値を保持する
/// ]
/// フィールド: [
/// row: 衝突cellのzero-based row index
/// column: 衝突cellのzero-based column index
/// baseline: 共通基準値
/// local: Rowly内の値
/// disk: ディスク上の値
/// ]
/// ```
pub struct ExternalCellConflict {
    pub row: usize,
    pub column: usize,
    pub baseline: String,
    pub local: String,
    pub disk: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// ```text
/// 責務: [
/// ExternalStructureConflict: 外部変更を安全に統合できない構造上の理由を表す
/// ]
/// ```
pub enum ExternalStructureConflict {
    RowCountChanged,
    RowShapeChanged,
    HeaderChanged,
    RowOrderChanged,
    DuplicateRowsAmbiguous,
}

impl CsvDocument {
    /// ```text
    /// 責務: [
    /// create: 指定行からUTF-8 CSVを作成し、保存済み状態のdocumentを返す
    /// ]
    /// 引数: [
    /// path: impl AsRef<Path>
    /// rows: Vec<Vec<String>>
    /// ]
    /// 戻り値: [
    /// Self: 処理成功時の結果
    /// ]
    /// エラー: [
    /// DocumentError: CSVファイルを作成・書き込みできない場合
    /// ]
    /// ```
    pub fn create(path: impl AsRef<Path>, rows: Vec<Vec<String>>) -> Result<Self, DocumentError> {
        let path = path.as_ref().to_path_buf();
        let table = Table::new(rows);
        let disk_fingerprint =
            write_csv_utf8(&path, &table).map_err(|error| DocumentError::Save {
                path: path.display().to_string(),
                message: error.to_string(),
            })?;

        let mut history = EditHistory::default();
        history.mark_saved();
        Ok(Self {
            path,
            source_encoding: SourceEncoding::Utf8,
            baseline: table.clone(),
            table,
            history,
            validation_rules: BTreeMap::new(),
            metadata: ColumnMetadata::default(),
            metadata_error: None,
            save_metadata_sidecar: true,
            disk_fingerprint,
            external_conflicts: Vec::new(),
        })
    }

    /// ```text
    /// 責務: [
    /// open: CSVと隣接するRowlyメタデータを読み込んで開く
    /// ]
    /// 引数: [
    /// path: impl AsRef<Path>
    /// ]
    /// 戻り値: [
    /// Self: 処理成功時の結果
    /// ]
    /// エラー: [
    /// DocumentError: CSVを読み込めない場合
    /// ]
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        Self::open_with_metadata(path.as_ref(), true)
    }

    // {
    // 責務: [
    // open_without_metadata: Rowlyメタデータを読み書きせずCSVを開く
    // ]
    // 引数: [
    // path: impl AsRef<Path>
    // ]
    // 戻り値: [
    // Self: 処理成功時の結果
    // ]
    // エラー: [
    // DocumentError: CSVを読み込めない場合
    // ]
    // }
    pub(crate) fn open_without_metadata(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        Self::open_with_metadata(path.as_ref(), false)
    }

    // {
    // 責務: [
    // open_with_metadata: CSVを読み込み、指定に応じて隣接メタデータも読み込む
    // ]
    // 引数: [
    // path: &Path
    // load_metadata: bool
    // ]
    // 戻り値: [
    // Self: 処理成功時の結果
    // ]
    // エラー: [
    // DocumentError: CSVを読み込めない場合
    // ]
    // }
    fn open_with_metadata(path: &Path, load_metadata: bool) -> Result<Self, DocumentError> {
        let path = path.to_path_buf();
        let loaded = read_csv(&path).map_err(|error| DocumentError::Open {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;

        let (metadata, metadata_error) = if load_metadata {
            match ColumnMetadata::load(&path) {
                Ok(metadata) => (metadata, None),
                Err(error) => (ColumnMetadata::default(), Some(error.to_string())),
            }
        } else {
            (ColumnMetadata::default(), None)
        };

        let baseline = loaded.table.clone();
        Ok(Self {
            path,
            source_encoding: loaded.encoding,
            table: loaded.table,
            baseline,
            history: EditHistory::default(),
            validation_rules: BTreeMap::new(),
            metadata,
            metadata_error,
            save_metadata_sidecar: load_metadata,
            disk_fingerprint: loaded.fingerprint,
            external_conflicts: Vec::new(),
        })
    }

    /// ```text
    /// 責務: [
    /// path: 操作対象CSVのパスを返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// &Path: 処理成功時の結果
    /// ]
    /// ```
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// ```text
    /// 責務: [
    /// source_encoding: CSVの現在の文字コードを返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// SourceEncoding: 処理成功時の結果
    /// ]
    /// ```
    pub fn source_encoding(&self) -> SourceEncoding {
        self.source_encoding
    }

    /// ```text
    /// 責務: [
    /// is_dirty: 保存後にCSV内容の編集があるか返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// ```
    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    /// ```text
    /// 責務: [
    /// refresh_if_external_change: ディスク上の変更を統合し処理結果を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// bool: ディスク内容が変化して同期処理を行った場合true
    /// ]
    /// エラー: [
    /// DocumentError: 編集中transactionがある、またはディスク確認・統合に失敗した場合
    /// ]
    /// 補足: [
    /// 安全な差分は統合し、競合はdraftに保存してdisk snapshotを現在内容にする
    /// ]
    /// ```
    pub fn refresh_if_external_change(&mut self) -> Result<bool, DocumentError> {
        self.ensure_no_transaction("refresh external changes")?;
        self.merge_external_disk()
    }

    /// ```text
    /// 責務: [
    /// external_conflict_drafts: 保留中の外部変更競合スナップショットを返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// &[ExternalConflictDraft]: 処理成功時の結果
    /// ]
    /// ```
    pub fn external_conflict_drafts(&self) -> &[ExternalConflictDraft] {
        &self.external_conflicts
    }

    /// ```text
    /// 責務: [
    /// reapply_local_cell_conflicts: 競合draftのローカルcell値を現在のtableへ再適用する
    /// ]
    /// 引数: [
    /// draft_index: usize
    /// ]
    /// 戻り値: [
    /// usize: 対象または適用件数
    /// ]
    /// エラー: [
    /// DocumentError: draftが存在しない、構造競合がある、cellが変化済み、または再適用値が検証規則に反する場合
    /// ]
    /// ```
    pub fn reapply_local_cell_conflicts(
        &mut self,
        draft_index: usize,
    ) -> Result<usize, DocumentError> {
        self.ensure_no_transaction("reapply conflict draft")?;
        let draft = self.external_conflicts.get(draft_index).ok_or_else(|| {
            DocumentError::ConflictDraft(format!("draft {draft_index} does not exist"))
        })?;
        if draft.structural_conflict.is_some() {
            return Err(DocumentError::ConflictDraft(
                "structural conflicts require manual review".into(),
            ));
        }

        let conflicts = draft.cell_conflicts.clone();
        let mut changes = Vec::new();
        for conflict in conflicts {
            let current = self
                .table
                .cell(conflict.row, conflict.column)
                .ok_or_else(|| {
                    DocumentError::ConflictDraft(format!(
                        "cell {},{} no longer exists",
                        conflict.row, conflict.column
                    ))
                })?;
            if current == conflict.local {
                continue;
            }
            if current != conflict.disk {
                return Err(DocumentError::ConflictDraft(format!(
                    "cell {},{} changed after the conflict was recorded",
                    conflict.row, conflict.column
                )));
            }
            changes.push((conflict.row, conflict.column, conflict.local));
        }

        if changes.is_empty() {
            return Ok(0);
        }
        self.begin_transaction()?;
        for (row, column, value) in &changes {
            if let Err(error) = self.set_cell(*row, *column, value.clone()) {
                let _ = self.rollback_transaction();
                return Err(error);
            }
        }
        self.commit_transaction()?;
        Ok(changes.len())
    }

    /// ```text
    /// 責務: [
    /// reapply_local_structural_draft: 競合draftのローカル構造全体を現在のtableへ再適用する
    /// ]
    /// 引数: [
    /// draft_index: usize
    /// ]
    /// 戻り値: [
    /// usize: 対象または適用件数
    /// ]
    /// エラー: [
    /// DocumentError: draftが存在しない、現在内容がdisk snapshotと異なる、またはtableを置換できない場合
    /// ]
    /// 補足: [
    /// 現在内容が記録済みdisk snapshotと一致する場合だけlocal snapshotを適用する
    /// ]
    /// ```
    pub fn reapply_local_structural_draft(
        &mut self,
        draft_index: usize,
    ) -> Result<usize, DocumentError> {
        self.ensure_no_transaction("reapply structural conflict draft")?;
        let draft = self.external_conflicts.get(draft_index).ok_or_else(|| {
            DocumentError::ConflictDraft(format!("draft {draft_index} does not exist"))
        })?;
        if draft.structural_conflict.is_none() {
            return Err(DocumentError::ConflictDraft(
                "draft does not contain a structural conflict".into(),
            ));
        }
        if self.table.rows() != draft.disk {
            return Err(DocumentError::ConflictDraft(
                "the current table changed after the structural conflict was recorded".into(),
            ));
        }

        let local = draft.local.clone();
        if local == self.table.rows() {
            return Ok(0);
        }
        self.begin_transaction()?;
        let removed = match self
            .table
            .replace_rows(0, self.table.row_count(), local.clone())
        {
            Ok(removed) => removed,
            Err(error) => {
                let _ = self.rollback_transaction();
                return Err(DocumentError::Edit(error.to_string()));
            }
        };
        self.history.record(EditOperation::Rows(RowEdit {
            index: 0,
            removed,
            inserted: local.clone(),
        }));
        self.commit_transaction()?;
        Ok(local.len())
    }

    /// ```text
    /// 責務: [
    /// metadata_path: CSVに隣接するメタデータファイルのパスを返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// PathBuf: 処理成功時の結果
    /// ]
    /// ```
    pub fn metadata_path(&self) -> PathBuf {
        sidecar_path(&self.path)
    }

    /// ```text
    /// 責務: [
    /// metadata_error: メタデータ処理時のエラーを返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// Option<&str>: 値が存在するときの結果
    /// ]
    /// ```
    pub fn metadata_error(&self) -> Option<&str> {
        self.metadata_error.as_deref()
    }

    /// ```text
    /// 責務: [
    /// set_column_type_declaration_by_header: header名に対応する列の型宣言を設定する
    /// ]
    /// 引数: [
    /// header: &str
    /// column_type: ColumnType
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: headerが存在しないか一意でない場合
    /// ]
    /// ```
    pub fn set_column_type_declaration_by_header(
        &mut self,
        header: &str,
        column_type: ColumnType,
    ) -> Result<(), DocumentError> {
        self.column_index_by_header(header)?;
        self.metadata.set(header.to_owned(), column_type);
        self.metadata_error = None;
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// remove_column_type_declaration_by_header: header名に対応する列の型宣言を削除する
    /// ]
    /// 引数: [
    /// header: &str
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// エラー: [
    /// DocumentError: headerが存在しないか一意でない場合
    /// ]
    /// ```
    pub fn remove_column_type_declaration_by_header(
        &mut self,
        header: &str,
    ) -> Result<bool, DocumentError> {
        self.column_index_by_header(header)?;
        Ok(self.metadata.remove(header))
    }

    /// ```text
    /// 責務: [
    /// column_type_declaration_by_header: header名に対応する列の型宣言を取得する
    /// ]
    /// 引数: [
    /// header: &str
    /// ]
    /// 戻り値: [
    /// Option<ColumnType>: 値が存在するときの結果
    /// ]
    /// エラー: [
    /// DocumentError: headerが存在しないか一意でない場合
    /// ]
    /// ```
    pub fn column_type_declaration_by_header(
        &self,
        header: &str,
    ) -> Result<Option<ColumnType>, DocumentError> {
        self.column_index_by_header(header)?;
        Ok(self.metadata.get(header))
    }

    /// ```text
    /// 責務: [
    /// column_type_declarations: 保存対象の列型宣言を列挙する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// impl Iterator<Item = (&str, ColumnType)>: 遅延評価される項目列
    /// ]
    /// ```
    pub fn column_type_declarations(&self) -> impl Iterator<Item = (&str, ColumnType)> {
        self.metadata.declarations()
    }

    /// ```text
    /// 責務: [
    /// save_metadata: 列型宣言をRowlyメタデータとして保存する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: メタデータsidecarを書き込めない場合
    /// ]
    /// ```
    pub fn save_metadata(&mut self) -> Result<(), DocumentError> {
        if !self.save_metadata_sidecar {
            return Ok(());
        }
        self.metadata
            .save(&self.path)
            .map_err(|error| DocumentError::Metadata(error.to_string()))?;
        self.metadata_error = None;
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// set_validation_rule: 列の入力検証規則を設定し編集履歴へ記録する
    /// ]
    /// 引数: [
    /// target: ValidationTarget
    /// rule: ValidationRule
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: target列を解決できない場合
    /// ]
    /// 補足: [
    /// session限定の規則。変更はundo可能だがCSVをdirtyにせず、CSV保存では永続化しない
    /// ]
    /// ```
    pub fn set_validation_rule(
        &mut self,
        target: ValidationTarget,
        rule: ValidationRule,
    ) -> Result<(), DocumentError> {
        let column = self.resolve_validation_target(&target)?;
        let before = self.validation_rules.clone();
        let replaced = self
            .validation_rules
            .keys()
            .filter_map(|existing| {
                (self.resolve_validation_target(existing).ok() == Some(column))
                    .then_some(existing.clone())
            })
            .collect::<Vec<_>>();
        for existing in replaced {
            self.validation_rules.remove(&existing);
        }
        self.validation_rules.insert(target, rule);
        let after = self.validation_rules.clone();
        self.history
            .record(EditOperation::ValidationRules { before, after });
        Ok(())
    }

    // {
    // 責務: [
    // replace_project_validation_rules: project設定由来の検証規則一式を置き換える
    // ]
    // 引数: [
    // rules: BTreeMap<ValidationTarget, ValidationRule>
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // 補足: [
    // 変更はundo可能な履歴に記録されるがCSV dirty状態には影響しない
    // ]
    // }
    pub(crate) fn replace_project_validation_rules(
        &mut self,
        rules: BTreeMap<ValidationTarget, ValidationRule>,
    ) {
        let before = self.validation_rules.clone();
        if before == rules {
            return;
        }
        self.validation_rules = rules;
        self.history.record(EditOperation::ValidationRules {
            before,
            after: self.validation_rules.clone(),
        });
    }

    /// ```text
    /// 責務: [
    /// remove_validation_rule: 列に紐づく入力検証規則を削除する
    /// ]
    /// 引数: [
    /// target: &ValidationTarget
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// ```
    pub fn remove_validation_rule(
        &mut self,
        target: &ValidationTarget,
    ) -> Result<bool, DocumentError> {
        let before = self.validation_rules.clone();
        if let Ok(column) = self.resolve_validation_target(target) {
            let matching = self
                .validation_rules
                .keys()
                .filter_map(|existing| {
                    (self.resolve_validation_target(existing).ok() == Some(column))
                        .then_some(existing.clone())
                })
                .collect::<Vec<_>>();
            for existing in matching {
                self.validation_rules.remove(&existing);
            }
        } else {
            self.validation_rules.remove(target);
        }
        let after = self.validation_rules.clone();
        if before == after {
            return Ok(false);
        }
        self.history
            .record(EditOperation::ValidationRules { before, after });
        Ok(true)
    }

    /// ```text
    /// 責務: [
    /// validation_rule: 指定targetの入力検証規則を取得する
    /// ]
    /// 引数: [
    /// target: &ValidationTarget
    /// ]
    /// 戻り値: [
    /// Option<&ValidationRule>: 値が存在するときの結果
    /// ]
    /// ```
    pub fn validation_rule(&self, target: &ValidationTarget) -> Option<&ValidationRule> {
        self.validation_rules.get(target)
    }

    /// ```text
    /// 責務: [
    /// validation_rules: 設定済み入力検証規則を列挙する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// impl Iterator<Item = (&ValidationTarget, &ValidationRule)>: 遅延評価される項目列
    /// ]
    /// ```
    pub fn validation_rules(&self) -> impl Iterator<Item = (&ValidationTarget, &ValidationRule)> {
        self.validation_rules.iter()
    }

    /// ```text
    /// 責務: [
    /// allowed_values_for_column: 列に有効な規則が持つ許可値を取得する
    /// ]
    /// 引数: [
    /// column: usize
    /// ]
    /// 戻り値: [
    /// Option<&[String]>: 値が存在するときの結果
    /// ]
    /// ```
    pub fn allowed_values_for_column(&self, column: usize) -> Option<&[String]> {
        self.validation_rules.iter().find_map(|(target, rule)| {
            (self.resolve_validation_target(target).ok() == Some(column))
                .then(|| rule.allowed_values())
                .flatten()
        })
    }

    /// ```text
    /// 責務: [
    /// validation_report: 現在のtableに対する入力検証結果を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// ValidationReport: 処理成功時の結果
    /// ]
    /// ```
    pub fn validation_report(&self) -> ValidationReport {
        self.validation_report_for(&self.table)
    }

    // {
    // 責務: [
    // validation_report_for: 指定tableのデータ行を検証規則に照らして検査する
    // ]
    // 引数: [
    // table: &Table
    // ]
    // 戻り値: [
    // ValidationReport: 処理成功時の結果
    // ]
    // }
    fn validation_report_for(&self, table: &Table) -> ValidationReport {
        let mut report = ValidationReport::default();
        let mut rules_by_column =
            BTreeMap::<usize, Vec<(&ValidationTarget, &ValidationRule)>>::new();
        for (target, rule) in &self.validation_rules {
            if let Ok(column) = self.resolve_validation_target(target) {
                rules_by_column
                    .entry(column)
                    .or_default()
                    .push((target, rule));
            }
        }

        for row in 1..table.row_count() {
            for column in 0..table.column_count() {
                let Some(rules) = rules_by_column.get(&column) else {
                    continue;
                };
                let Some(value) = table.cell(row, column) else {
                    continue;
                };
                report.checked_cells += 1;
                for (target, rule) in rules {
                    if !rule.matches(value) {
                        report.violations.push(ValidationViolation {
                            cell: CellRef::new(row, column),
                            target: (*target).clone(),
                            value: value.to_owned(),
                            rule: (*rule).clone(),
                        });
                    }
                }
            }
        }
        report
    }

    /// ```text
    /// 責務: [
    /// can_undo: 取り消し可能な編集履歴があるか返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// ```
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// ```text
    /// 責務: [
    /// can_redo: やり直し可能な編集履歴があるか返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// ```
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// ```text
    /// 責務: [
    /// row_count: 現在のtableの行数を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// usize: 対象または適用件数
    /// ]
    /// ```
    pub fn row_count(&self) -> usize {
        self.table.row_count()
    }

    /// ```text
    /// 責務: [
    /// column_count: 現在のtableの最大列幅を返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// usize: 対象または適用件数
    /// ]
    /// ```
    pub fn column_count(&self) -> usize {
        self.table.column_count()
    }

    /// ```text
    /// 責務: [
    /// rows: 現在の行を順序を保って参照列挙する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// impl Iterator<Item = &[String]>: 遅延評価される項目列
    /// ]
    /// ```
    pub fn rows(&self) -> impl Iterator<Item = &[String]> {
        self.table.rows().iter().map(Vec::as_slice)
    }

    /// ```text
    /// 責務: [
    /// cell: 行列番号で指定されたcell値を取得する
    /// ]
    /// 引数: [
    /// row: usize
    /// column: usize
    /// ]
    /// 戻り値: [
    /// Option<&str>: 値が存在するときの結果
    /// ]
    /// ```
    pub fn cell(&self, row: usize, column: usize) -> Option<&str> {
        self.table.cell(row, column)
    }

    /// ```text
    /// 責務: [
    /// cell_ref: cell参照で指定された値を取得する
    /// ]
    /// 引数: [
    /// reference: CellRef
    /// ]
    /// 戻り値: [
    /// Option<&str>: 値が存在するときの結果
    /// ]
    /// ```
    pub fn cell_ref(&self, reference: CellRef) -> Option<&str> {
        self.cell(reference.row(), reference.column())
    }

    /// ```text
    /// 責務: [
    /// cell_a1: A1形式の参照からcell値を取得する
    /// ]
    /// 引数: [
    /// reference: &str
    /// ]
    /// 戻り値: [
    /// Option<&str>: 値が存在するときの結果
    /// ]
    /// エラー: [
    /// DocumentError: A1参照の形式が不正な場合
    /// ]
    /// ```
    pub fn cell_a1(&self, reference: &str) -> Result<Option<&str>, DocumentError> {
        Ok(self.cell_ref(reference.parse()?))
    }

    /// ```text
    /// 責務: [
    /// csv_text: 現在のtableをUTF-8のCSV文字列へ直列化する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// String: 処理成功時の結果
    /// ]
    /// エラー: [
    /// DocumentError: CSV文字列へ直列化できない場合
    /// ]
    /// ```
    pub fn csv_text(&self) -> Result<String, DocumentError> {
        let mut writer = WriterBuilder::new()
            .terminator(Terminator::Any(b'\n'))
            .from_writer(Vec::new());
        for row in self.table.rows() {
            writer
                .write_record(row)
                .map_err(|error| DocumentError::Csv(error.to_string()))?;
        }
        let bytes = writer
            .into_inner()
            .map_err(|error| DocumentError::Csv(error.error().to_string()))?;
        String::from_utf8(bytes).map_err(|error| DocumentError::Csv(error.to_string()))
    }

    /// ```text
    /// 責務: [
    /// apply_csv_text: CSV文字列を解析し検証後に一括適用する
    /// ]
    /// 引数: [
    /// text: &str
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: CSV文字列を解析できないか、値が検証規則に反する場合
    /// ]
    /// ```
    pub fn apply_csv_text(&mut self, text: &str) -> Result<(), DocumentError> {
        let mut reader = ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|error| DocumentError::Csv(error.to_string()))?;
            rows.push(record.iter().map(str::to_owned).collect());
        }
        self.replace_contents(rows)
    }

    /// ```text
    /// 責務: [
    /// replace_contents: table全体を検証して置き換え変更を履歴へ記録する
    /// ]
    /// 引数: [
    /// rows: Vec<Vec<String>>
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: table化または検証規則の確認に失敗した場合
    /// ]
    /// ```
    pub fn replace_contents(&mut self, rows: Vec<Vec<String>>) -> Result<(), DocumentError> {
        let replacement = Table::new(rows);
        let before = self.table.rows().to_vec();
        let after = replacement.rows().to_vec();
        if before == after {
            return Ok(());
        }

        let report = self.validation_report_for(&replacement);
        if !report.is_valid() {
            return Err(DocumentError::ValidationRejected {
                violations: report.violations,
            });
        }

        self.table = replacement;
        self.history
            .record(EditOperation::Contents { before, after });
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// set_cell: 行列番号で指定したcell値を編集する
    /// ]
    /// 引数: [
    /// row: usize
    /// column: usize
    /// value: impl Into<String>
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: cellが存在しないか、新しい値が検証規則に反する場合
    /// ]
    /// ```
    pub fn set_cell(
        &mut self,
        row: usize,
        column: usize,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_cell_ref(CellRef::new(row, column), value)
    }

    /// ```text
    /// 責務: [
    /// set_cell_ref: cell参照で指定した値を編集する
    /// ]
    /// 引数: [
    /// reference: CellRef
    /// value: impl Into<String>
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: cellが存在しないか、新しい値が検証規則に反する場合
    /// ]
    /// ```
    pub fn set_cell_ref(
        &mut self,
        reference: CellRef,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_references_value([reference], value.into())
    }

    /// ```text
    /// 責務: [
    /// set_cell_a1: A1形式の参照で指定した値を編集する
    /// ]
    /// 引数: [
    /// reference: &str
    /// value: impl Into<String>
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 参照が不正、cellが存在しない、または値が検証規則に反する場合
    /// ]
    /// ```
    pub fn set_cell_a1(
        &mut self,
        reference: &str,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_cell_ref(reference.parse()?, value)
    }

    /// ```text
    /// 責務: [
    /// set_range_value: cell範囲の値を一括編集する
    /// ]
    /// 引数: [
    /// range: CellRange
    /// value: impl Into<String>
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 存在しないcellがあるか、新しい値が検証規則に反する場合
    /// ]
    /// ```
    pub fn set_range_value(
        &mut self,
        range: CellRange,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_references_value(range.iter(), value.into())
    }

    /// ```text
    /// 責務: [
    /// set_range_a1: A1形式の範囲の値を一括編集する
    /// ]
    /// 引数: [
    /// range: &str
    /// value: impl Into<String>
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 範囲参照が不正、cellが存在しない、または値が検証規則に反する場合
    /// ]
    /// ```
    pub fn set_range_a1(
        &mut self,
        range: &str,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        self.set_range_value(range.parse()?, value)
    }

    /// ```text
    /// 責務: [
    /// insert_rows: 指定位置へ空行を挿入し編集履歴へ記録する
    /// ]
    /// 引数: [
    /// index: usize
    /// count: usize
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 挿入位置がtableの範囲外の場合
    /// ]
    /// ```
    pub fn insert_rows(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let width = self.column_count().max(1);
        let inserted = vec![vec![String::new(); width]; count];
        let removed = self
            .table
            .replace_rows(index, 0, inserted.clone())
            .map_err(|error| DocumentError::Edit(error.to_string()))?;

        self.history.record(EditOperation::Rows(RowEdit {
            index,
            removed,
            inserted,
        }));
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// delete_rows: 指定位置の行を削除し編集履歴へ記録する
    /// ]
    /// 引数: [
    /// index: usize
    /// count: usize
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 削除範囲がtableの範囲外の場合
    /// ]
    /// ```
    pub fn delete_rows(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let removed = self
            .table
            .replace_rows(index, count, Vec::new())
            .map_err(|error| DocumentError::Edit(error.to_string()))?;

        self.history.record(EditOperation::Rows(RowEdit {
            index,
            removed,
            inserted: Vec::new(),
        }));
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// insert_columns: 指定位置へ列を挿入し検証targetと履歴を更新する
    /// ]
    /// 引数: [
    /// index: usize
    /// count: usize
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 挿入位置が列範囲外か、検証target indexがoverflowする場合
    /// ]
    /// ```
    pub fn insert_columns(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let column_count = self.column_count();
        if index > column_count {
            return Err(DocumentError::Edit(format!(
                "column insertion index {index} is out of bounds for {column_count} columns"
            )));
        }

        let before_validation = self.validation_rules.clone();
        let shifted_validation = self.shift_validation_rules_for_insert(index, count)?;
        let inserted = vec![String::new(); count];
        let changes = self
            .table
            .rows()
            .iter()
            .enumerate()
            .filter(|(_, row)| index <= row.len())
            .map(|(row, _)| ColumnChange {
                row,
                index,
                removed: Vec::new(),
                inserted: inserted.clone(),
            })
            .collect::<Vec<_>>();

        for change in &changes {
            self.table
                .replace_row_segment(change.row, change.index, 0, &change.inserted)
                .map_err(|error| DocumentError::Edit(error.to_string()))?;
        }

        self.validation_rules = shifted_validation;
        self.record_column_edit(changes, before_validation);
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// begin_transaction: 編集transactionを開始する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: すでにtransactionが開始されている場合
    /// ]
    /// ```
    pub fn begin_transaction(&mut self) -> Result<(), DocumentError> {
        if !self.history.begin_transaction() {
            return Err(DocumentError::Transaction(
                "a transaction is already active".into(),
            ));
        }
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// commit_transaction: transactionを一つの履歴操作として確定する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: active transactionがない場合
    /// ]
    /// ```
    pub fn commit_transaction(&mut self) -> Result<(), DocumentError> {
        if !self.history.commit_transaction() {
            return Err(DocumentError::Transaction(
                "no transaction is active".into(),
            ));
        }
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// rollback_transaction: 編集中のtransactionを逆順に適用して取り消す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: active transactionがないか、履歴操作に失敗した場合
    /// ]
    /// ```
    pub fn rollback_transaction(&mut self) -> Result<(), DocumentError> {
        let Some(operations) = self.history.take_transaction() else {
            return Err(DocumentError::Transaction(
                "no transaction is active".into(),
            ));
        };

        for operation in operations.iter().rev() {
            if let Err(error) = self.apply_operation(operation, CommandDirection::Undo) {
                self.history.restore_transaction(operations);
                return Err(error);
            }
        }
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// transaction_active: 編集中のtransactionがあるか返す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// ```
    pub fn transaction_active(&self) -> bool {
        self.history.transaction_active()
    }

    /// ```text
    /// 責務: [
    /// delete_columns: 指定範囲の列を削除し検証targetと履歴を更新する
    /// ]
    /// 引数: [
    /// index: usize
    /// count: usize
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: 列範囲がtableの範囲外か、範囲計算がoverflowする場合
    /// ]
    /// ```
    pub fn delete_columns(&mut self, index: usize, count: usize) -> Result<(), DocumentError> {
        let column_count = self.column_count();
        let end = index
            .checked_add(count)
            .ok_or_else(|| DocumentError::Edit("column range arithmetic overflowed".into()))?;
        if index > column_count || end > column_count {
            return Err(DocumentError::Edit(format!(
                "column range starting at {index} with length {count} exceeds {column_count} columns"
            )));
        }

        let before_validation = self.validation_rules.clone();
        let shifted_validation = self.shift_validation_rules_for_delete(index, end, count);
        let changes = self
            .table
            .rows()
            .iter()
            .enumerate()
            .filter_map(|(row_index, row)| {
                if index >= row.len() {
                    return None;
                }

                let row_end = end.min(row.len());
                Some(ColumnChange {
                    row: row_index,
                    index,
                    removed: row[index..row_end].to_vec(),
                    inserted: Vec::new(),
                })
            })
            .collect::<Vec<_>>();

        for change in &changes {
            self.table
                .replace_row_segment(change.row, change.index, change.removed.len(), &[])
                .map_err(|error| DocumentError::Edit(error.to_string()))?;
        }

        self.validation_rules = shifted_validation;
        self.record_column_edit(changes, before_validation);
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// undo: 直前の編集操作を取り消す
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// エラー: [
    /// DocumentError: transaction中、または履歴が現在tableと一致しない場合
    /// ]
    /// ```
    pub fn undo(&mut self) -> Result<bool, DocumentError> {
        self.ensure_no_transaction("undo")?;
        let Some(command) = self.history.take_undo() else {
            return Ok(false);
        };

        if let Err(error) = self.apply_command(&command, CommandDirection::Undo) {
            self.history.restore_undo(command);
            return Err(error);
        }

        self.history.commit_undo(command);
        Ok(true)
    }

    /// ```text
    /// 責務: [
    /// redo: 直前に取り消した編集操作を再適用する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// bool: 判定または変更結果
    /// ]
    /// エラー: [
    /// DocumentError: transaction中、または履歴が現在tableと一致しない場合
    /// ]
    /// ```
    pub fn redo(&mut self) -> Result<bool, DocumentError> {
        self.ensure_no_transaction("redo")?;
        let Some(command) = self.history.take_redo() else {
            return Ok(false);
        };

        if let Err(error) = self.apply_command(&command, CommandDirection::Redo) {
            self.history.restore_redo(command);
            return Err(error);
        }

        self.history.commit_redo(command);
        Ok(true)
    }

    /// ```text
    /// 責務: [
    /// save: 外部変更を確認し現在のCSVをUTF-8で保存する
    /// ]
    /// 引数: [
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: transaction中、外部変更の確認・統合に失敗、またはCSVを書き込めない場合
    /// ]
    /// 補足: [
    /// 成功時に文字コード・baseline・編集履歴の保存状態を更新する
    /// ]
    /// ```
    pub fn save(&mut self) -> Result<(), DocumentError> {
        self.ensure_no_transaction("save")?;
        let current =
            fingerprint_file(&self.path).map_err(|error| DocumentError::ExternalCheck {
                path: self.path.display().to_string(),
                message: error.to_string(),
            })?;
        if current != self.disk_fingerprint {
            self.merge_external_disk()?;
            if !self.is_dirty() && self.source_encoding == SourceEncoding::Utf8 {
                return Ok(());
            }
        }
        if !self.is_dirty() && self.source_encoding == SourceEncoding::Utf8 {
            return Ok(());
        }
        self.disk_fingerprint =
            match write_csv_utf8_if_unchanged(&self.path, &self.table, Some(self.disk_fingerprint))
            {
                Ok(fingerprint) => fingerprint,
                Err(crate::data::CsvIoError::ExternalModification) => {
                    self.merge_external_disk()?;
                    if self.is_dirty() {
                        return Err(DocumentError::ExternalModification {
                            path: self.path.display().to_string(),
                        });
                    }
                    return Ok(());
                }
                Err(error) => {
                    return Err(DocumentError::Save {
                        path: self.path.display().to_string(),
                        message: error.to_string(),
                    });
                }
            };

        self.source_encoding = SourceEncoding::Utf8;
        self.baseline = self.table.clone();
        self.history.mark_saved();
        Ok(())
    }

    /// ```text
    /// 責務: [
    /// save_as: 現在のCSVを指定先へUTF-8で保存する
    /// ]
    /// 引数: [
    /// path: impl AsRef<Path>
    /// ]
    /// 戻り値: [
    /// (): 成功時に値を返さない
    /// ]
    /// エラー: [
    /// DocumentError: transaction中、または指定先へCSVを書き込めない場合
    /// ]
    /// 補足: [
    /// 成功時に操作対象pathとbaselineを更新する
    /// ]
    /// ```
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        self.ensure_no_transaction("save_as")?;
        let path = path.as_ref().to_path_buf();
        self.disk_fingerprint =
            write_csv_utf8(&path, &self.table).map_err(|error| DocumentError::Save {
                path: path.display().to_string(),
                message: error.to_string(),
            })?;

        self.path = path;
        self.source_encoding = SourceEncoding::Utf8;
        self.baseline = self.table.clone();
        self.history.mark_saved();
        Ok(())
    }

    // {
    // 責務: [
    // merge_external_disk: 基準・ローカル・ディスクの差分を安全性に応じて統合する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // bool: 判定または変更結果
    // ]
    // エラー: [
    // DocumentError: ディスクCSVを読み込めないか、差分の統合・編集履歴更新に失敗した場合
    // ]
    // 補足: [
    // 基準・ローカル・ディスクの3状態を比較し、競合draftを保持する場合がある
    // ]
    // }
    fn merge_external_disk(&mut self) -> Result<bool, DocumentError> {
        let loaded = read_csv(&self.path).map_err(|error| DocumentError::ExternalCheck {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })?;
        if loaded.fingerprint == self.disk_fingerprint {
            return Ok(false);
        }

        let baseline = self.baseline.rows().to_vec();
        let local = self.table.rows().to_vec();
        let disk = loaded.table.rows().to_vec();

        if local == baseline {
            self.install_disk_snapshot(loaded);
            return Ok(true);
        }
        if local == disk {
            self.install_disk_snapshot(loaded);
            return Ok(true);
        }
        if disk == baseline {
            self.disk_fingerprint = loaded.fingerprint;
            self.source_encoding = loaded.encoding;
            self.baseline = loaded.table;
            return Ok(true);
        }

        match merge_row_only_changes(&baseline, &local, &disk) {
            Ok(merged) => {
                self.install_disk_snapshot(loaded);
                if merged != self.table.rows() {
                    self.begin_transaction()?;
                    let removed = self
                        .table
                        .replace_rows(
                            1,
                            self.table.row_count().saturating_sub(1),
                            merged[1..].to_vec(),
                        )
                        .map_err(|error| DocumentError::Edit(error.to_string()))?;
                    self.history.record(EditOperation::Rows(RowEdit {
                        index: 1,
                        removed,
                        inserted: merged[1..].to_vec(),
                    }));
                    self.commit_transaction()?;
                }
                return Ok(true);
            }
            Err(reason) if baseline.len() != local.len() || baseline.len() != disk.len() => {
                self.external_conflicts.push(ExternalConflictDraft {
                    baseline,
                    local,
                    disk,
                    cell_conflicts: Vec::new(),
                    structural_conflict: Some(reason),
                });
                self.install_disk_snapshot(loaded);
                return Ok(true);
            }
            Err(_) => {}
        }

        if let Some(reason) = structural_conflict(&baseline, &local, &disk) {
            self.external_conflicts.push(ExternalConflictDraft {
                baseline,
                local,
                disk,
                cell_conflicts: Vec::new(),
                structural_conflict: Some(reason),
            });
            self.install_disk_snapshot(loaded);
            return Ok(true);
        }

        let mut safe_local_changes = Vec::new();
        let mut conflicts = Vec::new();
        for row in 0..baseline.len() {
            for column in 0..baseline[row].len() {
                let before = &baseline[row][column];
                let local_value = &local[row][column];
                let disk_value = &disk[row][column];
                if local_value == before || local_value == disk_value {
                    continue;
                }
                if disk_value == before {
                    safe_local_changes.push((row, column, local_value.clone()));
                } else {
                    conflicts.push(ExternalCellConflict {
                        row,
                        column,
                        baseline: before.clone(),
                        local: local_value.clone(),
                        disk: disk_value.clone(),
                    });
                }
            }
        }

        if !conflicts.is_empty() {
            self.external_conflicts.push(ExternalConflictDraft {
                baseline,
                local,
                disk,
                cell_conflicts: conflicts,
                structural_conflict: None,
            });
        }

        self.install_disk_snapshot(loaded);
        if !safe_local_changes.is_empty() {
            self.begin_transaction()?;
            for (row, column, value) in safe_local_changes {
                if let Err(error) = self.set_cell(row, column, value) {
                    let _ = self.rollback_transaction();
                    return Err(error);
                }
            }
            self.commit_transaction()?;
        }
        Ok(true)
    }

    // {
    // 責務: [
    // install_disk_snapshot: ディスク状態を現在内容と保存基準に設定する
    // ]
    // 引数: [
    // loaded: crate::data::LoadedCsv
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn install_disk_snapshot(&mut self, loaded: crate::data::LoadedCsv) {
        self.table = loaded.table;
        self.baseline = self.table.clone();
        self.source_encoding = loaded.encoding;
        self.disk_fingerprint = loaded.fingerprint;
        self.history = EditHistory::default();
        self.history.mark_saved();
    }

    // {
    // 責務: [
    // resolve_validation_target: 検証targetを現行tableの列番号へ解決する
    // ]
    // 引数: [
    // target: &ValidationTarget
    // ]
    // 戻り値: [
    // usize: 対象または適用件数
    // ]
    // エラー: [
    // ColumnError: 列indexが範囲外か、headerが存在しないか一意でない場合
    // ]
    // }
    fn resolve_validation_target(&self, target: &ValidationTarget) -> Result<usize, ColumnError> {
        match target {
            ValidationTarget::Index(column) if *column < self.column_count() => Ok(*column),
            ValidationTarget::Index(column) => Err(ColumnError::ColumnOutOfBounds {
                column: *column,
                column_count: self.column_count(),
            }),
            ValidationTarget::Header(header) => self.column_index_by_header(header),
        }
    }

    // {
    // 責務: [
    // shift_validation_rules_for_insert: 列挿入に合わせてindex指定の検証targetを移動する
    // ]
    // 引数: [
    // index: usize
    // count: usize
    // ]
    // 戻り値: [
    // BTreeMap<ValidationTarget, ValidationRule>: 処理成功時の結果
    // ]
    // エラー: [
    // DocumentError: 移動先の列indexがoverflowする場合
    // ]
    // }
    fn shift_validation_rules_for_insert(
        &self,
        index: usize,
        count: usize,
    ) -> Result<BTreeMap<ValidationTarget, ValidationRule>, DocumentError> {
        self.validation_rules
            .iter()
            .map(|(target, rule)| {
                let target = match target {
                    ValidationTarget::Index(column) if *column >= index => {
                        ValidationTarget::Index(column.checked_add(count).ok_or_else(|| {
                            DocumentError::Edit("validation column index overflowed".into())
                        })?)
                    }
                    _ => target.clone(),
                };
                Ok((target, rule.clone()))
            })
            .collect()
    }

    // {
    // 責務: [
    // shift_validation_rules_for_delete: 列削除に合わせて検証targetを削除または移動する
    // ]
    // 引数: [
    // index: usize
    // end: usize
    // count: usize
    // ]
    // 戻り値: [
    // BTreeMap<ValidationTarget, ValidationRule>: 処理成功時の結果
    // ]
    // }
    fn shift_validation_rules_for_delete(
        &self,
        index: usize,
        end: usize,
        count: usize,
    ) -> BTreeMap<ValidationTarget, ValidationRule> {
        self.validation_rules
            .iter()
            .filter_map(|(target, rule)| {
                let target = match target {
                    ValidationTarget::Index(column) if *column >= index && *column < end => {
                        return None;
                    }
                    ValidationTarget::Index(column) if *column >= end => {
                        ValidationTarget::Index(column - count)
                    }
                    _ => target.clone(),
                };
                Some((target, rule.clone()))
            })
            .collect()
    }

    // {
    // 責務: [
    // record_column_edit: 列編集と検証規則の変更を履歴へまとめて記録する
    // ]
    // 引数: [
    // columns: Vec<ColumnChange>
    // before_validation: BTreeMap<ValidationTarget, ValidationRule>
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn record_column_edit(
        &mut self,
        columns: Vec<ColumnChange>,
        before_validation: BTreeMap<ValidationTarget, ValidationRule>,
    ) {
        let after_validation = self.validation_rules.clone();
        let operation = if before_validation == after_validation {
            EditOperation::Columns(columns)
        } else {
            EditOperation::Batch(vec![
                EditOperation::Columns(columns),
                EditOperation::ValidationRules {
                    before: before_validation,
                    after: after_validation,
                },
            ])
        };
        self.history.record(operation);
    }

    // {
    // 責務: [
    // set_references_value: 複数cellを事前検証して一括更新し履歴へ記録する
    // ]
    // 引数: [
    // references: impl IntoIterator<Item = CellRef>
    // value: String
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // 補足: [
    // 全対象と検証規則を先に確認してから一括変更する
    // ]
    // }
    fn set_references_value(
        &mut self,
        references: impl IntoIterator<Item = CellRef>,
        value: String,
    ) -> Result<(), DocumentError> {
        let mut changes = Vec::new();
        let mut violations = Vec::new();

        for reference in references {
            let before = self.cell_ref(reference).ok_or_else(|| {
                DocumentError::Edit(format!(
                    "cell `{reference}` is outside the existing CSV table"
                ))
            })?;

            if before != value {
                if reference.row() > 0 {
                    for (target, rule) in &self.validation_rules {
                        if self.resolve_validation_target(target).ok() == Some(reference.column())
                            && !rule.matches(&value)
                        {
                            violations.push(ValidationViolation {
                                cell: reference,
                                target: target.clone(),
                                value: value.clone(),
                                rule: rule.clone(),
                            });
                        }
                    }
                }
                changes.push(CellChange {
                    reference,
                    before: before.to_owned(),
                    after: value.clone(),
                });
            }
        }

        if !violations.is_empty() {
            return Err(DocumentError::ValidationRejected { violations });
        }

        for change in &changes {
            self.table
                .set_cell(
                    change.reference.row(),
                    change.reference.column(),
                    change.after.clone(),
                )
                .map_err(|error| DocumentError::Edit(error.to_string()))?;
        }

        self.history.record(EditOperation::Cells(changes));
        Ok(())
    }

    // {
    // 責務: [
    // apply_command: 編集commandを指定方向へ適用する
    // ]
    // 引数: [
    // command: &EditCommand
    // direction: CommandDirection
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // エラー: [
    // DocumentError: 編集履歴が現在状態と一致しない場合
    // ]
    // }
    fn apply_command(
        &mut self,
        command: &EditCommand,
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        self.apply_operation(&command.operation, direction)
    }

    // {
    // 責務: [
    // apply_operation: 編集操作を種類に応じて指定方向へ適用する
    // ]
    // 引数: [
    // operation: &EditOperation
    // direction: CommandDirection
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // エラー: [
    // DocumentError: 編集履歴が現在tableまたは検証規則と一致しない場合
    // ]
    // }
    fn apply_operation(
        &mut self,
        operation: &EditOperation,
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        match operation {
            EditOperation::Cells(changes) => self.apply_cell_changes(changes, direction),
            EditOperation::Rows(edit) => self.apply_row_edit(edit, direction),
            EditOperation::Columns(changes) => self.apply_column_changes(changes, direction),
            EditOperation::Contents { before, after } => {
                self.apply_contents_change(before, after, direction)
            }
            EditOperation::ValidationRules { before, after } => {
                let (expected, replacement) = match direction {
                    CommandDirection::Undo => (after, before),
                    CommandDirection::Redo => (before, after),
                };
                if &self.validation_rules != expected {
                    return Err(DocumentError::Edit(
                        "edit history no longer matches the validation rules".into(),
                    ));
                }
                self.validation_rules = replacement.clone();
                Ok(())
            }
            EditOperation::Batch(operations) => match direction {
                CommandDirection::Undo => {
                    for operation in operations.iter().rev() {
                        self.apply_operation(operation, direction)?;
                    }
                    Ok(())
                }
                CommandDirection::Redo => {
                    for operation in operations {
                        self.apply_operation(operation, direction)?;
                    }
                    Ok(())
                }
            },
        }
    }

    // {
    // 責務: [
    // ensure_no_transaction: transaction中に禁止される操作を拒否する
    // ]
    // 引数: [
    // operation: &str
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // エラー: [
    // DocumentError: 指定操作の実行中にtransactionがactiveな場合
    // ]
    // }
    fn ensure_no_transaction(&self, operation: &str) -> Result<(), DocumentError> {
        if self.history.transaction_active() {
            return Err(DocumentError::Transaction(format!(
                "cannot {operation} while a transaction is active"
            )));
        }
        Ok(())
    }

    // {
    // 責務: [
    // apply_cell_changes: cell編集履歴を指定方向へ適用する
    // ]
    // 引数: [
    // changes: &[CellChange]
    // direction: CommandDirection
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // エラー: [
    // DocumentError: 履歴内のcellが現在tableに存在しない場合
    // ]
    // }
    fn apply_cell_changes(
        &mut self,
        changes: &[CellChange],
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        for change in changes {
            let value = match direction {
                CommandDirection::Undo => &change.before,
                CommandDirection::Redo => &change.after,
            };

            self.table
                .set_cell(
                    change.reference.row(),
                    change.reference.column(),
                    value.clone(),
                )
                .map_err(|error| {
                    DocumentError::Edit(format!(
                        "edit history no longer matches the table: {error}"
                    ))
                })?;
        }

        Ok(())
    }

    // {
    // 責務: [
    // apply_row_edit: 行編集履歴の整合性を確認して指定方向へ適用する
    // ]
    // 引数: [
    // edit: &RowEdit
    // direction: CommandDirection
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // エラー: [
    // DocumentError: 履歴内の行範囲または行内容が現在tableと一致しない場合
    // ]
    // }
    fn apply_row_edit(
        &mut self,
        edit: &RowEdit,
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        let (expected, replacement) = match direction {
            CommandDirection::Undo => (&edit.inserted, &edit.removed),
            CommandDirection::Redo => (&edit.removed, &edit.inserted),
        };
        let end = edit
            .index
            .checked_add(expected.len())
            .ok_or_else(|| DocumentError::Edit("row history range overflowed".into()))?;
        let current = self.table.rows().get(edit.index..end).ok_or_else(|| {
            DocumentError::Edit("edit history no longer matches the table rows".into())
        })?;
        if current != expected.as_slice() {
            return Err(DocumentError::Edit(
                "edit history no longer matches the table rows".into(),
            ));
        }

        self.table
            .replace_rows(edit.index, expected.len(), replacement.to_vec())
            .map_err(|error| {
                DocumentError::Edit(format!("edit history no longer matches the table: {error}"))
            })?;
        Ok(())
    }

    // {
    // 責務: [
    // apply_contents_change: table全体の編集履歴を整合性確認後に適用する
    // ]
    // 引数: [
    // before: &[Vec<String>]
    // after: &[Vec<String>]
    // direction: CommandDirection
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // エラー: [
    // DocumentError: 履歴内のtable内容が現在tableと一致しない場合
    // ]
    // }
    fn apply_contents_change(
        &mut self,
        before: &[Vec<String>],
        after: &[Vec<String>],
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        let (expected, replacement) = match direction {
            CommandDirection::Undo => (after, before),
            CommandDirection::Redo => (before, after),
        };
        if self.table.rows() != expected {
            return Err(DocumentError::Edit(
                "edit history no longer matches the table contents".into(),
            ));
        }
        self.table = Table::new(replacement.to_vec());
        Ok(())
    }

    // {
    // 責務: [
    // apply_column_changes: 列編集履歴の整合性を確認して指定方向へ適用する
    // ]
    // 引数: [
    // changes: &[ColumnChange]
    // direction: CommandDirection
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // エラー: [
    // DocumentError: 履歴内の列範囲または値が現在tableと一致しない場合
    // ]
    // }
    fn apply_column_changes(
        &mut self,
        changes: &[ColumnChange],
        direction: CommandDirection,
    ) -> Result<(), DocumentError> {
        for change in changes {
            let expected = match direction {
                CommandDirection::Undo => &change.inserted,
                CommandDirection::Redo => &change.removed,
            };
            let row = self.table.rows().get(change.row).ok_or_else(|| {
                DocumentError::Edit("edit history no longer matches the table rows".into())
            })?;
            let end = change
                .index
                .checked_add(expected.len())
                .ok_or_else(|| DocumentError::Edit("column history range overflowed".into()))?;
            if row.get(change.index..end) != Some(expected.as_slice()) {
                return Err(DocumentError::Edit(
                    "edit history no longer matches the table columns".into(),
                ));
            }
        }

        for change in changes {
            let (expected, replacement) = match direction {
                CommandDirection::Undo => (&change.inserted, &change.removed),
                CommandDirection::Redo => (&change.removed, &change.inserted),
            };
            self.table
                .replace_row_segment(change.row, change.index, expected.len(), replacement)
                .map_err(|error| {
                    DocumentError::Edit(format!(
                        "edit history no longer matches the table: {error}"
                    ))
                })?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
// {
// 責務: [
// CommandDirection: 編集操作を履歴へ適用する方向を表す
// ]
// }
enum CommandDirection {
    Undo,
    Redo,
}

// {
// 責務: [
// structural_conflict: 3つのtable間で自動マージできない構造変更を判定する
// ]
// 引数: [
// baseline: &[Vec<String>]
// local: &[Vec<String>]
// disk: &[Vec<String>]
// ]
// 戻り値: [
// Option<ExternalStructureConflict>: 値が存在するときの結果
// ]
// }
fn structural_conflict(
    baseline: &[Vec<String>],
    local: &[Vec<String>],
    disk: &[Vec<String>],
) -> Option<ExternalStructureConflict> {
    if baseline.len() != local.len() || baseline.len() != disk.len() {
        return Some(ExternalStructureConflict::RowCountChanged);
    }
    if baseline.iter().enumerate().any(|(index, row)| {
        local.get(index).map(Vec::len) != Some(row.len())
            || disk.get(index).map(Vec::len) != Some(row.len())
    }) {
        return Some(ExternalStructureConflict::RowShapeChanged);
    }
    if baseline
        .first()
        .is_some_and(|header| local.first() != Some(header) || disk.first() != Some(header))
    {
        return Some(ExternalStructureConflict::HeaderChanged);
    }
    if row_order_changed(baseline, local) || row_order_changed(baseline, disk) {
        return Some(ExternalStructureConflict::RowOrderChanged);
    }
    if duplicate_rows_changed(baseline, local) || duplicate_rows_changed(baseline, disk) {
        return Some(ExternalStructureConflict::DuplicateRowsAmbiguous);
    }
    None
}

#[derive(Default)]
// {
// 責務: [
// RowDelta: 基準tableからの行削除・挿入・cell変更を集約する
// ]
// フィールド: [
// removed: 削除された基準行のindex
// inserted: 挿入行と挿入位置
// cell_changes: 変更cellと新しい値
// ]
// }
struct RowDelta {
    removed: HashSet<usize>,
    inserted: BTreeMap<usize, Vec<Vec<String>>>,
    cell_changes: BTreeMap<(usize, usize), String>,
}

// {
// 責務: [
// merge_row_only_changes: 行挿入・削除とcell変更を基準tableへ統合する
// ]
// 引数: [
// baseline: &[Vec<String>]
// local: &[Vec<String>]
// disk: &[Vec<String>]
// ]
// 戻り値: [
// Vec<Vec<String>>: 処理成功時の結果
// ]
// エラー: [
// ExternalStructureConflict: header・行形状・順序などが安全な統合条件を満たさない場合
// ]
// }
fn merge_row_only_changes(
    baseline: &[Vec<String>],
    local: &[Vec<String>],
    disk: &[Vec<String>],
) -> Result<Vec<Vec<String>>, ExternalStructureConflict> {
    if baseline.is_empty() || local.first() != baseline.first() || disk.first() != baseline.first()
    {
        return Err(ExternalStructureConflict::HeaderChanged);
    }

    let local_delta = analyze_row_delta(baseline, local)?;
    let disk_delta = analyze_row_delta(baseline, disk)?;
    let mut removed = local_delta.removed.clone();
    removed.extend(disk_delta.removed.iter().copied());

    let mut insertions = local_delta.inserted.clone();
    for (slot, rows) in &disk_delta.inserted {
        match insertions.get(slot) {
            Some(local_rows) if local_rows != rows => {
                return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
            }
            Some(_) => {}
            None => {
                insertions.insert(*slot, rows.clone());
            }
        }
    }

    // 削除行の隣への挿入は、既存行の置換か独立挿入かを区別できないため競合にする。
    if insertions.keys().any(|slot| {
        removed.contains(slot)
            || slot
                .checked_sub(1)
                .is_some_and(|index| removed.contains(&index))
    }) {
        return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
    }

    let mut merged = vec![baseline[0].clone()];
    let mut mapped_rows = HashMap::new();
    for (row_index, baseline_row) in baseline.iter().enumerate().skip(1) {
        if let Some(rows) = insertions.get(&row_index) {
            merged.extend(rows.iter().cloned());
        }
        if !removed.contains(&row_index) {
            mapped_rows.insert(row_index, merged.len());
            merged.push(baseline_row.clone());
        }
    }
    if let Some(rows) = insertions.get(&baseline.len()) {
        merged.extend(rows.iter().cloned());
    }
    for (&(row, column), value) in local_delta
        .cell_changes
        .iter()
        .chain(disk_delta.cell_changes.iter())
    {
        let Some(&merged_row) = mapped_rows.get(&row) else {
            return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
        };
        let cell = merged[merged_row]
            .get_mut(column)
            .ok_or(ExternalStructureConflict::RowShapeChanged)?;
        *cell = value.clone();
    }
    Ok(merged)
}

// {
// 責務: [
// analyze_row_delta: 基準tableから変更tableへの行単位差分を抽出する
// ]
// 引数: [
// baseline: &[Vec<String>]
// changed: &[Vec<String>]
// ]
// 戻り値: [
// RowDelta: 処理成功時の結果
// ]
// エラー: [
// ExternalStructureConflict: 行順序・重複行・行形状から差分を一意に求められない場合
// ]
// }
fn analyze_row_delta(
    baseline: &[Vec<String>],
    changed: &[Vec<String>],
) -> Result<RowDelta, ExternalStructureConflict> {
    if baseline.len() == changed.len()
        && baseline
            .iter()
            .zip(changed)
            .all(|(left, right)| left == right)
    {
        return Ok(RowDelta::default());
    }
    if baseline.len() == changed.len() {
        if row_order_changed(baseline, changed) || duplicate_rows_changed(baseline, changed) {
            return Err(ExternalStructureConflict::RowOrderChanged);
        }
        let mut delta = RowDelta::default();
        for (row_index, (before, after)) in baseline.iter().zip(changed).enumerate().skip(1) {
            if before.len() != after.len() {
                return Err(ExternalStructureConflict::RowShapeChanged);
            }
            for (column, (before, after)) in before.iter().zip(after).enumerate() {
                if before != after {
                    delta
                        .cell_changes
                        .insert((row_index, column), after.clone());
                }
            }
        }
        return Ok(delta);
    }
    let mut baseline_positions = HashMap::new();
    for (index, row) in baseline.iter().enumerate().skip(1) {
        if baseline_positions.insert(row.as_slice(), index).is_some() {
            return Err(ExternalStructureConflict::DuplicateRowsAmbiguous);
        }
    }
    let mut changed_counts = HashMap::<&[String], usize>::new();
    for row in changed.iter().skip(1) {
        *changed_counts.entry(row.as_slice()).or_default() += 1;
    }

    let mut delta = RowDelta::default();
    let mut next_baseline = 1;
    let mut pending_insertions = Vec::new();
    for row in changed.iter().skip(1) {
        match baseline_positions.get(row.as_slice()).copied() {
            Some(index) => {
                if changed_counts.get(row.as_slice()) != Some(&1) || index < next_baseline {
                    return Err(ExternalStructureConflict::RowOrderChanged);
                }
                if !pending_insertions.is_empty() {
                    delta
                        .inserted
                        .insert(index, std::mem::take(&mut pending_insertions));
                }
                delta.removed.extend(next_baseline..index);
                next_baseline = index + 1;
            }
            None => pending_insertions.push(row.clone()),
        }
    }
    if !pending_insertions.is_empty() {
        delta.inserted.insert(baseline.len(), pending_insertions);
    }
    delta.removed.extend(next_baseline..baseline.len());
    Ok(delta)
}

// {
// 責務: [
// row_order_changed: 一意な既存行の順序が変更されたか判定する
// ]
// 引数: [
// baseline: &[Vec<String>]
// changed: &[Vec<String>]
// ]
// 戻り値: [
// bool: 判定または変更結果
// ]
// }
fn row_order_changed(baseline: &[Vec<String>], changed: &[Vec<String>]) -> bool {
    let baseline_counts = row_counts(baseline);
    let mut changed_positions = HashMap::<&[String], (usize, usize)>::new();
    for (index, row) in changed.iter().enumerate() {
        let entry = changed_positions.entry(row.as_slice()).or_default();
        entry.0 += 1;
        entry.1 = index;
    }
    baseline.iter().enumerate().any(|(index, row)| {
        baseline_counts.get(row.as_slice()) == Some(&1)
            && matches!(changed_positions.get(row.as_slice()), Some((1, position)) if *position != index)
    })
}

// {
// 責務: [
// duplicate_rows_changed: 重複する基準行が曖昧な位置へ変更されたか判定する
// ]
// 引数: [
// baseline: &[Vec<String>]
// changed: &[Vec<String>]
// ]
// 戻り値: [
// bool: 判定または変更結果
// ]
// }
fn duplicate_rows_changed(baseline: &[Vec<String>], changed: &[Vec<String>]) -> bool {
    let baseline_counts = row_counts(baseline);
    baseline.iter().enumerate().any(|(index, row)| {
        baseline_counts
            .get(row.as_slice())
            .is_some_and(|count| *count > 1)
            && changed.get(index) != Some(row)
    })
}

// {
// 責務: [
// row_counts: table内の各行内容の出現数を数える
// ]
// 引数: [
// rows: &[Vec<String>]
// ]
// 戻り値: [
// HashMap<&[String], usize>: 処理成功時の結果
// ]
// }
fn row_counts(rows: &[Vec<String>]) -> HashMap<&[String], usize> {
    let mut counts = HashMap::new();
    for row in rows {
        *counts.entry(row.as_slice()).or_insert(0) += 1;
    }
    counts
}

#[derive(Debug, Error)]
/// ```text
/// 責務: [
/// DocumentError: document操作で発生する入出力・編集・検証・競合エラーを表す
/// ]
/// ```
pub enum DocumentError {
    #[error("failed to open CSV `{path}`: {message}")]
    Open { path: String, message: String },

    #[error(transparent)]
    Reference(#[from] ReferenceError),

    #[error("failed to edit CSV: {0}")]
    Edit(String),

    #[error("invalid CSV text: {0}")]
    Csv(String),

    #[error("failed to save CSV `{path}`: {message}")]
    Save { path: String, message: String },

    #[error("failed to verify current CSV `{path}` before refresh/save: {message}")]
    ExternalCheck { path: String, message: String },

    #[error("CSV `{path}` changed outside Rowly after it was opened")]
    ExternalModification { path: String },

    #[error("cannot reapply external conflict draft: {0}")]
    ConflictDraft(String),

    #[error("invalid transaction operation: {0}")]
    Transaction(String),

    #[error("Rowly metadata operation failed: {0}")]
    Metadata(String),

    #[error("input validation rejected one or more cell values")]
    ValidationRejected {
        violations: Vec<ValidationViolation>,
    },

    #[error(transparent)]
    Column(#[from] ColumnError),
}

#[cfg(test)]
mod tests {
    use std::fs;

    use encoding_rs::SHIFT_JIS;
    use tempfile::tempdir;

    use super::*;

    #[test]
    // {
    // 責務: [
    // create_writes_utf8_csv_and_starts_clean: 新規CSVのUTF-8保存と初期clean状態を確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn create_writes_utf8_csv_and_starts_clean() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("created.csv");
        let rows = vec![
            vec!["名前".to_owned(), "値".to_owned()],
            vec!["田中".to_owned(), "1".to_owned()],
        ];

        let document = CsvDocument::create(&path, rows.clone()).unwrap();

        assert_eq!(document.source_encoding(), SourceEncoding::Utf8);
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert_eq!(
            document.rows().map(|row| row.to_vec()).collect::<Vec<_>>(),
            rows
        );

        let reopened = CsvDocument::open(&path).unwrap();
        assert_eq!(reopened.cell_a1("A2").unwrap(), Some("田中"));
        assert_eq!(reopened.cell_a1("B2").unwrap(), Some("1"));
    }

    #[test]
    // {
    // 責務: [
    // column_type_metadata_round_trips_without_changing_csv: 型メタデータの再読込とCSV非変更を確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn column_type_metadata_round_trips_without_changing_csv() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        let source = "名前,年齢\n田中,20\n山田,21\n";
        fs::write(&path, source).unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document
            .set_column_type_declaration_by_header("年齢", ColumnType::Integer)
            .unwrap();
        document.save_metadata().unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        assert!(document.metadata_path().exists());

        let reopened = CsvDocument::open(&path).unwrap();
        assert_eq!(
            reopened.column_type_declaration_by_header("年齢").unwrap(),
            Some(ColumnType::Integer)
        );
        assert_eq!(reopened.metadata_error(), None);
        assert!(!reopened.is_dirty());
    }

    #[test]
    // {
    // 責務: [
    // column_type_metadata_follows_header_after_column_reorder: 列移動後もheader名に紐づく型宣言を確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn column_type_metadata_follows_header_after_column_reorder() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "名前,年齢\n田中,20\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document
            .set_column_type_declaration_by_header("年齢", ColumnType::Integer)
            .unwrap();
        document.insert_columns(0, 1).unwrap();
        document.set_cell(0, 0, "ID").unwrap();

        assert_eq!(document.column_index_by_header("年齢").unwrap(), 2);
        assert_eq!(
            document.column_type_declaration_by_header("年齢").unwrap(),
            Some(ColumnType::Integer)
        );
    }

    #[test]
    // {
    // 責務: [
    // duplicate_header_cannot_receive_type_declaration: 重複headerへの型宣言が拒否されることを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn duplicate_header_cannot_receive_type_declaration() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "名前,名前\n田中,山田\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert!(matches!(
            document.set_column_type_declaration_by_header("名前", ColumnType::String),
            Err(DocumentError::Column(ColumnError::AmbiguousHeader { .. }))
        ));
    }

    #[test]
    // {
    // 責務: [
    // invalid_sidecar_does_not_prevent_csv_open: 不正sidecarがCSV読込を妨げないことを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn invalid_sidecar_does_not_prevent_csv_open() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "名前,年齢\n田中,20\n").unwrap();
        fs::write(
            sidecar_path(&path),
            r#"{"version":1,"columns":{"年齢":{"type":"Unknown"}}}"#,
        )
        .unwrap();

        let document = CsvDocument::open(&path).unwrap();

        assert_eq!(document.cell_a1("B2").unwrap(), Some("20"));
        assert!(document.metadata_error().is_some());
        assert_eq!(
            document.column_type_declaration_by_header("年齢").unwrap(),
            None
        );
    }

    #[test]
    // {
    // 責務: [
    // edit_marks_document_dirty_until_save: 編集後のdirty状態と保存による解消を確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn edit_marks_document_dirty_until_save() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "name,value\nAlice,1\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert!(!document.is_dirty());

        document.set_cell(1, 1, "2").unwrap();
        assert!(document.is_dirty());
        assert_eq!(document.cell(1, 1), Some("2"));

        document.save().unwrap();
        assert!(!document.is_dirty());
        assert_eq!(document.source_encoding(), SourceEncoding::Utf8);
    }

    #[test]
    // {
    // 責務: [
    // a1_range_edit_is_one_undoable_command: A1範囲編集が一つのundo/redo操作になることを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn a1_range_edit_is_one_undoable_command() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "value\n1\n2\n3\n4\n5\n6\n7\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.set_range_a1("A1:A8", "8").unwrap();

        for row in 0..8 {
            assert_eq!(document.cell(row, 0), Some("8"));
        }
        assert!(document.can_undo());
        assert!(!document.can_redo());

        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("A1").unwrap(), Some("value"));
        assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
        assert_eq!(document.cell_a1("A8").unwrap(), Some("7"));
        assert!(document.can_redo());

        assert!(document.redo().unwrap());
        for row in 0..8 {
            assert_eq!(document.cell(row, 0), Some("8"));
        }
    }

    #[test]
    // {
    // 責務: [
    // range_edit_validates_every_cell_before_writing: 範囲編集で全cell検証に失敗した場合のatomic性を確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn range_edit_validates_every_cell_before_writing() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ragged.csv");
        fs::write(&path, "a,b\nc\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        let error = document.set_range_a1("A1:B2", "x").unwrap_err();

        assert!(error.to_string().contains("B2"));
        assert_eq!(document.cell_a1("A1").unwrap(), Some("a"));
        assert_eq!(document.cell_a1("B1").unwrap(), Some("b"));
        assert_eq!(document.cell_a1("A2").unwrap(), Some("c"));
        assert!(!document.can_undo());
        assert!(!document.is_dirty());
    }

    #[test]
    // {
    // 責務: [
    // row_insert_delete_and_history_restore_exact_rows: 行挿入・削除と履歴復元が行内容を保つことを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn row_insert_delete_and_history_restore_exact_rows() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("rows.csv");
        fs::write(&path, "a,b\n1,2\n3,4\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.insert_rows(1, 2).unwrap();
        assert_eq!(document.row_count(), 5);
        assert_eq!(document.cell(1, 0), Some(""));
        assert_eq!(document.cell(1, 1), Some(""));
        assert_eq!(document.cell(3, 0), Some("1"));

        assert!(document.undo().unwrap());
        assert_eq!(document.row_count(), 3);
        assert_eq!(document.cell(1, 0), Some("1"));
        assert!(document.redo().unwrap());
        assert_eq!(document.row_count(), 5);

        document.delete_rows(1, 3).unwrap();
        assert_eq!(document.row_count(), 2);
        assert_eq!(document.cell(1, 0), Some("3"));
        assert!(document.undo().unwrap());
        assert_eq!(document.row_count(), 5);
        assert_eq!(document.cell(3, 0), Some("1"));
        assert_eq!(document.cell(4, 0), Some("3"));
    }

    #[test]
    // {
    // 責務: [
    // column_insert_preserves_ragged_missing_cells_and_undo_restores_shape: 不定幅tableの列挿入とundoが欠損cellを保つことを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn column_insert_preserves_ragged_missing_cells_and_undo_restores_shape() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ragged.csv");
        fs::write(&path, "a,b,c\n1,2\nx\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.insert_columns(2, 1).unwrap();

        assert_eq!(document.column_count(), 4);
        assert_eq!(document.cell(0, 2), Some(""));
        assert_eq!(document.cell(0, 3), Some("c"));
        assert_eq!(document.cell(1, 2), Some(""));
        assert_eq!(document.cell(2, 1), None);

        assert!(document.undo().unwrap());
        assert_eq!(document.column_count(), 3);
        assert_eq!(document.cell(0, 2), Some("c"));
        assert_eq!(document.cell(1, 1), Some("2"));
        assert_eq!(document.cell(1, 2), None);
        assert_eq!(document.cell(2, 1), None);

        assert!(document.redo().unwrap());
        assert_eq!(document.cell(0, 3), Some("c"));
    }

    #[test]
    // {
    // 責務: [
    // column_delete_and_undo_restore_removed_values_per_row: 列削除とundoが各行の削除値を復元することを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn column_delete_and_undo_restore_removed_values_per_row() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("ragged.csv");
        fs::write(&path, "a,b,c,d\n1,2,3\nx,y\nz\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.delete_columns(1, 2).unwrap();

        assert_eq!(document.cell(0, 0), Some("a"));
        assert_eq!(document.cell(0, 1), Some("d"));
        assert_eq!(document.cell(1, 0), Some("1"));
        assert_eq!(document.cell(1, 1), None);
        assert_eq!(document.cell(2, 0), Some("x"));
        assert_eq!(document.cell(2, 1), None);
        assert_eq!(document.cell(3, 0), Some("z"));

        assert!(document.undo().unwrap());
        assert_eq!(document.cell(0, 1), Some("b"));
        assert_eq!(document.cell(0, 2), Some("c"));
        assert_eq!(document.cell(0, 3), Some("d"));
        assert_eq!(document.cell(1, 1), Some("2"));
        assert_eq!(document.cell(1, 2), Some("3"));
        assert_eq!(document.cell(2, 1), Some("y"));
        assert_eq!(document.cell(3, 1), None);
    }

    #[test]
    // {
    // 責務: [
    // invalid_structural_edit_is_atomic_and_not_recorded: 不正な構造編集が状態と履歴を変更しないことを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn invalid_structural_edit_is_atomic_and_not_recorded() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert!(document.delete_rows(1, 2).is_err());
        assert!(document.delete_columns(1, 2).is_err());

        assert_eq!(document.row_count(), 2);
        assert_eq!(document.column_count(), 2);
        assert_eq!(document.cell(1, 1), Some("2"));
        assert!(!document.can_undo());
        assert!(!document.is_dirty());
    }

    #[test]
    // {
    // 責務: [
    // dirty_state_tracks_saved_state_through_undo_and_redo: undo/redoと保存をまたぐdirty状態を確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn dirty_state_tracks_saved_state_through_undo_and_redo() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "name,value\nAlice,1\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.set_cell_a1("B2", "2").unwrap();
        assert!(document.is_dirty());

        assert!(document.undo().unwrap());
        assert!(!document.is_dirty());

        assert!(document.redo().unwrap());
        assert!(document.is_dirty());
        document.save().unwrap();
        assert!(!document.is_dirty());

        assert!(document.undo().unwrap());
        assert!(document.is_dirty());
        assert!(document.redo().unwrap());
        assert!(!document.is_dirty());
    }

    #[test]
    // {
    // 責務: [
    // structural_edit_uses_same_dirty_history_state: 構造編集がdirty履歴へ反映されることを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn structural_edit_uses_same_dirty_history_state() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.insert_columns(1, 1).unwrap();
        assert!(document.is_dirty());

        assert!(document.undo().unwrap());
        assert!(!document.is_dirty());
        assert!(document.redo().unwrap());
        assert!(document.is_dirty());
    }

    #[test]
    // {
    // 責務: [
    // new_edit_after_undo_discards_redo_branch: undo後の新規編集がredo分岐を破棄することを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn new_edit_after_undo_discards_redo_branch() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.set_cell_a1("A2", "x").unwrap();
        document.set_cell_a1("B2", "y").unwrap();
        assert!(document.undo().unwrap());
        assert!(document.can_redo());

        document.set_cell_a1("A1", "header").unwrap();
        assert!(!document.can_redo());
    }

    #[test]
    // {
    // 責務: [
    // committed_transaction_is_one_undoable_command: 確定した複数編集transactionが一つの履歴操作になることを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn committed_transaction_is_one_undoable_command() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n3,4\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.begin_transaction().unwrap();
        document.set_cell_a1("A2", "x").unwrap();
        document.set_cell_a1("B3", "y").unwrap();
        document.insert_rows(2, 1).unwrap();
        assert!(document.is_dirty());
        assert!(document.transaction_active());
        document.commit_transaction().unwrap();

        assert!(!document.transaction_active());
        assert_eq!(document.cell_a1("A2").unwrap(), Some("x"));
        assert_eq!(document.row_count(), 4);

        assert!(document.undo().unwrap());
        assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
        assert_eq!(document.cell_a1("B3").unwrap(), Some("4"));
        assert_eq!(document.row_count(), 3);
        assert!(!document.can_undo());

        assert!(document.redo().unwrap());
        assert_eq!(document.cell_a1("A2").unwrap(), Some("x"));
        assert_eq!(document.row_count(), 4);
    }

    #[test]
    // {
    // 責務: [
    // rollback_transaction_restores_all_changes_without_history: rollbackが状態を戻して履歴に残さないことを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn rollback_transaction_restores_all_changes_without_history() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.begin_transaction().unwrap();
        document.set_cell_a1("A2", "x").unwrap();
        document.insert_columns(1, 1).unwrap();
        assert!(document.is_dirty());

        document.rollback_transaction().unwrap();

        assert_eq!(document.cell_a1("A2").unwrap(), Some("1"));
        assert_eq!(document.cell_a1("B2").unwrap(), Some("2"));
        assert_eq!(document.column_count(), 2);
        assert!(!document.is_dirty());
        assert!(!document.can_undo());
        assert!(!document.transaction_active());
    }

    #[test]
    // {
    // 責務: [
    // transaction_rejects_nested_begin_and_history_or_save_operations: transaction中の入れ子開始・履歴操作・保存を拒否することを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn transaction_rejects_nested_begin_and_history_or_save_operations() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        document.begin_transaction().unwrap();
        assert!(matches!(
            document.begin_transaction(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.undo(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.redo(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.save(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.save_as(directory.path().join("other.csv")),
            Err(DocumentError::Transaction(_))
        ));

        document.rollback_transaction().unwrap();
    }

    #[test]
    // {
    // 責務: [
    // transaction_commit_and_rollback_require_active_transaction: commitとrollbackにactive transactionが必要なことを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn transaction_commit_and_rollback_require_active_transaction() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, "a,b\n1,2\n").unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert!(matches!(
            document.commit_transaction(),
            Err(DocumentError::Transaction(_))
        ));
        assert!(matches!(
            document.rollback_transaction(),
            Err(DocumentError::Transaction(_))
        ));
    }

    #[test]
    // {
    // 責務: [
    // saving_shift_jis_input_converts_file_to_utf8: Shift JISのCSVが保存時にUTF-8へ変換されることを確認する
    // ]
    // 引数: [
    // ]
    // 戻り値: [
    // (): 成功時に値を返さない
    // ]
    // }
    fn saving_shift_jis_input_converts_file_to_utf8() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("japanese.csv");
        let source = "部署,名前\n営業部,田中太郎\n開発部,山田一郎\n";
        let (encoded, _, had_errors) = SHIFT_JIS.encode(source);
        assert!(!had_errors);
        fs::write(&path, encoded.as_ref()).unwrap();

        let mut document = CsvDocument::open(&path).unwrap();
        assert_eq!(document.source_encoding(), SourceEncoding::ShiftJis);
        assert_eq!(document.cell(1, 1), Some("田中太郎"));

        document.save().unwrap();

        let saved = fs::read(&path).unwrap();
        let saved_text = std::str::from_utf8(&saved).unwrap();
        assert_eq!(saved_text, source);
        assert_eq!(document.source_encoding(), SourceEncoding::Utf8);
    }
}
