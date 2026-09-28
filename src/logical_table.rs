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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalProject {
    pub tables: Vec<LogicalTable>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalTable {
    /// The exact ordered header shared by every source in this table.
    pub schema: Vec<String>,
    /// Stable project source ids that contribute files to this table.
    pub source_ids: Vec<String>,
    /// CSV files supplied by each source id, in deterministic load order.
    pub source_files: Vec<SourceFile>,
    /// Source selected for implicit row creation, when explicitly configured.
    pub default_write_target: Option<String>,
    /// Stable indices into `rows`; changing this order does not change row provenance.
    pub display_order: Vec<usize>,
    pub rows: Vec<LogicalRow>,
    move_undo: Vec<RowMove>,
    move_redo: Vec<RowMove>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalRow {
    pub values: Vec<String>,
    pub origin: SourceRecord,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRecord {
    pub source_id: String,
    pub path: PathBuf,
    /// Zero-based CSV data-record index, excluding the header record.
    pub record_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub source_id: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Load every declared CSV source and group files by exact ordered header.
    /// Directory traversal follows each source's `recursive` setting.
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
    /// Select the project source that implicit row creation should target.
    /// Only sources already contributing to this exact-schema table are valid.
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

    pub fn clear_default_write_target(&mut self) {
        self.default_write_target = None;
    }

    /// Resolve an explicit row target, or the configured default when omitted.
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

    /// Append a row to the resolved source CSV and update this logical table.
    /// A source id with multiple CSV files is rejected as ambiguous.
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

    /// Move an existing record to another source in this logical table.
    /// Both CSV updates are saved as one recoverable operation.
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

    pub fn can_undo_move(&self) -> bool {
        !self.move_undo.is_empty()
    }

    pub fn can_redo_move(&self) -> bool {
        !self.move_redo.is_empty()
    }

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

fn collect_csv_files(root: &Path, recursive: bool) -> Result<Vec<PathBuf>, LogicalLoadError> {
    if !root.is_dir() {
        return Err(LogicalLoadError::MissingDirectory(root.to_path_buf()));
    }
    let mut paths = Vec::new();
    collect_csv_files_into(root, recursive, &mut paths)?;
    paths.sort();
    Ok(paths)
}

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

fn is_csv(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("csv"))
}

#[derive(Debug, Error)]
pub enum LogicalLoadError {
    #[error("project source directory does not exist: {0}")]
    MissingDirectory(PathBuf),
    #[error("CSV has no header record: {0}")]
    MissingHeader(PathBuf),
    #[error("failed to read `{path}`: {source}")]
    ReadDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to load CSV `{path}`: {message}")]
    Csv { path: PathBuf, message: String },
}

#[derive(Debug, Error, PartialEq, Eq)]
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
