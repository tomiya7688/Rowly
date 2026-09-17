use thiserror::Error;

use super::{CellRef, CsvDocument};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    String,
    Integer,
    Decimal,
    Boolean,
}

impl ColumnType {
    fn matches(self, value: &str) -> bool {
        match self {
            Self::String => true,
            Self::Integer => value.parse::<i64>().is_ok(),
            Self::Decimal => value.parse::<f64>().is_ok_and(|parsed| parsed.is_finite()),
            Self::Boolean => {
                value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnCell {
    reference: CellRef,
    value: String,
}

impl ColumnCell {
    pub fn reference(&self) -> CellRef {
        self.reference
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnTypeReport {
    column: usize,
    column_type: ColumnType,
    checked_cells: usize,
    mismatches: Vec<ColumnCell>,
}

impl ColumnTypeReport {
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn column_type(&self) -> ColumnType {
        self.column_type
    }

    pub fn checked_cells(&self) -> usize {
        self.checked_cells
    }

    pub fn mismatches(&self) -> &[ColumnCell] {
        &self.mismatches
    }

    pub fn is_valid(&self) -> bool {
        self.mismatches.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JapaneseCheckReport {
    column: usize,
    checked_cells: usize,
    matches: Vec<ColumnCell>,
    mismatches: Vec<ColumnCell>,
}

impl JapaneseCheckReport {
    pub fn column(&self) -> usize {
        self.column
    }

    pub fn checked_cells(&self) -> usize {
        self.checked_cells
    }

    pub fn matches(&self) -> &[ColumnCell] {
        &self.matches
    }

    pub fn mismatches(&self) -> &[ColumnCell] {
        &self.mismatches
    }

    pub fn all_match(&self) -> bool {
        self.checked_cells > 0 && self.mismatches.is_empty()
    }
}

impl CsvDocument {
    pub fn column_indices_by_header(&self, header: &str) -> Vec<usize> {
        (0..self.column_count())
            .filter(|&column| self.cell(0, column) == Some(header))
            .collect()
    }

    pub fn column_index_by_header(&self, header: &str) -> Result<usize, ColumnError> {
        let matches = self.column_indices_by_header(header);
        match matches.as_slice() {
            [column] => Ok(*column),
            [] => Err(ColumnError::HeaderNotFound(header.to_owned())),
            _ => Err(ColumnError::AmbiguousHeader {
                header: header.to_owned(),
                columns: matches,
            }),
        }
    }

    pub fn validate_column_type(
        &self,
        column: usize,
        column_type: ColumnType,
    ) -> Result<ColumnTypeReport, ColumnError> {
        self.ensure_column_exists(column)?;

        let mut checked_cells = 0;
        let mut mismatches = Vec::new();
        for row in 1..self.row_count() {
            let Some(value) = self.cell(row, column) else {
                continue;
            };
            checked_cells += 1;
            if !column_type.matches(value) {
                mismatches.push(ColumnCell {
                    reference: CellRef::new(row, column),
                    value: value.to_owned(),
                });
            }
        }

        Ok(ColumnTypeReport {
            column,
            column_type,
            checked_cells,
            mismatches,
        })
    }

    pub fn validate_column_type_by_header(
        &self,
        header: &str,
        column_type: ColumnType,
    ) -> Result<ColumnTypeReport, ColumnError> {
        let column = self.column_index_by_header(header)?;
        self.validate_column_type(column, column_type)
    }

    pub fn check_column_japanese(&self, column: usize) -> Result<JapaneseCheckReport, ColumnError> {
        self.ensure_column_exists(column)?;

        let mut checked_cells = 0;
        let mut matches = Vec::new();
        let mut mismatches = Vec::new();
        for row in 1..self.row_count() {
            let Some(value) = self.cell(row, column) else {
                continue;
            };
            checked_cells += 1;
            let cell = ColumnCell {
                reference: CellRef::new(row, column),
                value: value.to_owned(),
            };
            if contains_japanese(value) {
                matches.push(cell);
            } else {
                mismatches.push(cell);
            }
        }

        Ok(JapaneseCheckReport {
            column,
            checked_cells,
            matches,
            mismatches,
        })
    }

    pub fn check_column_japanese_by_header(
        &self,
        header: &str,
    ) -> Result<JapaneseCheckReport, ColumnError> {
        let column = self.column_index_by_header(header)?;
        self.check_column_japanese(column)
    }

    fn ensure_column_exists(&self, column: usize) -> Result<(), ColumnError> {
        let column_count = self.column_count();
        if column >= column_count {
            return Err(ColumnError::ColumnOutOfBounds {
                column,
                column_count,
            });
        }
        Ok(())
    }
}

pub fn contains_japanese(value: &str) -> bool {
    value.chars().any(is_japanese_character)
}

fn is_japanese_character(character: char) -> bool {
    matches!(
        character as u32,
        0x3005..=0x3007
            | 0x3040..=0x309F
            | 0x30A0..=0x30FF
            | 0x31F0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFF66..=0xFF9D
    )
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ColumnError {
    #[error("column header `{0}` was not found")]
    HeaderNotFound(String),

    #[error("column header `{header}` is ambiguous across columns {columns:?}")]
    AmbiguousHeader { header: String, columns: Vec<usize> },

    #[error("column {column} is out of bounds for {column_count} columns")]
    ColumnOutOfBounds { column: usize, column_count: usize },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn open(source: &str) -> CsvDocument {
        let directory = tempdir().unwrap();
        let path = directory.path().join("data.csv");
        fs::write(&path, source).unwrap();
        CsvDocument::open(path).unwrap()
    }

    #[test]
    fn header_lookup_rejects_missing_and_ambiguous_names() {
        let document = open("名前,名前,年齢\n田中,山田,20\n");

        assert_eq!(document.column_indices_by_header("名前"), vec![0, 1]);
        assert!(matches!(
            document.column_index_by_header("名前"),
            Err(ColumnError::AmbiguousHeader { .. })
        ));
        assert_eq!(
            document.column_index_by_header("住所"),
            Err(ColumnError::HeaderNotFound("住所".into()))
        );
        assert_eq!(document.column_index_by_header("年齢").unwrap(), 2);
    }

    #[test]
    fn type_checks_do_not_mutate_canonical_csv_text() {
        let document = open("名前,年齢,有効\n田中,20,true\n山田,abc,false\n");

        let text = document
            .validate_column_type_by_header("名前", ColumnType::String)
            .unwrap();
        assert!(text.is_valid());
        assert_eq!(text.checked_cells(), 2);

        let integer = document
            .validate_column_type_by_header("年齢", ColumnType::Integer)
            .unwrap();
        assert!(!integer.is_valid());
        assert_eq!(integer.mismatches()[0].reference().to_string(), "B3");
        assert_eq!(integer.mismatches()[0].value(), "abc");
        assert_eq!(document.cell_a1("B3").unwrap(), Some("abc"));
    }

    #[test]
    fn japanese_check_reports_matching_and_nonmatching_cells() {
        let document = open("名前\n田中太郎\nAlice\n山田 Taro\n\"\"\n");
        let report = document.check_column_japanese_by_header("名前").unwrap();

        assert_eq!(report.checked_cells(), 4);
        assert_eq!(
            report
                .matches()
                .iter()
                .map(|cell| cell.reference().to_string())
                .collect::<Vec<_>>(),
            vec!["A2", "A4"]
        );
        assert_eq!(
            report
                .mismatches()
                .iter()
                .map(|cell| cell.reference().to_string())
                .collect::<Vec<_>>(),
            vec!["A3", "A5"]
        );
        assert!(!report.all_match());
    }

    #[test]
    fn japanese_character_detection_covers_common_scripts() {
        for value in ["ひらがな", "カタカナ", "ﾊﾝｶｸ", "日本語", "山田 Taro", "々"]
        {
            assert!(contains_japanese(value), "{value}");
        }
        for value in ["Alice", "123", "", "hello-world"] {
            assert!(!contains_japanese(value), "{value}");
        }
    }
}
