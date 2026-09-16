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

    pub(crate) fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum TableError {
    #[error("row {row} is out of bounds for {row_count} rows")]
    RowOutOfBounds { row: usize, row_count: usize },

    #[error("column {column} is out of bounds for row {row}, which has {column_count} columns")]
    ColumnOutOfBounds {
        row: usize,
        column: usize,
        column_count: usize,
    },
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
}
