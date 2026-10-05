use thiserror::Error;

// {
//   責務: [
//     Table: 可変長rowと文字列cellからなるCSVのcanonical dataを保持する
//   ]
//   フィールド: [
//     rows: record順を保った可変長の文字列row一覧
//   ]
// }
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Table {
    rows: Vec<Vec<String>>,
}

impl Table {
    // {
    //   責務: [
    //     new: record row一覧からTableを作る
    //   ]
    //   引数: [
    //     rows: 順序と各rowの長さを保ったrecord一覧
    //   ]
    //   戻り値: [
    //     Self: 指定されたrecordを保持するtable
    //   ]
    // }
    pub(crate) fn new(rows: Vec<Vec<String>>) -> Self {
        Self { rows }
    }

    // {
    //   責務: [
    //     row_count: tableのrecord数を返す
    //   ]
    //   戻り値: [
    //     usize: 現在のrow数
    //   ]
    // }
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    // {
    //   責務: [
    //     column_count: 最も長いrowの幅をtableの列数として返す
    //   ]
    //   戻り値: [
    //     usize: 最大row幅。rowがないtableでは0
    //   ]
    // }
    pub(crate) fn column_count(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    // {
    //   責務: [
    //     cell: 指定row・columnのcellを範囲外ならNoneとして返す
    //   ]
    //   引数: [
    //     row: 取得対象のzero-based row index
    //     column: 取得対象のzero-based column index
    //   ]
    //   戻り値: [
    //     Option<&str>: cellが存在するときの文字列参照
    //   ]
    // }
    pub(crate) fn cell(&self, row: usize, column: usize) -> Option<&str> {
        self.rows
            .get(row)
            .and_then(|current_row| current_row.get(column))
            .map(String::as_str)
    }

    // {
    //   責務: [
    //     set_cell: 既存cellだけを更新し、実際に値が変わったかを返す
    //   ]
    //   引数: [
    //     row: 更新対象のzero-based row index
    //     column: 更新対象のzero-based column index
    //     value: 設定する文字列値
    //   ]
    //   戻り値: [
    //     bool: 値が変更された場合true、同じ値ならfalse
    //     TableError: rowまたはcellが存在しない理由
    //   ]
    // }
    pub(crate) fn set_cell(
        &mut self,
        row: usize,
        column: usize,
        value: impl Into<String>,
    ) -> Result<bool, TableError> {
        let row_count = self.rows.len();
        let current_row = self
            .rows
            .get_mut(row)
            .ok_or(TableError::RowOutOfBounds { row, row_count })?;

        let column_count = current_row.len();
        let cell = current_row
            .get_mut(column)
            .ok_or(TableError::ColumnOutOfBounds {
                row,
                column,
                column_count,
            })?;

        let value = value.into();
        if *cell == value {
            return Ok(false);
        }

        *cell = value;
        Ok(true)
    }

    // {
    //   責務: [
    //     replace_rows: 指定row範囲を挿入rowで置換し、削除rowを正確に返す
    //   ]
    //   引数: [
    //     index: zero-based挿入位置
    //     remove_count: 置換で削除するrow数
    //     inserted: 挿入するrow一覧
    //   ]
    //   戻り値: [
    //     Vec<Vec<String>>: 削除されたrowの元の順序と値
    //     TableError: 範囲またはindexが無効な理由
    //   ]
    // }
    pub(crate) fn replace_rows(
        &mut self,
        index: usize,
        remove_count: usize,
        inserted: Vec<Vec<String>>,
    ) -> Result<Vec<Vec<String>>, TableError> {
        let row_count = self.rows.len();
        if index > row_count {
            return Err(TableError::RowInsertOutOfBounds { index, row_count });
        }

        let end = index
            .checked_add(remove_count)
            .ok_or(TableError::RangeOverflow)?;
        if end > row_count {
            return Err(TableError::RowRangeOutOfBounds {
                index,
                remove_count,
                row_count,
            });
        }

        Ok(self.rows.splice(index..end, inserted).collect())
    }

    // {
    //   責務: [
    //     replace_row_segment: 1 row内のcolumn範囲を置換し、削除cellを正確に返す
    //   ]
    //   引数: [
    //     row: 更新対象のzero-based row index
    //     index: row内のzero-based挿入位置
    //     remove_count: 置換で削除するcell数
    //     inserted: 挿入するcell値
    //   ]
    //   戻り値: [
    //     Vec<String>: 削除されたcellの元の順序と値
    //     TableError: rowまたはcolumn範囲が無効な理由
    //   ]
    // }
    pub(crate) fn replace_row_segment(
        &mut self,
        row: usize,
        index: usize,
        remove_count: usize,
        inserted: &[String],
    ) -> Result<Vec<String>, TableError> {
        let row_count = self.rows.len();
        let current_row = self
            .rows
            .get_mut(row)
            .ok_or(TableError::RowOutOfBounds { row, row_count })?;
        let column_count = current_row.len();

        if index > column_count {
            return Err(TableError::ColumnInsertOutOfBounds {
                row,
                index,
                column_count,
            });
        }

        let end = index
            .checked_add(remove_count)
            .ok_or(TableError::RangeOverflow)?;
        if end > column_count {
            return Err(TableError::ColumnRangeOutOfBounds {
                row,
                index,
                remove_count,
                column_count,
            });
        }

        Ok(current_row
            .splice(index..end, inserted.iter().cloned())
            .collect())
    }

    // {
    //   責務: [
    //     rows: CSV書込などで利用するrow一覧への読み取り専用参照を返す
    //   ]
    //   戻り値: [
    //     &[Vec<String>]: tableが保持するrow一覧
    //   ]
    // }
    pub(crate) fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }
}

// {
//   責務: [
//     TableError: row・column編集が境界を越えた理由を表す
//   ]
//   フィールド: [
//     RowOutOfBounds: 存在しないrowを参照した位置とrow数
//     RowInsertOutOfBounds: row挿入位置とrow数
//     RowRangeOutOfBounds: row置換範囲とrow数
//     ColumnOutOfBounds: row内にないcolumnとrow幅
//     ColumnInsertOutOfBounds: column挿入位置とrow幅
//     ColumnRangeOutOfBounds: column置換範囲とrow幅
//     RangeOverflow: 範囲終端の加算overflow
//   ]
// }
#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum TableError {
    #[error("row {row} is out of bounds for {row_count} rows")]
    RowOutOfBounds { row: usize, row_count: usize },

    #[error("row insertion index {index} is out of bounds for {row_count} rows")]
    RowInsertOutOfBounds { index: usize, row_count: usize },

    #[error("row range starting at {index} with length {remove_count} exceeds {row_count} rows")]
    RowRangeOutOfBounds {
        index: usize,
        remove_count: usize,
        row_count: usize,
    },

    #[error("column {column} is out of bounds for row {row}, which has {column_count} columns")]
    ColumnOutOfBounds {
        row: usize,
        column: usize,
        column_count: usize,
    },

    #[error(
        "column insertion index {index} is out of bounds for row {row}, which has {column_count} columns"
    )]
    ColumnInsertOutOfBounds {
        row: usize,
        index: usize,
        column_count: usize,
    },

    #[error(
        "column range starting at {index} with length {remove_count} exceeds row {row}, which has {column_count} columns"
    )]
    ColumnRangeOutOfBounds {
        row: usize,
        index: usize,
        remove_count: usize,
        column_count: usize,
    },

    #[error("table range arithmetic overflowed")]
    RangeOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    // {
    //   責務: [
    //     column_count_uses_widest_row: ragged rowでは最大幅を列数として扱う
    //   ]
    //   処理: [
    //     1: 幅の異なるrowからtableを作る
    //     2: row数と最大column数を確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn column_count_uses_widest_row() {
        let table = Table::new(vec![
            vec!["a".into()],
            vec!["b".into(), "c".into(), "d".into()],
        ]);

        assert_eq!(table.row_count(), 2);
        assert_eq!(table.column_count(), 3);
    }

    // {
    //   責務: [
    //     editing_existing_cell_reports_change: cell更新の変更有無と保存値を確認する
    //   ]
    //   処理: [
    //     1: 異なる値と同じ値を順に設定する
    //     2: 変更flagと最終cell値を確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn editing_existing_cell_reports_change() {
        let mut table = Table::new(vec![vec!["old".into()]]);

        assert!(table.set_cell(0, 0, "new").unwrap());
        assert_eq!(table.cell(0, 0), Some("new"));
        assert!(!table.set_cell(0, 0, "new").unwrap());
    }

    // {
    //   責務: [
    //     replacing_rows_returns_exact_removed_rows: row置換が削除されたrow値を保持する
    //   ]
    //   処理: [
    //     1: 中間rowを複数rowで置換する
    //     2: 削除rowと置換後の全row順を確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn replacing_rows_returns_exact_removed_rows() {
        let mut table = Table::new(vec![vec!["a".into()], vec!["b".into()], vec!["c".into()]]);

        let removed = table
            .replace_rows(1, 1, vec![vec!["x".into()], vec!["y".into()]])
            .unwrap();

        assert_eq!(removed, vec![vec!["b".to_string()]]);
        assert_eq!(
            table.rows(),
            &[
                vec!["a".to_string()],
                vec!["x".to_string()],
                vec!["y".to_string()],
                vec!["c".to_string()],
            ]
        );
    }

    // {
    //   責務: [
    //     replacing_row_segment_preserves_surrounding_cells: column範囲置換で周辺cellと削除値を保持する
    //   ]
    //   処理: [
    //     1: 1 rowの中間cellを異なる長さのsegmentで置換する
    //     2: 削除値と周辺を含む置換後のrowを確認する
    //   ]
    //   引数: []
    //   戻り値: [
    //     (): assertion成功時に値を返さない
    //   ]
    // }
    #[test]
    fn replacing_row_segment_preserves_surrounding_cells() {
        let mut table = Table::new(vec![vec!["a".into(), "b".into(), "c".into(), "d".into()]]);

        let removed = table
            .replace_row_segment(0, 1, 2, &["x".into(), "y".into(), "z".into()])
            .unwrap();

        assert_eq!(removed, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(
            table.rows(),
            &[vec![
                "a".to_string(),
                "x".to_string(),
                "y".to_string(),
                "z".to_string(),
                "d".to_string(),
            ]]
        );
    }
}
