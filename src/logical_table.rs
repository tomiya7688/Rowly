//! Read-only logical views over project CSV sources.
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
    /// Source selected for implicit row creation, when explicitly configured.
    pub default_write_target: Option<String>,
    /// Stable indices into `rows`; changing this order does not change row provenance.
    pub display_order: Vec<usize>,
    pub rows: Vec<LogicalRow>,
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
                        default_write_target: None,
                        display_order: Vec::new(),
                        rows: Vec::new(),
                    });
                    index
                });
                let table = &mut tables[table_index];
                if !table.source_ids.contains(&source.id) {
                    table.source_ids.push(source.id.clone());
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
}
