use crate::process::{CellRef, CsvDocument, DocumentError};

use super::{RowlyApp, format_validation_violations, update_selection};

impl RowlyApp {
    // {
    //   責務: [refresh_code_buffer: CSV documentの現在のtextをeditor bufferへ同期する。]
    //   処理: [documentがなければbufferをclearし、取得できればtextを置き換えてdirty flagを解除する。]
    //   引数: [self: document・code buffer・statusを保持する画面状態。]
    //   戻り値: [(): text取得errorはstatusへ反映する。]
    // }
    pub(super) fn refresh_code_buffer(&mut self) {
        let Some(document) = self.document.as_ref() else {
            self.code_buffer.clear();
            self.code_buffer_dirty = false;
            return;
        };
        match document.csv_text() {
            Ok(text) => {
                self.code_buffer = text;
                self.code_buffer_dirty = false;
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [apply_code_buffer: text editorのCSV textをdocumentへ検証付きで適用する。]
    //   処理: [CsvDocument::apply_csv_textを呼び、成功時にbuffer・selectionを同期し、失敗理由をstatusへ示す。]
    //   引数: [self: 適用対象documentとcode buffer。]
    //   戻り値: [(): 適用成否をdirty flag・buffer・statusへ反映する。]
    // }
    pub(super) fn apply_code_buffer(&mut self) {
        let Some(document) = self.document.as_mut() else {
            self.status = "適用する CSV がありません".to_owned();
            return;
        };
        let result = document.apply_csv_text(&self.code_buffer);
        match result {
            Ok(()) => {
                self.code_buffer_dirty = false;
                self.status = match self.calculation_engine.recalculate_all(document) {
                    Ok(_) => "Code changes applied".to_owned(),
                    Err(error) => error.to_string(),
                };
                self.refresh_code_buffer();
                self.clamp_selection();
            }
            Err(DocumentError::ValidationRejected { violations }) => {
                self.status = format_validation_violations(&violations);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [set_selection: document bounds内へcell selectionを更新する。]
    //   処理: [現在のrow・column数を取得しupdate_selectionへselectionとanchor更新を委譲する。]
    //   引数: [self: selection・anchor・reference入力を保持する画面状態。 reference: 選択先。 extend: anchorを維持するか。]
    //   戻り値: [(): selection・address入力・editing状態を同期する。]
    // }
    pub(super) fn set_selection(&mut self, reference: CellRef, extend: bool) {
        let (row_count, column_count) = self.document.as_ref().map_or((0, 0), |document| {
            (document.row_count(), document.column_count())
        });
        update_selection(
            &mut self.selection,
            &mut self.selection_anchor,
            &mut self.reference_input,
            &mut self.editing,
            reference,
            extend,
            (row_count, column_count),
        );
    }

    // {
    //   責務: [begin_edit: 選択cellのinline editを開始し、既存値に対応する候補位置を設定する。]
    //   処理: [入力値をcopyし、allowed values内で一致する候補indexを探す。]
    //   引数: [self: selection・editing・candidate state。 value: 編集開始時のcell text。]
    //   戻り値: [(): inline editを有効化する。]
    // }
    pub(super) fn begin_edit(&mut self, value: &str) {
        self.edit_value = value.to_owned();
        self.editing = true;
        self.validation_candidates_open = false;
        self.validation_candidate_index = self
            .document
            .as_ref()
            .and_then(|document| {
                document
                    .allowed_values_for_column(self.selection.column())
                    .and_then(|values| values.iter().position(|candidate| candidate == value))
            })
            .unwrap_or(0);
    }

    // {
    //   責務: [set_cell: process APIでcell textを更新し結果をstatusへ反映する。]
    //   処理: [documentがあればset_cellを呼び、入力規則違反と他のdocument errorを区別する。]
    //   引数: [self: 更新対象documentとstatus。 row: 0-based row index。 column: 0-based column index。 value: 書き込むtext。]
    //   戻り値: [bool: 更新成功ならtrue、document不在または失敗ならfalse。]
    //   副作用: [成功時にdocumentを編集する。]
    // }
    pub(super) fn set_cell(&mut self, row: usize, column: usize, value: String) -> bool {
        let Some(document) = self.document.as_mut() else {
            return false;
        };
        match document.set_cell(row, column, value) {
            Ok(()) => {
                self.status = match self
                    .calculation_engine
                    .recalculate_for_changes(document, [CellRef::new(row, column)])
                {
                    Ok(_) => format!("Cell updated ({})", self.selection),
                    Err(error) => error.to_string(),
                };
                true
            }
            Err(DocumentError::ValidationRejected { violations }) => {
                self.status = format_validation_violations(&violations);
                false
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

    // {
    //   責務: [move_selection_after_tab: Tab確定後の次cellへselectionを進める。]
    //   処理: [同じrowの次columnを優先し、末columnなら次rowの先頭columnへ移動する。]
    //   引数: [self: documentと現在selectionを保持する画面状態。]
    //   戻り値: [(): selection更新対象がある場合に次cellへ移動する。]
    // }
    pub(super) fn move_selection_after_tab(&mut self) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let row_count = document.row_count();
        let column_count = document.column_count();
        if row_count == 0 || column_count == 0 {
            return;
        }

        let next = if self.selection.column() + 1 < column_count {
            CellRef::new(self.selection.row(), self.selection.column() + 1)
        } else if self.selection.row() + 1 < row_count {
            CellRef::new(self.selection.row() + 1, 0)
        } else {
            self.selection
        };
        self.set_selection(next, false);
    }

    // {
    //   責務: [navigate_to_cell: reference入力をCellRefとして解析し、該当cellへ移動する。]
    //   処理: [入力をparseし成功時はselection/statusを更新し、parse errorはstatusへ表示する。]
    //   引数: [self: reference入力・selection・statusを保持する画面状態。]
    //   戻り値: [(): reference解析結果を画面状態へ反映する。]
    // }
    pub(super) fn navigate_to_cell(&mut self) {
        match self.reference_input.parse::<CellRef>() {
            Ok(reference) => {
                self.set_selection(reference, false);
                self.status = format!("Selected {}", self.selection);
            }
            Err(error) => self.status = error.to_string(),
        }
    }

    // {
    //   責務: [insert_row: 選択rowの位置へ1行を挿入する。]
    //   処理: [CsvDocument::insert_rowsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 挿入結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればrow構造を変更する。]
    // }
    pub(super) fn insert_row(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.insert_rows(self.selection.row(), 1));
        self.apply_structure_result(result, "Inserted row");
    }

    // {
    //   責務: [delete_row: 選択rowを1行削除する。]
    //   処理: [CsvDocument::delete_rowsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 削除結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればrow構造を変更する。]
    // }
    pub(super) fn delete_row(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.delete_rows(self.selection.row(), 1));
        self.apply_structure_result(result, "Deleted row");
    }

    // {
    //   責務: [insert_column: 選択columnの位置へ1列を挿入する。]
    //   処理: [CsvDocument::insert_columnsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 挿入結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればcolumn構造を変更する。]
    // }
    pub(super) fn insert_column(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.insert_columns(self.selection.column(), 1));
        self.apply_structure_result(result, "Inserted column");
    }

    // {
    //   責務: [delete_column: 選択columnを1列削除する。]
    //   処理: [CsvDocument::delete_columnsを呼び、共通結果処理へ渡す。]
    //   引数: [self: 選択位置・documentを保持する画面状態。]
    //   戻り値: [(): 削除結果をstatus・selectionへ反映する。]
    //   副作用: [documentがあればcolumn構造を変更する。]
    // }
    pub(super) fn delete_column(&mut self) {
        let result = self
            .document
            .as_mut()
            .map(|document| document.delete_columns(self.selection.column(), 1));
        self.apply_structure_result(result, "Deleted column");
    }

    // {
    //   責務: [apply_structure_result: row・column構造操作のoption付き結果を画面状態へ反映する。]
    //   処理: [成功時にstatus設定とselection clampを行い、errorまたはdocument不在をstatusへ表示する。]
    //   引数: [self: status・selection・editing state。 result: document不在または構造操作結果。 success: 成功時に使うstatus text。]
    //   戻り値: [(): 操作結果を画面状態へ反映する。]
    // }
    pub(super) fn apply_structure_result(
        &mut self,
        result: Option<Result<(), crate::process::DocumentError>>,
        success: &str,
    ) {
        match result {
            Some(Ok(())) => {
                self.status = success.to_owned();
                self.clamp_selection();
            }
            Some(Err(error)) => self.status = error.to_string(),
            None => self.status = "CSV ファイルを開いてください".to_owned(),
        }
        self.editing = false;
    }

    // {
    //   責務: [clamp_selection: documentのrow・column範囲外にあるselectionを末尾へ収める。]
    //   処理: [現在位置を各dimensionの最大indexへ制限し、通常のselection更新経路を使う。]
    //   引数: [self: documentとselectionを保持する画面状態。]
    //   戻り値: [(): documentがある場合にselectionを範囲内へ更新する。]
    // }
    pub(super) fn clamp_selection(&mut self) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let row = self
            .selection
            .row()
            .min(document.row_count().saturating_sub(1));
        let column = self
            .selection
            .column()
            .min(document.column_count().saturating_sub(1));
        self.set_selection(CellRef::new(row, column), false);
    }

    // {
    //   責務: [undo: documentの直近commandを取り消し、selectionを有効範囲へ保つ。]
    //   処理: [CsvDocument::undo結果をstatusへ反映し、変更時はselectionをclampする。]
    //   引数: [self: document・selection・statusを保持する画面状態。]
    //   戻り値: [(): undo結果をstatus・selection・editing stateへ反映する。]
    // }
    pub(super) fn undo(&mut self) {
        match self.document.as_mut().map(CsvDocument::undo) {
            Some(Ok(true)) => {
                self.status = match self
                    .document
                    .as_ref()
                    .map(|document| self.calculation_engine.recalculate_all(document))
                {
                    Some(Ok(_)) => "Undone".to_owned(),
                    Some(Err(error)) => error.to_string(),
                    None => "Undone".to_owned(),
                };
                self.clamp_selection();
            }
            Some(Ok(false)) => self.status = "Nothing to undo".to_owned(),
            Some(Err(error)) => self.status = error.to_string(),
            None => {}
        }
        self.editing = false;
    }

    // {
    //   責務: [redo: documentの直近undo commandを再適用し、selectionを有効範囲へ保つ。]
    //   処理: [CsvDocument::redo結果をstatusへ反映し、変更時はselectionをclampする。]
    //   引数: [self: document・selection・statusを保持する画面状態。]
    //   戻り値: [(): redo結果をstatus・selection・editing stateへ反映する。]
    // }
    pub(super) fn redo(&mut self) {
        match self.document.as_mut().map(CsvDocument::redo) {
            Some(Ok(true)) => {
                self.status = match self
                    .document
                    .as_ref()
                    .map(|document| self.calculation_engine.recalculate_all(document))
                {
                    Some(Ok(_)) => "Redone".to_owned(),
                    Some(Err(error)) => error.to_string(),
                    None => "Redone".to_owned(),
                };
                self.clamp_selection();
            }
            Some(Ok(false)) => self.status = "Nothing to redo".to_owned(),
            Some(Err(error)) => self.status = error.to_string(),
            None => {}
        }
        self.editing = false;
    }
}
