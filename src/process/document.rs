use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::data::{read_csv, write_csv_utf8, SourceEncoding, Table};

#[derive(Debug)]
pub struct CsvDocument {
    path: PathBuf,
    source_encoding: SourceEncoding,
    table: Table,
    dirty: bool,
}

impl CsvDocument {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DocumentError> {
        let path = path.as_ref().to_path_buf();
        let loaded = read_csv(&path).map_err(|error| DocumentError::Open {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;

        Ok(Self {
            path,
            source_encoding: loaded.encoding,
            table: loaded.table,
            dirty: false,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn source_encoding(&self) -> SourceEncoding {
        self.source_encoding
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn row_count(&self) -> usize {
        self.table.row_count()
    }

    pub fn column_count(&self) -> usize {
        self.table.column_count()
    }

    pub fn cell(&self, row: usize, column: usize) -> Option<&str> {
        self.table.cell(row, column)
    }

    pub fn set_cell(
        &mut self,
        row: usize,
        column: usize,
        value: impl Into<String>,
    ) -> Result<(), DocumentError> {
        let changed = self
            .table
            .set_cell(row, column, value)
            .map_err(|error| DocumentError::Edit(error.to_string()))?;

        self.dirty |= changed;
        Ok(())
    }

    pub fn save(&mut self) -> Result<(), DocumentError> {
        write_csv_utf8(&self.path, &self.table).map_err(|error| DocumentError::Save {
            path: self.path.display().to_string(),
            message: error.to_string(),
        })?;

        self.source_encoding = SourceEncoding::Utf8;
        self.dirty = false;
        Ok(())
    }

    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<(), DocumentError> {
        let path = path.as_ref().to_path_buf();
        write_csv_utf8(&path, &self.table).map_err(|error| DocumentError::Save {
            path: path.display().to_string(),
            message: error.to_string(),
        })?;

        self.path = path;
        self.source_encoding = SourceEncoding::Utf8;
        self.dirty = false;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum DocumentError {
    #[error("failed to open CSV `{path}`: {message}")]
    Open { path: String, message: String },

    #[error("failed to edit CSV: {0}")]
    Edit(String),

    #[error("failed to save CSV `{path}`: {message}")]
    Save { path: String, message: String },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use encoding_rs::SHIFT_JIS;
    use tempfile::tempdir;

    use super::*;

    #[test]
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
