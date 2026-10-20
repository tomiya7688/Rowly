use eframe::egui;

use crate::process::CellRange;

use super::RowlyApp;

impl RowlyApp {
    // {
    //   責務: [copy_selected_csv_values: 選択範囲のCSV値をclipboardへコピーする。],
    //   処理: [process層で範囲をTSVへ変換し、eguiのclipboard出力へ渡す。],
    //   引数: [self: documentと選択範囲を持つ画面状態, ctx: clipboard出力先。],
    //   戻り値: [(): コピー失敗時は理由をstatusへ表示する。]
    // }
    pub(super) fn copy_selected_csv_values(&mut self, ctx: &egui::Context) {
        let Some(document) = self.document.as_ref() else {
            return;
        };
        let selected_range = CellRange::new(self.selection_anchor, self.selection);
        match document.copy_range_tsv(selected_range) {
            Ok(text) => ctx.copy_text(text),
            Err(error) => self.status = format!("CSV値をclipboardへコピーできません: {error}"),
        }
    }

    // {
    //   責務: [paste_csv_values: clipboardのCSV値を貼り付け、依存する計算結果を更新する。],
    //   処理: [process層で一括適用し、変更cellの再計算とCode表示の同期を行う。],
    //   引数: [self: documentと現在cellを持つ画面状態, text: clipboardのTSV文字列。],
    //   戻り値: [(): 貼り付けまたは再計算の結果をstatusへ表示する。],
    //   副作用: [成功時にCSV値を編集する。未適用のCode編集がある場合は貼り付けを拒否する。]
    // }
    pub(super) fn paste_csv_values(&mut self, text: &str) {
        if self.code_buffer_dirty {
            self.status = "先にText Editorの未適用CSV変更を適用してください".to_owned();
            return;
        }
        let Some(document) = self.document.as_mut() else {
            return;
        };
        let changed_cells = match document.paste_tsv_values(self.selection, text) {
            Ok(cells) if !cells.is_empty() => cells,
            Ok(_) => {
                self.status = "貼り付け値に変更はありません".to_owned();
                return;
            }
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        };

        // Bindingの定義は変更せず、CSV値に依存する結果だけを再評価する。
        self.status = match self
            .calculation_engine
            .recalculate_for_changes(document, changed_cells)
        {
            Ok(_) => "CSV値を貼り付けました".to_owned(),
            Err(error) => error.to_string(),
        };
        self.refresh_code_buffer();
    }
}
