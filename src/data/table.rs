use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Table {
    rows: Vec<Vec<String>>,
}

impl Table {
    pub(crate) fn new(rows: Vec<Vec<String>>) -> Self {
        Self { rows }
    }

    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn column_count(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub(crate) fn cell(&self, row: usize, column: usize) -> Option<&str> {
        self.rows
            .get(row)
            .and_then(|current_row| current_row.get(column))
            .map(String::as_str)
    }

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

    pub(crate) fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }
}

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

    #[test]
    fn column_count_uses_widest_row() {
        let table = Table::new(vec![
            vec!["a".into()],
            vec!["b".into(), "c".into(), "d".into()],
        ]);

        assert_eq!(table.row_count(), 2);
        assert_eq!(table.column_count(), 3);
    }

    #[test]
    fn editing_existing_cell_reports_change() {
        let mut table = Table::new(vec![vec!["old".into()]]);

        assert!(table.set_cell(0, 0, "new").unwrap());
        assert_eq!(table.cell(0, 0), Some("new"));
        assert!(!table.set_cell(0, 0, "new").unwrap());
    }

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
