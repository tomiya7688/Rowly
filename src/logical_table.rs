//! Logical views and source-aware row operations over project CSV sources.
//!
//! Rows keep their source id and file-local record index as provenance. Table
//! grouping and display order never rewrite or merge the physical CSV files.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::{
    process::CsvDocument,
    project::{RowlyProject, SourceKind},
};

/// ```text
/// 責務: [
/// LogicalProject: projectのsource CSVをschemaごとの論理tableとしてまとめる
/// ]
/// フィールド: [
/// tables: 読み込み順に並ぶ、schema単位の論理table
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalProject {
    /// ```text
    /// 責務: [
    /// tables: sourceの出現順で保持するschema別table一覧
    /// ]
    /// ```
    pub tables: Vec<LogicalTable>,
}

/// ```text
/// 責務: [
/// LogicalTable: 同じ順序のheaderを持つCSV群の行とsource provenanceを保持する
/// ]
/// フィールド: [
/// schema: tableを構成するCSVに共通する順序付きheader
/// source_ids: tableへファイルを提供するproject source id
/// source_files: source idとCSV pathの対応一覧
/// default_write_target: 省略時の行追加先として設定されたsource id
/// display_order: rowsのindexを表示順に並べた一覧
/// rows: provenanceを含む論理行
/// move_undo: 取り消し可能な最新順の行移動履歴
/// move_redo: 再実行可能な行移動履歴
/// ]
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalTable {
    /// ```text
    /// 責務: [schema: tableを構成するCSVに共通する順序付きheader]
    /// ```
    pub schema: Vec<String>,
    /// ```text
    /// 責務: [source_ids: tableへファイルを提供するproject source id]
    /// ```
    pub source_ids: Vec<String>,
    /// ```text
    /// 責務: [source_files: source idごとに提供されたCSV pathを読み込み順で保持する]
    /// ```
    pub source_files: Vec<SourceFile>,
    /// ```text
    /// 責務: [default_write_target: 行追加時の既定source id。未設定ならNone]
    /// ```
    pub default_write_target: Option<String>,
    /// ```text
    /// 責務: [display_order: rowsのindexを画面上の順序として保持する]
    /// 補足: [並べ替えてもrowsやsource provenanceは変更しない]
    /// ```
    pub display_order: Vec<usize>,
    /// ```text
    /// 責務: [rows: source provenance付きの論理行を保持する]
    /// ```
    pub rows: Vec<LogicalRow>,
    move_undo: Vec<RowMove>,
    move_redo: Vec<RowMove>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// LogicalRow: CSVの値と現在の格納先の位置を一行分まとめる
/// ]
/// フィールド: [
/// values: CSV record順のcell値。schema幅と異なる場合がある
/// origin: 現在このrowを保持しているsource、path、record index
/// ]
/// ```
pub struct LogicalRow {
    /// ```text
    /// 責務: [values: CSV record順のcell値。schema幅と異なる場合がある]
    /// ```
    pub values: Vec<String>,
    /// ```text
    /// 責務: [origin: この行を保持するCSV recordのsource provenance]
    /// ```
    pub origin: SourceRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// SourceRecord: rowを現在保持しているsource id、path、headerを除くzero-based indexを保持する
/// ]
/// フィールド: [
/// source_id: project上のsource識別子
/// path: recordを保持するCSV file
/// record_index: headerを除いたrecord位置
/// ]
/// ```
pub struct SourceRecord {
    /// ```text
    /// 責務: [source_id: project上でsourceを識別するstable id]
    /// ```
    pub source_id: String,
    /// ```text
    /// 責務: [path: recordを保持するCSV fileへのpath]
    /// ```
    pub path: PathBuf,
    /// ```text
    /// 責務: [record_index: header recordを除くdata recordのzero-based位置]
    /// ```
    pub record_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// ```text
/// 責務: [
/// SourceFile: project sourceと、そのsourceが提供するCSV pathを対応付ける
/// ]
/// フィールド: [
/// source_id: project上のsource識別子
/// path: sourceが提供するCSV file
/// ]
/// ```
pub struct SourceFile {
    /// ```text
    /// 責務: [source_id: このfileを提供するproject sourceのstable id]
    /// ```
    pub source_id: String,
    /// ```text
    /// 責務: [path: project sourceが提供するCSV fileへのpath]
    /// ```
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
// {
//   責務: [
//     RowMove: 1回のsource間row移動をundo / redoするための値と位置を記録する
//   ]
//   フィールド: [
//     logical_row_index: 論理table内で移動したrowのindex
//     source_id: 移動元source id
//     source_path: 移動元CSV path
//     source_record_index: 移動元のdata record index
//     target_id: 移動先source id
//     target_path: 移動先CSV path
//     target_record_index: 移動先のdata record index
//     values: 移動対象rowのcell値
//   ]
// }
struct RowMove {
    logical_row_index: usize,
    source_id: String,
    source_path: PathBuf,
    source_record_index: usize,
    target_id: String,
    target_path: PathBuf,
    target_record_index: usize,
    values: Vec<String>,
}

impl LogicalProject {
    /// ```text
    /// 責務: [load: projectのCSV sourceを読み込み、ordered headerが一致するfileを同じtableにまとめる]
    /// 処理: [
    /// 1: file sourceまたは再帰設定に従ったdirectory sourceからCSV pathを集める
    /// 2: headerをschemaとしてtableを選び、source fileとdata recordのprovenanceを追加する
    /// 3: 各tableの初期display orderをrow順で設定する
    /// ]
    /// 引数: [
    /// project: 解決対象のsource定義を持つproject
    /// manifest_path: 相対source pathの基準となるmanifest path
    /// ]
    /// 戻り値: [
    /// LogicalProject: schema単位に構成したtable群
    /// ]
    /// エラー: [LogicalLoadError: directory読込、CSV解析、またはheader欠落の理由]
    /// ```
    pub fn load(
        project: &RowlyProject,
        manifest_path: impl AsRef<Path>,
    ) -> Result<Self, LogicalLoadError> {
        let mut tables = Vec::<LogicalTable>::new();
        let mut table_by_schema = HashMap::<Vec<String>, usize>::new();

        for source in project.resolve_sources(manifest_path) {
            let paths = match source.kind {
                SourceKind::File => vec![source.path],
                SourceKind::Directory => collect_csv_files(&source.path, source.recursive)?,
            };
            for path in paths {
                let document = CsvDocument::open(&path).map_err(|error| LogicalLoadError::Csv {
                    path: path.clone(),
                    message: error.to_string(),
                })?;
                let mut records = document.rows();
                let Some(header) = records.next() else {
                    return Err(LogicalLoadError::MissingHeader(path));
                };
                let schema = header.to_vec();
                let table_index = *table_by_schema.entry(schema.clone()).or_insert_with(|| {
                    let index = tables.len();
                    tables.push(LogicalTable {
                        schema,
                        source_ids: Vec::new(),
                        source_files: Vec::new(),
                        default_write_target: None,
                        display_order: Vec::new(),
                        rows: Vec::new(),
                        move_undo: Vec::new(),
                        move_redo: Vec::new(),
                    });
                    index
                });
                let table = &mut tables[table_index];
                if !table.source_ids.contains(&source.id) {
                    table.source_ids.push(source.id.clone());
                }
                if !table
                    .source_files
                    .iter()
                    .any(|file| file.source_id == source.id && file.path == path)
                {
                    table.source_files.push(SourceFile {
                        source_id: source.id.clone(),
                        path: path.clone(),
                    });
                }
                for (record_index, values) in records.enumerate() {
                    table.rows.push(LogicalRow {
                        values: values.to_vec(),
                        origin: SourceRecord {
                            source_id: source.id.clone(),
                            path: path.clone(),
                            record_index,
                        },
                    });
                }
            }
        }

        for table in &mut tables {
            table.display_order = (0..table.rows.len()).collect();
        }
        Ok(Self { tables })
    }
}

impl LogicalTable {
    /// ```text
    /// 責務: [set_default_write_target: 行追加時にsource idを省略した場合の書き込み先を設定する]
    /// 処理: [tableに含まれるsource idだけをdefault_write_targetへ保存する]
    /// 引数: [source_id: このtableにCSVを提供しているsource id]
    /// 戻り値: [(): 設定成功時に値を返さない]
    /// エラー: [LogicalTableError: tableに含まれないsource idが指定された]
    /// 副作用: [default_write_targetを更新する]
    /// ```
    pub fn set_default_write_target(
        &mut self,
        source_id: impl Into<String>,
    ) -> Result<(), LogicalTableError> {
        let source_id = source_id.into();
        if !self.source_ids.contains(&source_id) {
            return Err(LogicalTableError::IncompatibleWriteTarget(source_id));
        }
        self.default_write_target = Some(source_id);
        Ok(())
    }

    /// ```text
    /// 責務: [clear_default_write_target: 行追加先の既定source idを解除する]
    /// 処理: [default_write_targetをNoneにする]
    /// 引数: []
    /// 戻り値: [(): 値を返さない]
    /// 副作用: [default_write_targetを更新する]
    /// ```
    pub fn clear_default_write_target(&mut self) {
        self.default_write_target = None;
    }

    /// ```text
    /// 責務: [resolve_write_target: 明示source idまたは既定値が有効な書き込み先か解決する]
    /// 処理: [明示値を優先し、tableに含まれるsource idであることを確認する]
    /// 引数: [explicit_source_id: 指定があれば使うsource id]
    /// 戻り値: [String: 解決したsource id]
    /// エラー: [LogicalTableError: target未指定、またはtableに含まれないsource id]
    /// ```
    pub fn resolve_write_target(
        &self,
        explicit_source_id: Option<&str>,
    ) -> Result<String, LogicalTableError> {
        let target = explicit_source_id
            .or(self.default_write_target.as_deref())
            .ok_or(LogicalTableError::NoDefaultWriteTarget)?;
        if !self.source_ids.iter().any(|source_id| source_id == target) {
            return Err(LogicalTableError::IncompatibleWriteTarget(
                target.to_owned(),
            ));
        }
        Ok(target.to_owned())
    }

    /// ```text
    /// 責務: [append_row: 選択したsource CSV末尾へrowを追加し、論理tableを更新する]
    /// 処理: [row幅とsource fileを検証し、CSV保存後に論理rowと表示順を追加する]
    /// 引数: [
    /// values: schema幅に一致するcell値
    /// explicit_source_id: 書き込み先source id。Noneなら既定値を使う
    /// ]
    /// 戻り値: [usize: 追加した論理row index]
    /// 副作用: [CSVを保存し、rowsとdisplay_orderを更新してredo履歴を消去する]
    /// エラー: [LogicalTableError: row幅、target、CSV、schema、または保存の失敗]
    /// ```
    pub fn append_row(
        &mut self,
        values: Vec<String>,
        explicit_source_id: Option<&str>,
    ) -> Result<usize, LogicalTableError> {
        if values.len() != self.schema.len() {
            return Err(LogicalTableError::InvalidRowWidth {
                expected: self.schema.len(),
                actual: values.len(),
            });
        }
        let source_id = self.resolve_write_target(explicit_source_id)?;
        let mut matching_files = self
            .source_files
            .iter()
            .filter(|file| file.source_id == source_id);
        let file = matching_files
            .next()
            .ok_or_else(|| LogicalTableError::SourceFileUnavailable(source_id.clone()))?;
        let path = file.path.clone();
        if matching_files.next().is_some() {
            return Err(LogicalTableError::AmbiguousSourceFile(source_id));
        }
        let mut document =
            CsvDocument::open(&path).map_err(|error| LogicalTableError::SourceDocument {
                path: path.clone(),
                message: error.to_string(),
            })?;
        let header = document.rows().next().unwrap_or_default();
        if header != self.schema {
            return Err(LogicalTableError::SchemaChanged(path));
        }
        let row_index_in_document = document.row_count();
        let record_index = row_index_in_document.saturating_sub(1);
        let append_result = (|| {
            document.begin_transaction()?;
            document.insert_rows(row_index_in_document, 1)?;
            for (column, value) in values.iter().enumerate() {
                document.set_cell(row_index_in_document, column, value.clone())?;
            }
            document.commit_transaction()?;
            document.save()
        })();
        if let Err(error) = append_result {
            if document.transaction_active() {
                let _ = document.rollback_transaction();
            }
            return Err(LogicalTableError::SourceDocument {
                path,
                message: error.to_string(),
            });
        }
        let row_index = self.rows.len();
        self.rows.push(LogicalRow {
            values,
            origin: SourceRecord {
                source_id,
                path,
                record_index,
            },
        });
        self.display_order.push(row_index);
        self.move_redo.clear();
        Ok(row_index)
    }

    /// ```text
    /// 責務: [move_row: 同じschemaの別source CSVへ既存rowを移動する]
    /// 処理: [rowとsourceを検証し、CSV移送後にprovenanceとundo履歴を更新する]
    /// 引数: [
    /// row_index: 移動対象の論理row index
    /// target_source_id: 移動先source id
    /// ]
    /// 戻り値: [(): 移動成功時に値を返さない]
    /// 副作用: [両CSVとrow provenanceを更新し、undo履歴へ記録してredo履歴を消去する]
    /// エラー: [LogicalTableError: index、source、schema、record、またはCSV更新の失敗]
    /// ```
    pub fn move_row(
        &mut self,
        row_index: usize,
        target_source_id: &str,
    ) -> Result<(), LogicalTableError> {
        let row = self
            .rows
            .get(row_index)
            .cloned()
            .ok_or(LogicalTableError::InvalidRowIndex(row_index))?;
        let target_id = self.resolve_write_target(Some(target_source_id))?;
        if target_id == row.origin.source_id {
            return Err(LogicalTableError::SameSource);
        }
        let target_path = self.single_source_path(&target_id)?;
        if target_path == row.origin.path {
            return Err(LogicalTableError::SameSource);
        }
        let target_document = open_compatible_source(&target_path, &self.schema)?;
        let target_record_index = target_document.row_count().saturating_sub(1);
        transfer_csv_record(
            &row.origin.path,
            row.origin.record_index,
            &target_path,
            target_record_index,
            &row.values,
            &self.schema,
        )?;
        self.apply_provenance_move(
            row_index,
            &row.origin.path,
            row.origin.record_index,
            &target_path,
            target_record_index,
            &target_id,
        );
        self.move_undo.push(RowMove {
            logical_row_index: row_index,
            source_id: row.origin.source_id,
            source_path: row.origin.path,
            source_record_index: row.origin.record_index,
            target_id,
            target_path,
            target_record_index,
            values: row.values,
        });
        self.move_redo.clear();
        Ok(())
    }

    /// ```text
    /// 責務: [can_undo_move: undo履歴が空でないかを返す]
    /// 処理: [move_undoが空でないか確認する]
    /// 引数: []
    /// 戻り値: [bool: undo履歴があればtrue。CSV状態と照合した実行可否は確認しない]
    /// ```
    pub fn can_undo_move(&self) -> bool {
        !self.move_undo.is_empty()
    }

    /// ```text
    /// 責務: [can_redo_move: redo履歴が空でないかを返す]
    /// 処理: [move_redoが空でないか確認する]
    /// 引数: []
    /// 戻り値: [bool: redo履歴があればtrue。CSV状態と照合した実行可否は確認しない]
    /// ```
    pub fn can_redo_move(&self) -> bool {
        !self.move_redo.is_empty()
    }

    /// ```text
    /// 責務: [undo_move: 最新のrow移動を元のCSVへ戻す]
    /// 処理: [CSV間でrecordを戻し、provenanceとundo / redo履歴を更新する]
    /// 引数: []
    /// 戻り値: [bool: 移動を取り消した場合true、履歴が空ならfalse]
    /// 副作用: [CSV、row provenance、移動履歴を更新する]
    /// エラー: [LogicalTableError: CSVのschemaまたはrecordの検証・移送に失敗した]
    /// ```
    pub fn undo_move(&mut self) -> Result<bool, LogicalTableError> {
        let Some(operation) = self.move_undo.last().cloned() else {
            return Ok(false);
        };
        transfer_csv_record(
            &operation.target_path,
            operation.target_record_index,
            &operation.source_path,
            operation.source_record_index,
            &operation.values,
            &self.schema,
        )?;
        self.apply_provenance_move(
            operation.logical_row_index,
            &operation.target_path,
            operation.target_record_index,
            &operation.source_path,
            operation.source_record_index,
            &operation.source_id,
        );
        self.move_undo.pop();
        self.move_redo.push(operation);
        Ok(true)
    }

    /// ```text
    /// 責務: [redo_move: 最新のundo済みrow移動を再実行する]
    /// 処理: [CSV間でrecordを再移送し、provenanceとundo / redo履歴を更新する]
    /// 引数: []
    /// 戻り値: [bool: 移動を再実行した場合true、履歴が空ならfalse]
    /// 副作用: [CSV、row provenance、移動履歴を更新する]
    /// エラー: [LogicalTableError: CSVのschemaまたはrecordの検証・移送に失敗した]
    /// ```
    pub fn redo_move(&mut self) -> Result<bool, LogicalTableError> {
        let Some(operation) = self.move_redo.last().cloned() else {
            return Ok(false);
        };
        transfer_csv_record(
            &operation.source_path,
            operation.source_record_index,
            &operation.target_path,
            operation.target_record_index,
            &operation.values,
            &self.schema,
        )?;
        self.apply_provenance_move(
            operation.logical_row_index,
            &operation.source_path,
            operation.source_record_index,
            &operation.target_path,
            operation.target_record_index,
            &operation.target_id,
        );
        self.move_redo.pop();
        self.move_undo.push(operation);
        Ok(true)
    }

    // {
    //   責務: [
    //     single_source_path: source idに対応する一意なCSV pathを得る
    //   ]
    //   処理: [
    //     1: source file群をsource idで絞る
    //     2: pathがない、または複数ならエラーにする
    //   ]
    //   引数: [source_id: pathを解決するsource id]
    //   戻り値: [PathBuf: 一意に選ばれたCSV path]
    //   エラー: [LogicalTableError: fileなし、または複数fileで一意にできない]
    // }
    fn single_source_path(&self, source_id: &str) -> Result<PathBuf, LogicalTableError> {
        let mut files = self
            .source_files
            .iter()
            .filter(|file| file.source_id == source_id);
        let path = files
            .next()
            .map(|file| file.path.clone())
            .ok_or_else(|| LogicalTableError::SourceFileUnavailable(source_id.to_owned()))?;
        if files.next().is_some() {
            return Err(LogicalTableError::AmbiguousSourceFile(source_id.to_owned()));
        }
        Ok(path)
    }

    // {
    //   責務: [
    //     apply_provenance_move: row移動後の各data record indexと移動rowのprovenanceを同期する
    //   ]
    //   処理: [
    //     1: 移動元以降のindexを詰め、移動先以降のindexをずらす
    //     2: 移動rowへ新しいsource id、path、indexを設定する
    //   ]
    //   引数: [
    //     logical_row_index: provenanceを更新する論理row index
    //     source_path: recordを削除したCSV path
    //     source_record_index: 移動前のdata record index
    //     target_path: recordを挿入したCSV path
    //     target_record_index: 移動後のdata record index
    //     target_id: 移動後のsource id
    //   ]
    //   戻り値: [(): provenance更新後は値を返さない]
    //   副作用: [rows内のSourceRecordを変更する]
    // }
    fn apply_provenance_move(
        &mut self,
        logical_row_index: usize,
        source_path: &Path,
        source_record_index: usize,
        target_path: &Path,
        target_record_index: usize,
        target_id: &str,
    ) {
        for (index, row) in self.rows.iter_mut().enumerate() {
            if index == logical_row_index {
                continue;
            }
            if row.origin.path == source_path && row.origin.record_index > source_record_index {
                row.origin.record_index -= 1;
            }
            if row.origin.path == target_path && row.origin.record_index >= target_record_index {
                row.origin.record_index += 1;
            }
        }
        let row = &mut self.rows[logical_row_index];
        row.origin.source_id = target_id.to_owned();
        row.origin.path = target_path.to_path_buf();
        row.origin.record_index = target_record_index;
    }
}

// {
//   責務: [
//     open_compatible_source: CSVを開き、headerが論理tableのschemaと一致することを確認する
//   ]
//   処理: [
//     1: CsvDocumentを開く
//     2: 先頭recordをschemaと比較する
//   ]
//   引数: [path: 開くCSV path, schema: 期待するordered header]
//   戻り値: [CsvDocument: header検証済みのCSV document]
//   エラー: [LogicalTableError: CSV読込失敗またはheader不一致]
// }
fn open_compatible_source(
    path: &Path,
    schema: &[String],
) -> Result<CsvDocument, LogicalTableError> {
    let document = CsvDocument::open(path).map_err(|error| LogicalTableError::SourceDocument {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if document.rows().next().unwrap_or_default() != schema {
        return Err(LogicalTableError::SchemaChanged(path.to_path_buf()));
    }
    Ok(document)
}

// {
//   責務: [
//     transfer_csv_record: 期待した値のrecordをsource CSVからtarget CSVへ移送する
//   ]
//   処理: [
//     1: 両CSVのschema、source recordのindexと値、target indexを検証する
//     2: 両CSVをbackupし、transactionでrecordを削除・挿入して保存する
//     3: 保存失敗時はtransactionを戻し、両CSVをbackupから復元する
//   ]
//   引数: [source_path: 移動元CSV, source_record_index: 移動元data record位置, target_path: 移動先CSV, target_record_index: 移動先data record位置, expected_values: 移送対象の期待値, schema: 両CSVに必要なordered header]
//   戻り値: [(): 両CSVへの移送が完了したとき値を返さない]
//   副作用: [両CSV fileを更新し、一時backup fileを作成する。backup削除はbest-effortで、失敗時はfileが残り得る]
//   エラー: [LogicalTableError: schema、record、I/O、transaction、save、復元の失敗]
// }
fn transfer_csv_record(
    source_path: &Path,
    source_record_index: usize,
    target_path: &Path,
    target_record_index: usize,
    expected_values: &[String],
    schema: &[String],
) -> Result<(), LogicalTableError> {
    if source_path == target_path {
        return Err(LogicalTableError::SameSource);
    }
    let mut source = open_compatible_source(source_path, schema)?;
    let mut target = open_compatible_source(target_path, schema)?;
    let source_row_index = source_record_index + 1;
    let actual_values = source
        .rows()
        .nth(source_row_index)
        .map(<[String]>::to_vec)
        .ok_or_else(|| LogicalTableError::InvalidRecordIndex {
            path: source_path.to_path_buf(),
            record_index: source_record_index,
        })?;
    if actual_values != expected_values {
        return Err(LogicalTableError::SourceRecordChanged {
            path: source_path.to_path_buf(),
            record_index: source_record_index,
        });
    }
    if actual_values.len() != schema.len() {
        return Err(LogicalTableError::InvalidRowWidth {
            expected: schema.len(),
            actual: actual_values.len(),
        });
    }
    let target_row_index = target_record_index + 1;
    if target_row_index > target.row_count() {
        return Err(LogicalTableError::InvalidRecordIndex {
            path: target_path.to_path_buf(),
            record_index: target_record_index,
        });
    }

    let source_backup = make_backup(source_path)?;
    let target_backup = match make_backup(target_path) {
        Ok(backup) => backup,
        Err(error) => {
            let _ = fs::remove_file(&source_backup);
            return Err(error);
        }
    };

    let edit_result = (|| {
        source.begin_transaction()?;
        target.begin_transaction()?;
        source.delete_rows(source_row_index, 1)?;
        target.insert_rows(target_row_index, 1)?;
        for (column, value) in actual_values.iter().enumerate() {
            target.set_cell(target_row_index, column, value.clone())?;
        }
        source.commit_transaction()?;
        target.commit_transaction()?;
        target.save()?;
        source.save()?;
        Ok::<(), crate::process::DocumentError>(())
    })();

    if let Err(error) = edit_result {
        if source.transaction_active() {
            let _ = source.rollback_transaction();
        }
        if target.transaction_active() {
            let _ = target.rollback_transaction();
        }
        let restore_errors = [
            fs::copy(&source_backup, source_path).err(),
            fs::copy(&target_backup, target_path).err(),
        ]
        .into_iter()
        .flatten()
        .map(|restore_error| restore_error.to_string())
        .collect::<Vec<_>>();
        let _ = fs::remove_file(source_backup);
        let _ = fs::remove_file(target_backup);
        let mut message = error.to_string();
        if !restore_errors.is_empty() {
            message.push_str("; failed to restore original CSVs: ");
            message.push_str(&restore_errors.join("; "));
        }
        return Err(LogicalTableError::SourceDocument {
            path: source_path.to_path_buf(),
            message,
        });
    }
    let _ = fs::remove_file(source_backup);
    let _ = fs::remove_file(target_backup);
    Ok(())
}

// {
//   責務: [
//     make_backup: CSV fileの隣に衝突しにくい名前のbackupを作成する
//   ]
//   処理: [process idと時刻を名前に含め、元fileをcopyする]
//   引数: [path: backup元のCSV file]
//   戻り値: [PathBuf: 作成したbackup path]
//   副作用: [backup fileを作成する]
//   エラー: [LogicalTableError: backup fileの作成に失敗した理由]
// }
fn make_backup(path: &Path) -> Result<PathBuf, LogicalTableError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.{}.rowly-backup", std::process::id(), stamp));
    let backup = path.with_file_name(name);
    fs::copy(path, &backup).map_err(|error| LogicalTableError::SourceDocument {
        path: path.to_path_buf(),
        message: format!("failed to create transaction backup: {error}"),
    })?;
    Ok(backup)
}

// {
//   責務: [
//     collect_csv_files: directory source内のCSV pathを集め、path順に返す
//   ]
//   処理: [recursive設定でdirectoryを探索し、結果をsortする]
//   引数: [root: 探索するdirectory, recursive: 子directoryも探索するか]
//   戻り値: [Vec<PathBuf>: path順のCSV一覧]
//   エラー: [LogicalLoadError: rootが存在しないかdirectoryではない、または読込できない]
// }
fn collect_csv_files(root: &Path, recursive: bool) -> Result<Vec<PathBuf>, LogicalLoadError> {
    if !root.is_dir() {
        return Err(LogicalLoadError::MissingDirectory(root.to_path_buf()));
    }
    let mut paths = Vec::new();
    collect_csv_files_into(root, recursive, &mut paths)?;
    paths.sort();
    Ok(paths)
}

// {
//   責務: [
//     collect_csv_files_into: directory entryを調べ、CSV pathを出力一覧へ追加する
//   ]
//   処理: [
//     1: symlinkを除外し、CSV fileを一覧へ追加する
//     2: recursiveが有効なときだけ子directoryを探索する
//   ]
//   引数: [root: 現在の探索directory, recursive: 子directoryを探索するか, paths: 発見したCSV pathの出力先]
//   戻り値: [(): path追加後は値を返さない]
//   副作用: [発見したCSV pathをpathsへ追加する]
//   エラー: [LogicalLoadError: directory entryの読込に失敗した理由]
// }
fn collect_csv_files_into(
    root: &Path,
    recursive: bool,
    paths: &mut Vec<PathBuf>,
) -> Result<(), LogicalLoadError> {
    let entries = fs::read_dir(root).map_err(|source| LogicalLoadError::ReadDirectory {
        path: root.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| LogicalLoadError::ReadDirectory {
            path: root.to_path_buf(),
            source,
        })?;
        let file_type = entry
            .file_type()
            .map_err(|source| LogicalLoadError::ReadDirectory {
                path: entry.path(),
                source,
            })?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_file() && is_csv(&entry.path()) {
            paths.push(entry.path());
        } else if recursive && file_type.is_dir() {
            collect_csv_files_into(&entry.path(), true, paths)?;
        }
    }
    Ok(())
}

// {
//   責務: [
//     is_csv: pathの拡張子が大文字小文字を問わずcsvか判定する
//   ]
//   処理: [拡張子をUTF-8文字列として取得してcsvと比較する]
//   引数: [path: 拡張子を確認するpath]
//   戻り値: [bool: csv拡張子ならtrue]
// }
fn is_csv(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}

#[derive(Debug, Error)]
/// ```text
/// 責務: [
/// LogicalLoadError: projectのsource CSVを論理tableへ読み込めない理由を表す
/// ]
/// 補足: [
/// MissingDirectory: source pathが存在しないかdirectoryではない
/// MissingHeader: CSVにheader recordがない
/// ReadDirectory: directoryまたはentryを読み込めない
/// Csv: CSV fileを開く、または解析できない
/// ]
/// ```
pub enum LogicalLoadError {
    /// source pathが存在しないか、directoryでない場合。
    #[error("project source directory does not exist: {0}")]
    MissingDirectory(PathBuf),
    /// CSVの先頭にheader recordがない場合。
    #[error("CSV has no header record: {0}")]
    MissingHeader(PathBuf),
    /// directoryまたはentryの読み込みに失敗した場合。
    #[error("failed to read `{path}`: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// CSVを開く、または解析できない場合。
    #[error("failed to load CSV `{path}`: {message}")]
    Csv { path: PathBuf, message: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
/// ```text
/// 責務: [
/// LogicalTableError: logical tableのsource選択、row操作、CSV更新の失敗理由を表す
/// ]
/// 補足: [
/// NoDefaultWriteTarget: 明示値も既定書き込み先もない
/// IncompatibleWriteTarget: source idがlogical tableに含まれない
/// InvalidRowIndex: 論理row indexが範囲外
/// InvalidRowWidth: row値の個数がschema幅と一致しない
/// SourceFileUnavailable: source idに対応するCSVがない
/// AmbiguousSourceFile: source idに複数のCSVが対応し書き込み先を一意に決められない
/// SchemaChanged: 読み込み後にCSV headerが変わった
/// InvalidRecordIndex: CSV内に指定recordがない
/// SourceRecordChanged: 読み込み後にrecord値が変わった
/// SameSource: 移動元と移動先が同一
/// SourceDocument: CSV操作に失敗した
/// ]
/// ```
pub enum LogicalTableError {
    #[error("no default write target is configured and no explicit target was provided")]
    NoDefaultWriteTarget,
    #[error("source `{0}` is not part of this logical table")]
    IncompatibleWriteTarget(String),
    #[error("logical row index {0} is out of range")]
    InvalidRowIndex(usize),
    #[error("row has {actual} values, but this logical table requires {expected}")]
    InvalidRowWidth { expected: usize, actual: usize },
    #[error("source `{0}` has no CSV file in this logical table")]
    SourceFileUnavailable(String),
    #[error("source `{0}` contributes multiple CSV files; row target file is ambiguous")]
    AmbiguousSourceFile(String),
    #[error("source CSV header changed since this logical table was loaded: {0}")]
    SchemaChanged(PathBuf),
    #[error("source `{path}` has no data record at index {record_index}")]
    InvalidRecordIndex { path: PathBuf, record_index: usize },
    #[error("source `{path}` record {record_index} changed since the logical table was loaded")]
    SourceRecordChanged { path: PathBuf, record_index: usize },
    #[error("a row cannot be moved to its existing source")]
    SameSource,
    #[error("failed to update source CSV `{path}`: {message}")]
    SourceDocument { path: PathBuf, message: String },
}
