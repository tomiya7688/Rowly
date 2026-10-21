use csv::{ReaderBuilder, Terminator, WriterBuilder};

use super::{CellRange, CellRef, CsvDocument, DocumentError};

impl CsvDocument {
    // {
    //   責務: [copy_range_tsv: 指定範囲のCSV値をclipboard用のTSV文字列へ変換する。],
    //   処理: [範囲を確認し、タブ・改行・引用符をTSVの引用規則で符号化する。],
    //   引数: [self: コピー元のCSV document, range: コピーする矩形範囲。],
    //   戻り値: [Result<String, DocumentError>: TSV文字列、または範囲・変換エラー。],
    //   補足: [短い行の欠けた末尾cellは空欄として出力する。計算式と計算結果は参照しない。]
    // }
    pub fn copy_range_tsv(&self, range: CellRange) -> Result<String, DocumentError> {
        if range.end().row() >= self.row_count() || range.end().column() >= self.column_count() {
            return Err(DocumentError::Edit(
                "コピー範囲が既存CSVの行数または列数を超えています".to_owned(),
            ));
        }

        let mut writer = WriterBuilder::new()
            .delimiter(b'\t')
            .terminator(Terminator::Any(b'\n'))
            .from_writer(Vec::new());
        // CSV値を参照して符号化するため、コピーでdocumentや編集履歴は変更されない。
        for row_index in range.start().row()..=range.end().row() {
            let cells = (range.start().column()..=range.end().column())
                .map(|column_index| self.cell(row_index, column_index).unwrap_or(""));
            writer
                .write_record(cells)
                .map_err(|error| DocumentError::Csv(error.to_string()))?;
        }
        let bytes = writer
            .into_inner()
            .map_err(|error| DocumentError::Csv(error.error().to_string()))?;
        String::from_utf8(bytes).map_err(|error| DocumentError::Csv(error.to_string()))
    }

    // {
    //   責務: [paste_tsv_values: clipboardのTSV値を既存CSV cellへ一括適用する。],
    //   処理: [TSVと全対象cellを確認し、CSV全体の検証に成功した場合だけ置換する。],
    //   引数: [self: 更新対象のCSV document, start: 貼り付け先の左上cell, text: TSV文字列。],
    //   戻り値: [Result<Vec<CellRef>, DocumentError>: 変更cell一覧、または解析・範囲・検証エラー。],
    //   副作用: [成功時の全変更を一つのUndo単位として記録する。失敗時はCSVと履歴を保つ。],
    //   補足: [短い行の欠けた末尾cellへ空欄を貼る場合は行を拡張しない。Bindingは操作しない。]
    // }
    pub fn paste_tsv_values(
        &mut self,
        start: CellRef,
        text: &str,
    ) -> Result<Vec<CellRef>, DocumentError> {
        let pasted_rows = parse_clipboard_tsv(text)?;
        let changes = collect_clipboard_changes(self, start, pasted_rows)?;
        if changes.is_empty() {
            return Ok(Vec::new());
        }

        // 既存の一括置換APIで最終状態を検証する。header変更時も列名による規則を検証する。
        // 範囲不正・変更なしの貼り付けでは全表snapshotを作らず、redo履歴も保持する。
        let mut rows = self.rows().map(<[String]>::to_vec).collect::<Vec<_>>();
        let mut changed_cells = Vec::with_capacity(changes.len());
        for (reference, value) in changes {
            rows[reference.row()][reference.column()] = value;
            changed_cells.push(reference);
        }
        self.replace_contents(rows)?;
        Ok(changed_cells)
    }
}

// {
//   責務: [parse_clipboard_tsv: clipboardのTSV文字列を行とcell値へ解析する。],
//   処理: [引用されたタブ・改行と行ごとの列数の違いを許容してrecordを読む。],
//   引数: [text: clipboardのTSV文字列。],
//   戻り値: [Result<Vec<Vec<String>>, DocumentError>: 解析した行、またはTSV解析エラー。]
// }
fn parse_clipboard_tsv(text: &str) -> Result<Vec<Vec<String>>, DocumentError> {
    ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes())
        .records()
        .map(|record| {
            record
                .map(|record| record.iter().map(str::to_owned).collect())
                .map_err(|error| DocumentError::Csv(error.to_string()))
        })
        .collect()
}

// {
//   責務: [collect_clipboard_changes: 貼り付け範囲を確認し、値が変わる既存cellだけを集める。],
//   処理: [行列の加算overflowと範囲外を拒否し、短い行への空欄の貼り付けを変更なしとする。],
//   引数: [document: 読み取り専用の貼り付け先, start: 左上cell, pasted_rows: 解析済みTSV値。],
//   戻り値: [Result<Vec<(CellRef, String)>, DocumentError>: cellと新値の組、または範囲エラー。]
// }
fn collect_clipboard_changes(
    document: &CsvDocument,
    start: CellRef,
    pasted_rows: Vec<Vec<String>>,
) -> Result<Vec<(CellRef, String)>, DocumentError> {
    if start
        .row()
        .checked_add(pasted_rows.len())
        .is_none_or(|end_row| end_row > document.row_count())
    {
        return Err(DocumentError::Edit(
            "貼り付け範囲がCSVの行数を超えています".to_owned(),
        ));
    }
    let column_count = document.column_count();
    let mut changes = Vec::new();
    for (row_offset, pasted_row) in pasted_rows.into_iter().enumerate() {
        if start
            .column()
            .checked_add(pasted_row.len())
            .is_none_or(|end_column| end_column > column_count)
        {
            return Err(DocumentError::Edit(
                "貼り付け範囲がCSVの列数を超えています".to_owned(),
            ));
        }
        for (column_offset, value) in pasted_row.into_iter().enumerate() {
            let reference = CellRef::new(start.row() + row_offset, start.column() + column_offset);
            match document.cell_ref(reference) {
                Some(before) if before != value => changes.push((reference, value)),
                Some(_) => {}
                None if value.is_empty() => {}
                None => {
                    return Err(DocumentError::Edit(format!(
                        "貼り付け先cell {reference} が存在しません"
                    )));
                }
            }
        }
    }
    Ok(changes)
}
